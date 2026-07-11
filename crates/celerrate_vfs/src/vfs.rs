use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use celerrate_source::FileId;

/// One file whose effective contents changed since the last
/// [`Vfs::take_changes`]. `contents` is the current effective state:
/// `None` means the file no longer exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub file_id: FileId,
    pub contents: Option<Vec<u8>>,
}

/// The in-memory file state: interned paths, disk contents, overlays.
///
/// The effective contents of a file are its overlay when one is set,
/// otherwise its disk state. A mutation is reported as a change only
/// when it alters the effective contents, so redundant writes never
/// reach the database.
#[derive(Debug, Default)]
pub struct Vfs {
    paths: Vec<PathBuf>,
    identifiers: HashMap<PathBuf, FileId>,
    disk: HashMap<FileId, Vec<u8>>,
    overlays: HashMap<FileId, Vec<u8>>,
    changed: BTreeSet<FileId>,
}

impl Vfs {
    /// Interns a path, assigning the next identifier on first sight.
    pub fn file_id(&mut self, path: &Path) -> FileId {
        if let Some(&existing) = self.identifiers.get(path) {
            return existing;
        }
        let assigned = FileId::new(self.paths.len() as u32);
        self.paths.push(path.to_path_buf());
        self.identifiers.insert(path.to_path_buf(), assigned);
        assigned
    }

    /// The path a file identifier was interned under.
    pub fn path(&self, file_id: FileId) -> Option<&Path> {
        self.paths
            .get(file_id.as_u32() as usize)
            .map(PathBuf::as_path)
    }

    /// Sets or deletes (`None`) the disk state of a file.
    pub fn set_file_contents(&mut self, path: &Path, contents: Option<Vec<u8>>) -> FileId {
        let file_id = self.file_id(path);
        let before = self.effective(file_id).map(<[u8]>::to_vec);
        match contents {
            Some(bytes) => {
                self.disk.insert(file_id, bytes);
            }
            None => {
                self.disk.remove(&file_id);
            }
        }
        self.record_if_changed(file_id, before);
        file_id
    }

    /// Sets an overlay shadowing the disk state of a file.
    pub fn set_overlay(&mut self, path: &Path, contents: Vec<u8>) -> FileId {
        let file_id = self.file_id(path);
        let before = self.effective(file_id).map(<[u8]>::to_vec);
        self.overlays.insert(file_id, contents);
        self.record_if_changed(file_id, before);
        file_id
    }

    /// Removes a file's overlay, revealing its disk state again.
    pub fn clear_overlay(&mut self, file_id: FileId) {
        let before = self.effective(file_id).map(<[u8]>::to_vec);
        self.overlays.remove(&file_id);
        self.record_if_changed(file_id, before);
    }

    /// The effective contents: the overlay when set, the disk state
    /// otherwise, `None` when the file does not exist.
    pub fn contents(&self, file_id: FileId) -> Option<&[u8]> {
        self.effective(file_id)
    }

    /// Drains the accumulated changes, sorted by file identifier, each
    /// carrying the file's current effective contents.
    pub fn take_changes(&mut self) -> Vec<ChangedFile> {
        let changed = core::mem::take(&mut self.changed);
        changed
            .into_iter()
            .map(|file_id| ChangedFile {
                file_id,
                contents: self.effective(file_id).map(<[u8]>::to_vec),
            })
            .collect()
    }

    fn effective(&self, file_id: FileId) -> Option<&[u8]> {
        self.overlays
            .get(&file_id)
            .or_else(|| self.disk.get(&file_id))
            .map(Vec::as_slice)
    }

    fn record_if_changed(&mut self, file_id: FileId, before: Option<Vec<u8>>) {
        if self.effective(file_id) != before.as_deref() {
            self.changed.insert(file_id);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::Path;

    use super::*;

    #[test]
    fn interning_is_stable() {
        let mut vfs = Vfs::default();
        let first = vfs.file_id(Path::new("/project/a.php"));
        let second = vfs.file_id(Path::new("/project/b.php"));
        assert_ne!(first, second);
        assert_eq!(vfs.file_id(Path::new("/project/a.php")), first);
        assert_eq!(vfs.path(first), Some(Path::new("/project/a.php")));
    }

    #[test]
    fn contents_round_trip_through_disk_state() {
        let mut vfs = Vfs::default();
        let file = vfs.set_file_contents(Path::new("/project/a.php"), Some(b"<?php".to_vec()));
        assert_eq!(vfs.contents(file), Some(b"<?php".as_slice()));
    }

    #[test]
    fn overlays_shadow_disk_state_and_clear_back() {
        let mut vfs = Vfs::default();
        let file = vfs.set_file_contents(Path::new("/project/a.php"), Some(b"disk".to_vec()));
        vfs.set_overlay(Path::new("/project/a.php"), b"overlay".to_vec());
        assert_eq!(vfs.contents(file), Some(b"overlay".as_slice()));
        vfs.clear_overlay(file);
        assert_eq!(vfs.contents(file), Some(b"disk".as_slice()));
    }

    #[test]
    fn changes_report_effective_contents_sorted_and_deduplicated() {
        let mut vfs = Vfs::default();
        // Interned first, so `file_b` receives the lower identifier.
        let file_b = vfs.set_file_contents(Path::new("/project/b.php"), Some(b"2".to_vec()));
        let file_a = vfs.set_file_contents(Path::new("/project/a.php"), Some(b"1".to_vec()));
        vfs.set_file_contents(Path::new("/project/a.php"), Some(b"3".to_vec()));
        assert!(file_b < file_a);
        let changes = vfs.take_changes();
        assert_eq!(
            changes,
            vec![
                ChangedFile {
                    file_id: file_b,
                    contents: Some(b"2".to_vec()),
                },
                ChangedFile {
                    file_id: file_a,
                    contents: Some(b"3".to_vec()),
                },
            ],
        );
        assert!(vfs.take_changes().is_empty());
    }

    #[test]
    fn unchanged_effective_contents_do_not_report_a_change() {
        let mut vfs = Vfs::default();
        let file = vfs.set_file_contents(Path::new("/project/a.php"), Some(b"same".to_vec()));
        vfs.take_changes();
        vfs.set_file_contents(Path::new("/project/a.php"), Some(b"same".to_vec()));
        vfs.set_overlay(Path::new("/project/a.php"), b"same".to_vec());
        assert!(vfs.take_changes().is_empty());
        let _ = file;
    }

    #[test]
    fn deleting_under_an_overlay_keeps_the_effective_contents() {
        let mut vfs = Vfs::default();
        let file = vfs.set_file_contents(Path::new("/project/a.php"), Some(b"disk".to_vec()));
        vfs.set_overlay(Path::new("/project/a.php"), b"overlay".to_vec());
        vfs.take_changes();
        vfs.set_file_contents(Path::new("/project/a.php"), None);
        assert!(vfs.take_changes().is_empty());
        vfs.clear_overlay(file);
        let changes = vfs.take_changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes.first().unwrap().contents, None);
    }
}
