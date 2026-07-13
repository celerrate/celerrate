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
use celerrate_vfs::{Vfs, enumerate_php_files};
use salsa::Setter as _;

use crate::analysis::{AnalysisInputs, AnalysisOutcome};
use crate::cache::pack::PackHeader;
use crate::cache::snapshot::{CacheSnapshot, SnapshotCache};
use crate::database::AnalysisDatabase;
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
}

impl Session {
    /// Discovers, walks, loads, and sets the inputs. Never fails: a
    /// missing manifest is a notice, an undecodable stub blob is an
    /// internal error, and neither stops the run.
    pub fn start(root: &Path) -> Self {
        let mut internal_errors = Vec::new();
        let database = AnalysisDatabase::default();
        let discovery = discover(root);

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

        let cache_directory = root.join(".celerrate").join("cache");
        let cache_loaded_range = discovery.php_version_range;
        let cache = Arc::new(CacheSnapshot::load(
            &cache_directory,
            &PackHeader::current(cache_loaded_range),
        ));
        let _ = ArtifactCacheInput::builder(CacheHandle(Arc::new(SnapshotCache(cache.clone()))))
            .durability(salsa::Durability::HIGH)
            .new(&database);

        let mut session = Self {
            database,
            vfs: Vfs::default(),
            discovery,
            configuration,
            stubs,
            files,
            sources: BTreeMap::new(),
            internal_errors,
            cache,
            cache_directory,
            cache_loaded_range,
        };
        let paths = enumerate_php_files(&session.discovery.walk_roots());
        session.load(&paths);
        session
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
        AnalysisInputs {
            database: self.database.clone(),
            files: self.files,
            stubs: self.stubs,
            configuration: self.configuration,
            reported: self.reported_files(),
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
    /// recomputes those.
    pub fn forget_analysis_errors(&mut self) {
        self.internal_errors.retain(|error| {
            !matches!(
                error,
                InternalError::FilePanicked { .. } | InternalError::AnalysisPanicked
            )
        });
    }

    /// Makes the analyzed set exactly `paths`: bytes read for each, files
    /// that left the walk dropped. `SourceFile` has no deleted state, and
    /// a tombstone would leave the set lying about what it contains, so a
    /// departing file leaves the set outright.
    fn load(&mut self, paths: &[PathBuf]) {
        // This load decides, for the file list it is given, exactly which
        // files cannot be read. The previous load's verdict is superseded,
        // not added to: a lockfile saved twice under `--watch` re-runs
        // discovery twice, and an unreadable file must be reported once,
        // not once per save.
        self.internal_errors
            .retain(|error| !matches!(error, InternalError::FileUnreadable { .. }));
        let mut wanted: BTreeMap<FileId, SourceFile> = BTreeMap::new();
        for path in paths {
            let contents = match std::fs::read(path) {
                Ok(contents) => contents,
                Err(error) => {
                    self.internal_errors.push(InternalError::FileUnreadable {
                        path: path.clone(),
                        reason: error.to_string(),
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
            if !is_php(path) {
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
        path == root.join("composer.json") || path == root.join("composer.lock")
    }

    /// A changed lockfile re-runs discovery and rebuilds the
    /// configuration. The vendor tree is never watched: thousands of files
    /// that only move when the lockfile does, and this is what a lockfile
    /// change triggers anyway.
    fn rediscover(&mut self) {
        let root = self.discovery.root.clone();
        let discovery = discover(&root);
        if discovery.php_version_range != self.discovery.php_version_range {
            self.configuration
                .set_php_version_range(&mut self.database)
                .to(discovery.php_version_range);
        }
        self.discovery = discovery;
        let paths = enumerate_php_files(&self.discovery.walk_roots());
        self.load(&paths);
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
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

    use celerrate_project::{PhpVersion, ProjectNotice};

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
}
