use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use celerrate_source::FileId;

use crate::Vfs;

/// A directory the walk found but could not look inside: permission
/// denied, or a path that stopped resolving under it.
///
/// Only the `io::Error` rendered as a string is kept, so that the whole
/// walk can derive `Clone`, `PartialEq` and `Eq`, which `std::io::Error`
/// does not.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnreadableDirectory {
    pub path: PathBuf,
    pub reason: String,
}

/// What one walk found: the PHP files it could reach, and the directories
/// it could not look inside.
///
/// The second list exists because skipping a directory in silence is a lie
/// of omission. An unreadable `src/` used to vanish from the analysis
/// without a word, and the run then exited zero over a project it had only
/// half read. The walk still never fails, but it no longer pretends it saw
/// everything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Walk {
    /// Sorted and deduplicated.
    pub files: Vec<PathBuf>,
    /// Sorted and deduplicated, so that one directory reached under two
    /// overlapping roots is reported once.
    pub unreadable_directories: Vec<UnreadableDirectory>,
}

/// Enumerates the PHP files under a set of walk roots, sorted and
/// deduplicated, and reports the directories it could not read. A root
/// that is a file is included as-is (autoload `files` and `classmap`
/// entries may name single files of any extension); a root that is a
/// directory is walked recursively for `.php` files
/// (ASCII-case-insensitive). Symbolic links to directories are followed
/// with cycle protection.
///
/// A root that does not exist is skipped in silence: a declared autoload
/// directory that has not been created yet is ordinary. A directory that
/// exists and cannot be read is not, and it is reported. Enumeration never
/// fails either way.
pub fn enumerate_php_files(roots: &[PathBuf]) -> Walk {
    let mut files = BTreeSet::new();
    let mut unreadable = BTreeSet::new();
    let mut visited_directories = BTreeSet::new();
    for root in roots {
        if root.is_file() {
            files.insert(root.clone());
        } else if root.is_dir() {
            walk_directory(root, &mut files, &mut unreadable, &mut visited_directories);
        }
    }
    Walk {
        files: files.into_iter().collect(),
        unreadable_directories: unreadable.into_iter().collect(),
    }
}

fn walk_directory(
    directory: &Path,
    files: &mut BTreeSet<PathBuf>,
    unreadable: &mut BTreeSet<UnreadableDirectory>,
    visited: &mut BTreeSet<PathBuf>,
) {
    // The canonical form only guards against symbolic-link cycles;
    // reported paths keep the shape the walk reached them under. Failing
    // to resolve a directory we have just seen is itself a refusal, and it
    // is reported like any other: the alternative is the silent skip this
    // list exists to end.
    let canonical = match fs::canonicalize(directory) {
        Ok(canonical) => canonical,
        Err(reason) => {
            unreadable.insert(UnreadableDirectory {
                path: directory.to_path_buf(),
                reason: reason.to_string(),
            });
            return;
        }
    };
    if !visited.insert(canonical) {
        return;
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(reason) => {
            unreadable.insert(UnreadableDirectory {
                path: directory.to_path_buf(),
                reason: reason.to_string(),
            });
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_directory(&path, files, unreadable, visited);
        } else if has_php_extension(&path) && path.is_file() {
            files.insert(path);
        }
    }
}

fn has_php_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("php"))
}

impl Vfs {
    /// Loads a file's current disk bytes into the disk state. This is
    /// push-time work for the composition root, never called during a
    /// query.
    pub fn load_from_disk(&mut self, path: &Path) -> std::io::Result<FileId> {
        let bytes = fs::read(path)?;
        Ok(self.set_file_contents(path, Some(bytes)))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::fs;
    use std::path::{Path, PathBuf};

    use super::enumerate_php_files;
    use crate::Vfs;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn directories_are_walked_recursively_for_php_files_only() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/A.php"), "<?php");
        write(&root.join("src/nested/B.php"), "<?php");
        write(&root.join("src/notes.txt"), "not code");
        write(&root.join("outside/C.php"), "<?php");
        assert_eq!(
            enumerate_php_files(&[root.join("src")]).files,
            vec![root.join("src/A.php"), root.join("src/nested/B.php")],
        );
    }

    #[test]
    fn the_extension_match_ignores_ascii_case() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/Legacy.PHP"), "<?php");
        assert_eq!(
            enumerate_php_files(&[root.join("src")]).files,
            vec![root.join("src/Legacy.PHP")],
        );
    }

    #[test]
    fn an_explicit_file_root_is_included_regardless_of_extension() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("bootstrap.inc"), "<?php");
        assert_eq!(
            enumerate_php_files(&[root.join("bootstrap.inc")]).files,
            vec![root.join("bootstrap.inc")],
        );
    }

    #[test]
    fn results_are_sorted_and_deduplicated_across_overlapping_roots() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/B.php"), "<?php");
        write(&root.join("src/nested/A.php"), "<?php");
        let roots = vec![
            root.join("src"),
            root.join("src/nested"),
            root.join("src/B.php"),
        ];
        assert_eq!(
            enumerate_php_files(&roots).files,
            vec![root.join("src/B.php"), root.join("src/nested/A.php")],
        );
    }

    #[test]
    fn missing_roots_are_skipped_silently() {
        // A declared autoload directory that has not been created yet is
        // ordinary, and saying nothing is the right answer. That is what
        // separates it from a directory that exists and cannot be read.
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let walk = enumerate_php_files(&[root.join("does-not-exist")]);
        assert_eq!(walk.files, Vec::<PathBuf>::new());
        assert_eq!(walk.unreadable_directories, Vec::new());
    }

    /// Permissions are the only portable way to make a directory
    /// unreadable, and only Unix has them in the form this needs.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_directory_is_reported_and_the_rest_is_still_walked() {
        // The walk used to return silently on a `read_dir` error, so a
        // directory nobody may list vanished from the analysis without a
        // word. The run then went green over a project it had only half
        // read, which is the one failure a checker must never have.
        use std::os::unix::fs::PermissionsExt as _;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/A.php"), "<?php");
        write(&root.join("src/locked/B.php"), "<?php");
        let locked = root.join("src/locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        let walk = enumerate_php_files(&[root.join("src")]);

        // Restore before asserting: a failed assertion must not leave the
        // temporary directory undeletable.
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(
            walk.files,
            vec![root.join("src/A.php")],
            "everything the walk could reach is still enumerated",
        );
        let reported = walk.unreadable_directories.first().expect("it is reported");
        assert_eq!(reported.path, locked);
        assert_eq!(walk.unreadable_directories.len(), 1);
        assert!(!reported.reason.is_empty(), "the refusal says why");
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_root_is_reported_too() {
        use std::os::unix::fs::PermissionsExt as _;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/A.php"), "<?php");
        let locked = root.join("src");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        let walk = enumerate_php_files(std::slice::from_ref(&locked));

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(walk.files, Vec::<PathBuf>::new());
        let reported = walk.unreadable_directories.first().expect("it is reported");
        assert_eq!(reported.path, locked);
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_cycles_terminate() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/A.php"), "<?php");
        std::os::unix::fs::symlink(root.join("src"), root.join("src/loop")).unwrap();
        let walk = enumerate_php_files(&[root.join("src")]);
        assert_eq!(walk.files.first(), Some(&root.join("src/A.php")));
        assert_eq!(
            walk.unreadable_directories,
            Vec::new(),
            "a link that closes a cycle is not a directory we failed to read",
        );
    }

    #[test]
    fn load_from_disk_reads_the_bytes_into_the_disk_state() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/A.php"), "<?php echo 1;");
        let mut vfs = Vfs::default();
        let file = vfs.load_from_disk(&root.join("src/A.php")).unwrap();
        assert_eq!(vfs.contents(file), Some(b"<?php echo 1;".as_slice()));
        assert!(vfs.load_from_disk(&root.join("missing.php")).is_err());
    }
}
