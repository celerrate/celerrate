use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use celerrate_source::FileId;

use crate::Vfs;

/// Enumerates the PHP files under a set of walk roots, sorted and
/// deduplicated. A root that is a file is included as-is (autoload
/// `files` and `classmap` entries may name single files of any
/// extension); a root that is a directory is walked recursively for
/// `.php` files (ASCII-case-insensitive). Symbolic links to
/// directories are followed with cycle protection; missing roots and
/// unreadable entries are skipped: enumeration never fails.
pub fn enumerate_php_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = BTreeSet::new();
    let mut visited_directories = BTreeSet::new();
    for root in roots {
        if root.is_file() {
            files.insert(root.clone());
        } else if root.is_dir() {
            walk_directory(root, &mut files, &mut visited_directories);
        }
    }
    files.into_iter().collect()
}

fn walk_directory(
    directory: &Path,
    files: &mut BTreeSet<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
) {
    // The canonical form only guards against symbolic-link cycles;
    // reported paths keep the shape the walk reached them under.
    let Ok(canonical) = fs::canonicalize(directory) else {
        return;
    };
    if !visited.insert(canonical) {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_directory(&path, files, visited);
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
    #![allow(clippy::unwrap_used)]

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
            enumerate_php_files(&[root.join("src")]),
            vec![root.join("src/A.php"), root.join("src/nested/B.php")],
        );
    }

    #[test]
    fn the_extension_match_ignores_ascii_case() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/Legacy.PHP"), "<?php");
        assert_eq!(
            enumerate_php_files(&[root.join("src")]),
            vec![root.join("src/Legacy.PHP")],
        );
    }

    #[test]
    fn an_explicit_file_root_is_included_regardless_of_extension() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("bootstrap.inc"), "<?php");
        assert_eq!(
            enumerate_php_files(&[root.join("bootstrap.inc")]),
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
            enumerate_php_files(&roots),
            vec![root.join("src/B.php"), root.join("src/nested/A.php")],
        );
    }

    #[test]
    fn missing_roots_are_skipped_silently() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        assert_eq!(
            enumerate_php_files(&[root.join("does-not-exist")]),
            Vec::<PathBuf>::new(),
        );
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_cycles_terminate() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(&root.join("src/A.php"), "<?php");
        std::os::unix::fs::symlink(root.join("src"), root.join("src/loop")).unwrap();
        let files = enumerate_php_files(&[root.join("src")]);
        assert_eq!(files.first(), Some(&root.join("src/A.php")));
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
