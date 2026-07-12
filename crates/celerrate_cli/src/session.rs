//! Startup: parse arguments, discover the Composer configuration, walk
//! what the project declares, load it through the `Vfs`, and set the four
//! inputs the semantic query consumes.
//!
//! Durability is not decoration. The stub index is HIGH: it changes when
//! the binary does. The project configuration is MEDIUM: it changes when
//! the lockfile does. The file bytes and the analyzed set are LOW: they
//! change on every keystroke.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::{ProjectConfiguration, ProjectDiscovery, ProjectNotice, discover};
use celerrate_source::FileId;
use celerrate_stubs::{StubBlobError, StubIndex, StubIndexInput, embedded_stub_index};
use celerrate_vfs::{Vfs, enumerate_php_files};
use salsa::Setter as _;

use crate::database::AnalysisDatabase;

/// Something that must never happen happened. The run continues, the
/// report prints at the end, and the exit code is 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalError {
    /// The embedded stub blob did not decode. The run falls back to an
    /// empty index and reports; it never panics.
    StubBlobUndecodable(StubBlobError),
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

        let mut session = Self {
            database,
            vfs: Vfs::default(),
            discovery,
            configuration,
            stubs,
            files,
            sources: BTreeMap::new(),
            internal_errors,
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

    /// Makes the analyzed set exactly `paths`: bytes read for each, files
    /// that left the walk dropped. `SourceFile` has no deleted state, and
    /// a tombstone would leave the set lying about what it contains, so a
    /// departing file leaves the set outright.
    fn load(&mut self, paths: &[PathBuf]) {
        let mut wanted: BTreeMap<FileId, SourceFile> = BTreeMap::new();
        for path in paths {
            let contents = std::fs::read(path).unwrap_or_default();
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
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_project::{PhpVersion, ProjectNotice};

    use super::Session;

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
}
