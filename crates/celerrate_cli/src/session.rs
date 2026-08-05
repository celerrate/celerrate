//! Startup: parse arguments, discover the Composer configuration, walk
//! what the project declares, load it through the `Vfs`, and set the four
//! inputs the semantic query consumes.
//!
//! Durability is not decoration. The stub index is HIGH: it changes when
//! the binary does. The project configuration is MEDIUM: it changes when
//! the lockfile does. The file bytes and the analyzed set are LOW: they
//! change on every keystroke.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{
    FileOrigin, PhpVersionRange, ProjectConfiguration, ProjectDiscovery, ProjectNotice, discover,
};
use celerrate_semantics::{ArtifactCacheInput, CacheHandle};
use celerrate_source::FileId;
use celerrate_stubs::{StubBlobError, StubIndex, StubIndexInput, embedded_stub_index};
use celerrate_types::{TypedCacheHandle, TypedCacheInput};
use celerrate_vfs::{Vfs, Walk, enumerate_php_files};
use salsa::Setter as _;

use crate::analysis::{AnalysisInputs, AnalysisOutcome};
use crate::cache::pack::PackHeader;
use crate::cache::snapshot::{CacheSnapshot, SnapshotCache};
use crate::cache::statistics::CacheStatistics;
use crate::database::AnalysisDatabase;
use crate::phases::PhaseTimings;
use crate::plugins::{RegisteredPlugins, plugin_set_digest, register_core_rules, register_plugins};
use crate::watch::{InputMutation, reconcile};

/// Something that must never happen happened. The run continues, the
/// report prints at the end, and the exit code is 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalError {
    /// The embedded stub blob did not decode. The run falls back to an
    /// empty index and reports; it never panics.
    StubBlobUndecodable(StubBlobError),
    /// A walked file could not be read (permission denied, a dangling
    /// symlink, a deletion race between the walk and the load). The
    /// file still enters the analyzed set with empty bytes; only the
    /// `io::Error`, rendered as a string, is kept, since `InternalError`
    /// derives `Clone`, `PartialEq`, and `Eq` and `std::io::Error` does
    /// not.
    FileUnreadable { path: PathBuf, reason: String },
    /// A directory the walk found and could not look inside. Everything it
    /// could reach is still analyzed, but the run no longer claims to have
    /// seen the whole project: skipping it in silence was a green build
    /// over a project only half read. Like `FileUnreadable`, this is the
    /// environment's condition and not Celerrate's bug.
    DirectoryUnreadable { path: PathBuf, reason: String },
    /// A path `--watch` asked the operating system to observe, and the
    /// operating system refused: a declared autoload directory that has not
    /// been created yet, or a watch budget that is exhausted. Like
    /// `FileUnreadable` this is the environment's condition and not
    /// Celerrate's bug, and like it, only the refusal rendered as a string
    /// is kept: the watch's own error type belongs to the watch, and
    /// nothing else may learn about it. The run continues over the paths it
    /// could register and says which one it could not, because a watch that
    /// is partly dead must never look like a watch that is whole.
    PathUnwatchable { path: PathBuf, reason: String },
    /// Analyzing one file panicked. Every other file still reports.
    FilePanicked { file: FileId },
    /// The analysis loop itself panicked, outside any file's guard.
    AnalysisPanicked,
    /// A planned fix could not be applied to its file's text. This is
    /// Celerrate's bug: the planner admitted an edit set the applier
    /// refused.
    FixUnappliable { file: FileId, reason: String },
    /// The patched text could not be written back to disk. The
    /// environment's condition, like `FileUnreadable`: named, but no
    /// bug report invited.
    FixWriteFailed { path: PathBuf, reason: String },
    /// Rich rendering of one diagnostic failed; it was shown in the
    /// minimal one-line format instead. Always a Celerrate bug: the
    /// fallback keeps the report intact, the report invites the issue.
    DiagnosticRenderFailed {
        identifier: String,
        location: String,
    },
}

/// Everything one project needs to be analyzed, and everything `--watch`
/// mutates between cycles.
pub struct Session {
    pub database: AnalysisDatabase,
    pub vfs: Vfs,
    pub discovery: ProjectDiscovery,
    pub configuration: ProjectConfiguration,
    pub stubs: StubIndexInput,
    pub files: AnalyzedFileSet,
    /// Every analyzed file, by identity: the map `--watch` mutates and
    /// the renderer resolves a diagnostic's `FileId` through.
    pub sources: BTreeMap<FileId, SourceFile>,
    pub internal_errors: Vec<InternalError>,
    /// The cache snapshot this session was seeded from: consulted for
    /// verdicts on the first pass, compared against on persist.
    pub cache: Arc<CacheSnapshot>,
    /// Where this project's packs live: `<root>/.celerrate/cache`.
    pub cache_directory: PathBuf,
    /// The PHP version range the snapshot validated against. Under
    /// `--watch` a manifest edit can move the range at runtime, and
    /// range-dependent verdicts must not survive that: passes compare
    /// this against the current range before attaching them.
    pub cache_loaded_range: PhpVersionRange,
    /// The session's cache counters, shared with the registered
    /// `SnapshotCache` and with every `AnalysisInputs` clone. Never
    /// read by analysis; rendered to stderr on opt-in.
    pub statistics: Arc<CacheStatistics>,
    /// The session's per-phase timings, shared with the persist layer.
    /// Never read by analysis; rendered to stderr under `--verbose`.
    pub phases: Arc<PhaseTimings>,
    /// The plugins the composition root registered into the extension
    /// registries, and the ones it excluded. Set once, right after the
    /// database's other singleton inputs, before any query runs.
    pub plugins: RegisteredPlugins,
    /// The registered plugin-set digest (`plugins::plugin_set_digest`),
    /// computed once at startup and shared by every `PackHeader::current`
    /// call this session makes: load and persist must key packs on the
    /// same value, never recompute it independently.
    pub plugin_set_digest: [u8; 32],
    /// The configuration digest packs are keyed on this session
    /// (`configuration::configuration_digest` over the loaded model).
    pub configuration_digest: [u8; 32],
    /// The digest the snapshot was loaded under: `--watch` can move the
    /// live digest at runtime (a `celerrate.toml` edit), and verdicts
    /// persisted under the old configuration must not be served past it.
    pub cache_loaded_configuration_digest: [u8; 32],
    /// The loaded `celerrate.toml`, `None` when the project has none.
    /// Its diagnostics are reported and count toward the exit code, and
    /// `configuration_model` is what every consumer of its parsed content
    /// reads. `rediscover` reloads it whenever a save of it, `composer.json`,
    /// or `composer.lock` is absorbed under `--watch`.
    pub loaded_configuration: Option<crate::configuration::LoadedConfiguration>,
    /// The `[severity]` remap the per-file composition applies
    /// (`configuration::severity_remap`): identifier text to severity.
    /// Empty without a file; shared with every `AnalysisInputs` clone.
    pub severity_remap: Arc<BTreeMap<String, celerrate_diagnostics::Severity>>,
    /// The `celerrate-baseline.toml` at the project root, loaded once at
    /// startup and reloaded whenever `rediscover` runs. `None` when the
    /// project has none. CLI-layer only: never enters a salsa query.
    pub(crate) loaded_baseline: Option<crate::baseline::LoadedBaseline>,
}

impl Session {
    /// Discovers, walks, loads, and sets the inputs. Never fails: a
    /// missing manifest is a notice, an undecodable stub blob is an
    /// internal error, and neither stops the run.
    pub fn start(root: &Path) -> Self {
        // The same normalized form discovery would produce: paths are
        // interned and relativized against this exact value.
        let root = celerrate_vfs::normalize_path(root, root);
        let mut internal_errors = Vec::new();
        let database = AnalysisDatabase::default();
        let mut vfs = Vfs::default();
        // Loaded before discovery, deliberately: include/exclude and the
        // `php` override are discovery inputs now.
        // `celerrate.toml` therefore takes the first file identifier;
        // nothing reads the interning order, and rendering resolves
        // display paths through the VFS by identity.
        let loaded_configuration = crate::configuration::load(&root, &mut vfs);
        // The baseline is presentation, read alongside the configuration
        // and never entering a salsa query: resilience holds here too,
        // an unreadable file yields no entries plus a failure line.
        let loaded_baseline = crate::baseline::load(&root);
        let configuration_model = loaded_configuration
            .as_ref()
            .map(|loaded| loaded.configuration.clone())
            .unwrap_or_default();
        let discovery = discover(&root, &configuration_model);
        // Computed right after the model exists and before the cache
        // loads: load and persist must key packs on the same digest,
        // never recompute it independently (mirrors `plugin_set_digest`
        // below).
        let configuration_digest = crate::configuration::configuration_digest(&configuration_model);
        let severity_remap = Arc::new(crate::configuration::severity_remap(
            loaded_configuration.as_ref(),
        ));

        let index = match embedded_stub_index() {
            Ok(index) => index,
            Err(error) => {
                internal_errors.push(InternalError::StubBlobUndecodable(error));
                StubIndex::default()
            }
        };
        let stubs = StubIndexInput::builder(index)
            .durability(salsa::Durability::HIGH)
            .new(&database);
        let configuration = ProjectConfiguration::builder(discovery.php_version_range)
            .durability(salsa::Durability::MEDIUM)
            .new(&database);
        let files = AnalyzedFileSet::new(&database, Vec::new());

        let statistics = Arc::new(CacheStatistics::default());
        let phases = Arc::new(PhaseTimings::default());
        let cache_directory = root.join(".celerrate").join("cache");
        let cache_loaded_range = discovery.php_version_range;

        // Registration happens here, right after the database's other
        // singleton inputs and before any query runs: the registries
        // are themselves salsa singletons, set once per database. Also
        // ahead of the cache load below, so the plugin-set digest
        // (issue #60) can be derived from the effective, post-admission
        // set `register_plugins` actually produced, not a second,
        // independently-collected descriptor list.
        let plugins = register_plugins(&database);
        // The core rules register under their reserved identity, outside
        // the admitted plugin set the digest keys on: core behavior is
        // keyed by binary identity, not by the plugin-set digest. The
        // `[rules]` activation overrides ride along so the active set
        // reflects the loaded configuration from the first registration.
        register_core_rules(
            &database,
            &crate::configuration::rule_overrides(loaded_configuration.as_ref()),
        );
        // Computed once and threaded through: load and persist must key
        // packs on the same digest, never recompute it independently.
        let plugin_set_digest = plugin_set_digest(&plugins);
        let cache = Arc::new(CacheSnapshot::load(
            &cache_directory,
            &PackHeader::current(cache_loaded_range, plugin_set_digest, configuration_digest),
        ));
        let _ = ArtifactCacheInput::builder(CacheHandle(Arc::new(SnapshotCache {
            snapshot: cache.clone(),
            statistics: statistics.clone(),
        })))
        .durability(salsa::Durability::HIGH)
        .new(&database);
        // The typed-cache sibling: the same `SnapshotCache`, registered
        // a second time under the `celerrate_types`-owned trait, at the
        // same HIGH durability, reading it never invalidates anything
        // either.
        let _ = TypedCacheInput::builder(TypedCacheHandle(Arc::new(SnapshotCache {
            snapshot: cache.clone(),
            statistics: statistics.clone(),
        })))
        .durability(salsa::Durability::HIGH)
        .new(&database);

        let mut session = Self {
            database,
            vfs,
            discovery,
            configuration,
            stubs,
            files,
            sources: BTreeMap::new(),
            internal_errors,
            cache,
            cache_directory,
            cache_loaded_range,
            statistics,
            phases,
            plugins,
            plugin_set_digest,
            configuration_digest,
            cache_loaded_configuration_digest: configuration_digest,
            loaded_configuration,
            severity_remap,
            loaded_baseline,
        };
        // Wall-clock reads, legal here: `start` is orchestration, never
        // a salsa query, and the readings feed only the verbose channel.
        let started = std::time::Instant::now();
        let walk = enumerate_php_files(
            &session.discovery.walk_roots(),
            &session.discovery.excluded_roots,
        );
        session
            .phases
            .record(crate::phases::Phase::Walk, started.elapsed());
        let started = std::time::Instant::now();
        session.load(&walk);
        session
            .phases
            .record(crate::phases::Phase::ReadAndSetInputs, started.elapsed());
        session
    }

    /// The parsed configuration model, or the default when the project
    /// has no file: what every consumer of configuration data reads.
    pub(crate) fn configuration_model(&self) -> celerrate_config::Configuration {
        self.loaded_configuration
            .as_ref()
            .map(|loaded| loaded.configuration.clone())
            .unwrap_or_default()
    }

    /// The discovery notices, rendered as their own spanless block: a
    /// project-level finding has no span, and anchoring
    /// `MISSING_COMPOSER_MANIFEST` to `composer.json:1:1` would be a
    /// fiction about a file that by definition does not exist.
    pub fn notices(&self) -> &[ProjectNotice] {
        &self.discovery.notices
    }

    /// The fan-out's view of the session: a fresh database handle over the
    /// same storage, the three input handles, and the files the report
    /// speaks about.
    pub fn inputs(&self) -> AnalysisInputs {
        let current_range = self.configuration.php_version_range(&self.database);
        AnalysisInputs {
            database: self.database.clone(),
            files: self.files,
            stubs: self.stubs,
            configuration: self.configuration,
            reported: self.reported_files(),
            cache: if current_range == self.cache_loaded_range
                && self.configuration_digest == self.cache_loaded_configuration_digest
            {
                self.cache.clone()
            } else {
                Arc::new(CacheSnapshot::default())
            },
            statistics: self.statistics.clone(),
            severity_remap: self.severity_remap.clone(),
        }
    }

    /// The files whose diagnostics the run reports: the project's own.
    ///
    /// An installed dependency's files stay in the analyzed set, because
    /// their symbols are what make `use Vendor\Package\Thing;` resolve.
    /// What they do not do is speak: a third-party finding is not the
    /// user's to fix, and on a real Composer project it would drown the
    /// report and fail the build on code the user does not own. This is
    /// what `FileOrigin` and `ProjectDiscovery::classify` were built for.
    ///
    /// Derived on demand rather than cached, and deliberately: it is one
    /// prefix comparison per file against a set that only `load` and
    /// `apply` can move, next to a full re-analysis. A cache here would
    /// buy nothing and would have to be invalidated from both, which is
    /// exactly the kind of second place a wrong answer hides in.
    ///
    /// A file the `Vfs` cannot name counts as the project's. Erring that
    /// way reports a diagnostic that might have been a dependency's;
    /// erring the other way silences one that is certainly the user's.
    fn reported_files(&self) -> Arc<[SourceFile]> {
        self.sources
            .iter()
            .filter(|(file, _)| {
                self.vfs
                    .path(**file)
                    .is_none_or(|path| self.discovery.classify(path) == FileOrigin::Project)
            })
            .map(|(_, source)| *source)
            .collect()
    }

    /// Absorbs a completed pass: its panicked files become internal
    /// errors, and the exit code becomes 2.
    pub fn absorb_outcome(&mut self, outcome: &AnalysisOutcome) {
        for &file in &outcome.panicked {
            self.internal_errors
                .push(InternalError::FilePanicked { file });
        }
    }

    /// Absorbs the rich-rendering failures of one report, after the
    /// report is written and before the internal errors render.
    pub fn absorb_render_failures(
        &mut self,
        failures: Vec<celerrate_rules::render::RenderFailure>,
    ) {
        for failure in failures {
            self.internal_errors
                .push(InternalError::DiagnosticRenderFailed {
                    identifier: failure.id.as_str().to_owned(),
                    location: failure.location,
                });
        }
    }

    /// Drops what the last analysis pass, and only it, concluded.
    ///
    /// A single `check` never needs this: it analyzes once. `--watch`
    /// analyzes on every save and reprints the whole picture each time, so
    /// without it a file that panics every cycle would add a line to the
    /// internal-error block on every keystroke, and the block would become
    /// precisely the stale log of past edits the format exists to avoid.
    ///
    /// Only the pass's own verdicts go. An undecodable stub blob and an
    /// unreadable file describe the loaded session, not the pass, and they
    /// do not stop being true because a file was saved: `load` is what
    /// recomputes those. A rich-rendering failure is a verdict about the
    /// picture the pass produced, and every cycle renders its picture
    /// afresh, so it goes with the pass that produced it.
    pub fn forget_analysis_errors(&mut self) {
        self.internal_errors.retain(|error| {
            !matches!(
                error,
                InternalError::FilePanicked { .. }
                    | InternalError::AnalysisPanicked
                    | InternalError::DiagnosticRenderFailed { .. }
            )
        });
    }

    /// Makes the analyzed set exactly what the walk reached: bytes read for
    /// each file, files that left the walk dropped. `SourceFile` has no
    /// deleted state, and a tombstone would leave the set lying about what
    /// it contains, so a departing file leaves the set outright.
    ///
    /// The walk's own refusals arrive with it, because one load is one
    /// complete verdict about the filesystem it looked at: these are the
    /// files it could not read, and these are the directories it could not
    /// open.
    fn load(&mut self, walk: &Walk) {
        use rayon::prelude::*;

        // This load decides, for the walk it is given, exactly what could
        // not be read. The previous load's verdict is superseded, not added
        // to: a lockfile saved twice under `--watch` re-runs discovery
        // twice, and an unreadable path must be reported once, not once per
        // save.
        self.internal_errors.retain(|error| {
            !matches!(
                error,
                InternalError::FileUnreadable { .. } | InternalError::DirectoryUnreadable { .. }
            )
        });
        for directory in &walk.unreadable_directories {
            self.internal_errors
                .push(InternalError::DirectoryUnreadable {
                    path: directory.path.clone(),
                    reason: directory.reason.clone(),
                });
        }
        // The reads fan out; everything that mutates (`internal_errors`,
        // the VFS, the salsa inputs) stays on this thread, in walk
        // order. Rayon's indexed `collect` preserves input order, so
        // the zip below reunites each path with its own read and the
        // recorded failures keep their serial-era order.
        let read_outcomes: Vec<Result<Vec<u8>, String>> = walk
            .files
            .par_iter()
            .map(|path| std::fs::read(path).map_err(|error| error.to_string()))
            .collect();
        let mut wanted: BTreeMap<FileId, SourceFile> = BTreeMap::new();
        for (path, outcome) in walk.files.iter().zip(read_outcomes) {
            let contents = match outcome {
                Ok(contents) => contents,
                Err(reason) => {
                    self.internal_errors.push(InternalError::FileUnreadable {
                        path: path.clone(),
                        reason,
                    });
                    Vec::new()
                }
            };
            let file_id = self.vfs.set_file_contents(path, Some(contents.clone()));
            let source = match self.sources.get(&file_id) {
                Some(&source) => {
                    if source.bytes(&self.database) != &contents {
                        source.set_bytes(&mut self.database).to(contents);
                    }
                    source
                }
                None => SourceFile::new(&self.database, file_id, contents),
            };
            wanted.insert(file_id, source);
        }
        for departed in self.sources.keys() {
            if !wanted.contains_key(departed) {
                self.vfs.set_file_contents(
                    self.vfs
                        .path(*departed)
                        .map(Path::to_path_buf)
                        .unwrap_or_default()
                        .as_path(),
                    None,
                );
            }
        }
        let membership_changed = wanted.keys().ne(self.sources.keys());
        self.sources = wanted;
        if membership_changed {
            let members: Vec<SourceFile> = self.sources.values().copied().collect();
            self.files.set_files(&mut self.database).to(members);
        }
        // The VFS diff is consumed here so the watcher's `take_changes`
        // only ever sees what happened since the last cycle.
        let _ = self.vfs.take_changes();
    }

    /// Applies the mutations. A setter takes `&mut AnalysisDatabase`,
    /// which cancels every in-flight query on every other handle: that is
    /// how `--watch` restarts an analysis that a change overtook.
    pub fn apply(&mut self, mutations: &[InputMutation]) {
        let mut membership_changed = false;
        for mutation in mutations {
            match mutation {
                InputMutation::SetBytes { file, bytes } => {
                    if let Some(&source) = self.sources.get(file) {
                        source.set_bytes(&mut self.database).to(bytes.clone());
                    }
                }
                InputMutation::AddFile { file, bytes } => {
                    let source = SourceFile::new(&self.database, *file, bytes.clone());
                    self.sources.insert(*file, source);
                    membership_changed = true;
                }
                InputMutation::RemoveFile { file } => {
                    if self.sources.remove(file).is_some() {
                        membership_changed = true;
                    }
                }
            }
        }
        if membership_changed {
            let members: Vec<SourceFile> = self.sources.values().copied().collect();
            self.files.set_files(&mut self.database).to(members);
        }
    }

    /// Absorbs a coalesced burst of changed paths. A changed manifest or
    /// lockfile re-runs discovery, because the walk roots and the PHP
    /// version both come from it; everything else is a file change the
    /// VFS diffs and `reconcile` classifies.
    pub fn absorb(&mut self, changed: &[PathBuf]) {
        if changed.iter().any(|path| self.is_project_manifest(path)) {
            self.rediscover();
            return;
        }
        for path in changed {
            if !is_php(path) || self.discovery.is_excluded(path) {
                continue;
            }
            let contents = std::fs::read(path).ok();
            self.vfs.set_file_contents(path, contents);
        }
        let changes = self.vfs.take_changes();
        let analyzed: BTreeSet<FileId> = self.sources.keys().copied().collect();
        let mutations = reconcile(&changes, &analyzed);
        self.apply(&mutations);
    }

    fn is_project_manifest(&self, path: &Path) -> bool {
        let root = &self.discovery.root;
        path == root.join("composer.json")
            || path == root.join("composer.lock")
            || path == root.join("celerrate.toml")
            || path == root.join(crate::baseline::BASELINE_FILE_NAME)
    }

    /// A changed manifest, lockfile, or `celerrate.toml` re-runs discovery
    /// under the freshly reloaded configuration and rebuilds everything
    /// derived from it: the walk, the version range, the active set, the
    /// severity remap, and the cache digest. Configuration changes are
    /// rare; whole invalidation is the accepted cost (spec, rejected
    /// approach C).
    fn rediscover(&mut self) {
        let root = self.discovery.root.clone();
        self.loaded_configuration = crate::configuration::load(&root, &mut self.vfs);
        self.loaded_baseline = crate::baseline::load(&root);
        let model = self.configuration_model();
        let discovery = discover(&root, &model);
        if discovery.php_version_range != self.discovery.php_version_range {
            self.configuration
                .set_php_version_range(&mut self.database)
                .to(discovery.php_version_range);
        }
        self.discovery = discovery;
        self.refresh_configuration(&model);
        let walk =
            enumerate_php_files(&self.discovery.walk_roots(), &self.discovery.excluded_roots);
        self.load(&walk);
    }

    /// Refreshes the registration-time consumers of the configuration: the
    /// active set (through the registry setter, only when it actually
    /// moved, because setting a HIGH-durability input invalidates the
    /// world), the severity remap, and the digest packs are keyed on.
    fn refresh_configuration(&mut self, model: &celerrate_config::Configuration) {
        let overrides = crate::configuration::rule_overrides(self.loaded_configuration.as_ref());
        let desired = crate::plugins::core_registrations(&overrides);
        let desired_shape: Vec<(String, bool)> = desired
            .iter()
            .map(|registration| (registration.metadata.name.clone(), registration.active))
            .collect();
        let current_shape: Option<Vec<(String, bool)>> =
            celerrate_rules::RuleRegistry::try_get(&self.database).map(|registry| {
                registry
                    .registrations(&self.database)
                    .iter()
                    .map(|registration| (registration.metadata.name.clone(), registration.active))
                    .collect()
            });
        if current_shape.as_deref() != Some(desired_shape.as_slice())
            && let Some(registry) = celerrate_rules::RuleRegistry::try_get(&self.database)
        {
            registry.set_registrations(&mut self.database).to(desired);
        }
        self.severity_remap = Arc::new(crate::configuration::severity_remap(
            self.loaded_configuration.as_ref(),
        ));
        self.configuration_digest = crate::configuration::configuration_digest(model);
    }
}

/// Only PHP files enter the analyzed set, judged from the path alone: the
/// watcher reports every path under a walk root, directories and editor
/// swap files included, and a deleted file no longer exists to be
/// `is_file`-tested, yet still must be recognized as PHP so its removal
/// reaches the VFS.
fn is_php(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("php"))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectNotice};

    use super::{InternalError, Session};

    /// A project on disk, written into a temporary directory.
    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            let path = root.path().join(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }
        root
    }

    #[test]
    fn a_project_without_a_manifest_still_starts_and_says_so() {
        let root = project(&[("src/Kernel.php", "<?php class Kernel {}")]);
        let session = Session::start(root.path());

        assert_eq!(session.sources.len(), 1);
        assert!(
            session
                .notices()
                .contains(&ProjectNotice::MissingComposerManifest),
            "zero configuration works, and announces the fallback it took",
        );
        assert!(session.internal_errors.is_empty());
    }

    #[test]
    fn a_manifest_narrows_the_walk_and_sets_the_version() {
        let root = project(&[
            (
                "composer.json",
                r#"{"require": {"php": "^8.2"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
            ),
            ("src/Kernel.php", "<?php class Kernel {}"),
            ("scripts/build.php", "<?php echo 1;"),
        ]);
        let session = Session::start(root.path());

        assert_eq!(
            session.sources.len(),
            1,
            "only what the project declares is walked",
        );
        assert_eq!(
            session
                .configuration
                .php_version_range(&session.database)
                .minimum,
            PhpVersion::new(8, 2),
        );
    }

    #[test]
    fn the_stub_index_is_loaded_and_is_not_empty() {
        let root = project(&[("a.php", "<?php echo 1;")]);
        let session = Session::start(root.path());
        assert!(!session.stubs.index(&session.database).is_empty());
        assert!(session.internal_errors.is_empty());
    }

    /// A file the walk finds but cannot be read (permission denied) is
    /// recorded as an `InternalError`, not silently swallowed into an
    /// empty `SourceFile`. Unreadability is simulated by stripping all
    /// permission bits on Unix; this is the same real failure mode as a
    /// permission-denied file in a real project, so the test exercises
    /// the actual `std::fs::read` failure path, not a stand-in for it.
    /// It does not prove behavior for other read failures (a dangling
    /// symlink, a deletion race), since those are not exercised here,
    /// but they go through the same `Err(error)` arm.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_is_recorded_and_the_run_still_continues() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = project(&[("src/Locked.php", "<?php class Locked {}")]);
        let locked = root.path().join("src/Locked.php");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        let session = Session::start(root.path());

        // Restore permissions so the temporary directory can be cleaned
        // up regardless of test outcome.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(
            session.sources.len(),
            1,
            "the unreadable file still enters the analyzed set, with empty bytes",
        );
        let (_, source) = session.sources.iter().next().unwrap();
        assert!(source.bytes(&session.database).is_empty());

        assert_eq!(
            session.internal_errors.len(),
            1,
            "the read failure is recorded, not swallowed",
        );
        match &session.internal_errors[0] {
            super::InternalError::FileUnreadable { path, reason } => {
                assert_eq!(path, &locked);
                assert!(
                    !reason.is_empty(),
                    "the io::Error is rendered as a non-empty reason",
                );
            }
            other => panic!("expected FileUnreadable, got {other:?}"),
        }
    }

    /// The invariant parallelizing the read loop must preserve: several
    /// unreadable files are recorded in walk order (lexical path order,
    /// per `enumerate_php_files`), not in whatever order a fanned-out
    /// read happens to finish. Mirrors the fixture and the permission-bit
    /// guard of `an_unreadable_file_is_recorded_and_the_run_still_continues`
    /// above, over three root-level files so an out-of-order read would
    /// show up as a reordered `unreadable` vector.
    #[cfg(unix)]
    #[test]
    fn unreadable_files_are_recorded_in_walk_order() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = project(&[
            ("alpha.php", "<?php\n"),
            ("beta.php", "<?php\n"),
            ("gamma.php", "<?php\n"),
        ]);
        let alpha = root.path().join("alpha.php");
        let gamma = root.path().join("gamma.php");
        std::fs::set_permissions(&alpha, std::fs::Permissions::from_mode(0o000)).unwrap();
        std::fs::set_permissions(&gamma, std::fs::Permissions::from_mode(0o000)).unwrap();

        let session = Session::start(root.path());

        // Restore permissions so the temporary directory can be cleaned
        // up regardless of test outcome.
        std::fs::set_permissions(&alpha, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(&gamma, std::fs::Permissions::from_mode(0o644)).unwrap();

        let unreadable: Vec<String> = session
            .internal_errors
            .iter()
            .filter_map(|error| match error {
                InternalError::FileUnreadable { path, .. } => {
                    Some(path.file_name().unwrap().to_string_lossy().into_owned())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            unreadable,
            vec!["alpha.php", "gamma.php"],
            "the recorded failures keep the walk's lexical order",
        );
    }

    /// `--watch` re-analyzes on every save, and every cycle reprints the
    /// whole picture. A file that panics each time must therefore be
    /// reported once per picture, not once per save: otherwise the
    /// internal-error block grows a duplicate line on every keystroke and
    /// becomes exactly the stale log of past edits the format forbids.
    #[test]
    fn successive_passes_replace_the_previous_panics_rather_than_piling_onto_them() {
        let root = project(&[("Broken.php", "<?php echo 1;")]);
        let mut session = Session::start(root.path());
        let file = *session.sources.keys().next().unwrap();

        let outcome = crate::analysis::AnalysisOutcome {
            diagnostics: Vec::new(),
            panicked: vec![file],
        };
        for _ in 0..3 {
            session.forget_analysis_errors();
            session.absorb_outcome(&outcome);
        }

        assert_eq!(
            session.internal_errors.len(),
            1,
            "three cycles over one panicking file is one report, not three: {:?}",
            session.internal_errors,
        );
    }

    /// The errors that describe the loaded session, rather than a single
    /// analysis pass, are still true when the next pass starts: an
    /// undecodable stub blob does not become decodable because a file was
    /// saved.
    #[test]
    fn forgetting_a_passs_panics_leaves_the_sessions_own_errors_alone() {
        let root = project(&[("a.php", "<?php echo 1;")]);
        let mut session = Session::start(root.path());
        session
            .internal_errors
            .push(super::InternalError::StubBlobUndecodable(
                celerrate_stubs::StubBlobError::BadMagic,
            ));
        session.internal_errors.push(InternalError::FileUnreadable {
            path: root.path().join("Locked.php"),
            reason: "Permission denied (os error 13)".to_owned(),
        });
        session
            .internal_errors
            .push(InternalError::AnalysisPanicked);

        session.forget_analysis_errors();

        assert_eq!(
            session.internal_errors.len(),
            2,
            "the pass's panic goes, the session's own errors stay: {:?}",
            session.internal_errors,
        );
    }

    /// A render fallback is a verdict about the picture the pass produced,
    /// not about the loaded session, so it goes with the pass: otherwise
    /// `--watch` would add a line to the internal-error block on every
    /// cycle a diagnostic keeps failing to render, exactly the stale log
    /// the function exists to prevent.
    #[test]
    fn forgetting_a_passs_render_failures_leaves_the_sessions_own_errors_alone() {
        let root = project(&[("a.php", "<?php echo 1;")]);
        let mut session = Session::start(root.path());
        session
            .internal_errors
            .push(super::InternalError::StubBlobUndecodable(
                celerrate_stubs::StubBlobError::BadMagic,
            ));
        session
            .internal_errors
            .push(InternalError::DiagnosticRenderFailed {
                identifier: "CEL0018".to_owned(),
                location: "src/Kernel.php:4:22".to_owned(),
            });

        session.forget_analysis_errors();

        assert_eq!(
            session.internal_errors.len(),
            1,
            "the pass's render failure goes, the session's own errors stay: {:?}",
            session.internal_errors,
        );
    }

    #[test]
    fn absorbing_a_new_file_grows_the_analyzed_set() {
        let root = project(&[("a.php", "<?php class A {}")]);
        let mut session = Session::start(root.path());
        assert_eq!(session.sources.len(), 1);

        let added = root.path().join("b.php");
        std::fs::write(&added, "<?php class B {}").unwrap();
        session.absorb(&[added]);

        assert_eq!(session.sources.len(), 2);
        assert_eq!(session.files.files(&session.database).len(), 2);
    }

    #[test]
    fn absorbing_a_deletion_shrinks_the_analyzed_set() {
        let root = project(&[("a.php", "<?php class A {}"), ("b.php", "<?php class B {}")]);
        let mut session = Session::start(root.path());
        assert_eq!(session.sources.len(), 2);

        let removed = root.path().join("b.php");
        std::fs::remove_file(&removed).unwrap();
        session.absorb(&[removed]);

        assert_eq!(session.sources.len(), 1);
        assert_eq!(session.files.files(&session.database).len(), 1);
    }

    /// A change under an excluded root must not re-enter the analyzed
    /// set: the exclusion `celerrate.toml` declared is not a fact only
    /// the initial walk honors, it is a standing fact about what the
    /// project is. The sibling path proves the test isn't passing for
    /// the wrong reason — a session that absorbed nothing at all would
    /// also show the excluded path missing.
    #[test]
    fn absorbing_a_path_under_an_excluded_root_does_not_enter_the_analyzed_set() {
        let root = project(&[
            ("src/Kept.php", "<?php class Kept {}"),
            (
                "celerrate.toml",
                "[project]\nexclude = [\"src/Generated\"]\n",
            ),
        ]);
        let mut session = Session::start(root.path());
        assert_eq!(session.sources.len(), 1);

        let generated = root.path().join("src/Generated");
        std::fs::create_dir_all(&generated).unwrap();
        let excluded = generated.join("Machine.php");
        std::fs::write(&excluded, "<?php class Machine {}").unwrap();
        let sibling = root.path().join("src/Sibling.php");
        std::fs::write(&sibling, "<?php class Sibling {}").unwrap();

        session.absorb(&[excluded.clone(), sibling.clone()]);

        assert_eq!(
            session.sources.len(),
            2,
            "only the sibling path joins; the excluded path does not",
        );
        let analyzed_paths: Vec<_> = session
            .sources
            .keys()
            .filter_map(|&file| session.vfs.path(file).map(|path| path.to_path_buf()))
            .collect();
        assert!(
            analyzed_paths.contains(&sibling),
            "the sibling path must be in the analyzed set: {analyzed_paths:?}",
        );
        assert!(
            !analyzed_paths.contains(&excluded),
            "the excluded path must never be in the analyzed set: {analyzed_paths:?}",
        );
    }

    #[test]
    fn absorbing_a_lockfile_change_reruns_discovery() {
        let root = project(&[
            (
                "composer.json",
                r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
            ),
            ("src/Kernel.php", "<?php namespace App; class Kernel {}"),
        ]);
        let mut session = Session::start(root.path());
        assert_eq!(
            session
                .configuration
                .php_version_range(&session.database)
                .minimum,
            PhpVersion::new(8, 1),
        );

        let manifest = root.path().join("composer.json");
        std::fs::write(
            &manifest,
            r#"{"require": {"php": "^8.4"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
        )
        .unwrap();
        session.absorb(&[manifest]);

        assert_eq!(
            session
                .configuration
                .php_version_range(&session.database)
                .minimum,
            PhpVersion::new(8, 4),
        );
    }

    /// The part 2 wiring promise, at the session level: a `celerrate.toml`
    /// saved mid-watch is a manifest event like `composer.json` and
    /// `composer.lock`, and `absorb` reconfigures the session from it,
    /// reaching both the salsa PHP-version input and the rule registry.
    #[test]
    fn absorbing_a_configuration_change_reconfigures_the_session() {
        let root = project(&[
            (
                "composer.json",
                r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
            ),
            ("src/Kernel.php", "<?php namespace App; class Kernel {}"),
        ]);
        let mut session = Session::start(root.path());
        let original_digest = session.configuration_digest;

        let configuration = root.path().join("celerrate.toml");
        std::fs::write(
            &configuration,
            "[project]\nphp = \"8.3\"\n\n[rules.null-dereference]\nenabled = false\n",
        )
        .unwrap();
        session.absorb(&[configuration]);

        assert_eq!(
            session.configuration.php_version_range(&session.database),
            PhpVersionRange::point(PhpVersion::new(8, 3)),
            "the php override reached the salsa input",
        );
        // The digest normalizes over `[rules]` and `[severity]` only (spec
        // section 2), so it does move here, but only because of the
        // `[rules]` table riding alongside the `[project]` override: it
        // lands on exactly the digest that same `[rules]` table alone
        // would produce, proving the `[project]` table contributed
        // nothing to it.
        assert_ne!(
            session.configuration_digest, original_digest,
            "the [rules] table riding alongside the [project] override does move the digest",
        );
        let (rules_only, _) = celerrate_config::parse(
            celerrate_source::FileId::new(0),
            "[rules.null-dereference]\nenabled = false\n",
        );
        assert_eq!(
            session.configuration_digest,
            crate::configuration::configuration_digest(&rules_only),
            "the [project] table contributes nothing of its own to the digest",
        );
        let registry = celerrate_rules::RuleRegistry::try_get(&session.database)
            .expect("the registry is set at startup");
        let null_dereference = registry
            .registrations(&session.database)
            .iter()
            .find(|registration| registration.metadata.name == "null-dereference")
            .expect("the rule is registered");
        assert!(!null_dereference.active, "the disable reached the registry");
    }

    /// A `[rules]` change moves the configuration digest, and the digest
    /// the loaded snapshot was seeded under stays put: `inputs()` compares
    /// the two and must serve nothing stale rather than the pack a now
    /// stale configuration produced.
    #[test]
    fn absorbing_a_rules_change_moves_the_digest_and_gates_the_cache() {
        let root = project(&[
            (
                "composer.json",
                r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
            ),
            ("src/Kernel.php", "<?php namespace App; class Kernel {}"),
        ]);
        let mut session = Session::start(root.path());
        let original_digest = session.configuration_digest;

        let configuration = root.path().join("celerrate.toml");
        std::fs::write(
            &configuration,
            "[rules.null-dereference]\nenabled = false\n",
        )
        .unwrap();
        session.absorb(&[configuration]);

        assert_ne!(session.configuration_digest, original_digest);
        assert_eq!(
            session.cache_loaded_configuration_digest, original_digest,
            "the loaded snapshot keeps its own digest, so inputs() serves nothing stale",
        );
    }

    /// Deleting `celerrate.toml` mid-watch returns the session to zero
    /// configuration: `loaded_configuration` goes back to `None`, and every
    /// `Default`-tier rule the file had disabled is active again.
    #[test]
    fn a_deleted_configuration_file_returns_the_session_to_defaults() {
        let root = project(&[
            (
                "composer.json",
                r#"{"require": {"php": "^8.1"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#,
            ),
            (
                "celerrate.toml",
                "[rules.null-dereference]\nenabled = false\n",
            ),
            ("src/Kernel.php", "<?php namespace App; class Kernel {}"),
        ]);
        let mut session = Session::start(root.path());
        assert!(session.loaded_configuration.is_some());

        let configuration = root.path().join("celerrate.toml");
        std::fs::remove_file(&configuration).unwrap();
        session.absorb(&[configuration]);

        assert!(session.loaded_configuration.is_none());
        let registry = celerrate_rules::RuleRegistry::try_get(&session.database).unwrap();
        assert!(
            registry
                .registrations(&session.database)
                .iter()
                .all(|registration| registration.active),
            "every Default-tier rule is active again",
        );
    }
}
