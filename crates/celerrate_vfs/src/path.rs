use std::path::{Component, Path, PathBuf};

/// Joins `path` onto `base` when it is relative, then removes `.` and
/// resolves `..` lexically. No file-system access happens: symbolic
/// links are not resolved, so the result is a pure function of its
/// inputs. `base` must be absolute; the result is then absolute.
pub fn normalize_path(path: &Path, base: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Popping fails only at a filesystem root, where an
                // excess `..` stays at the root; a relative prefix
                // (impossible under an absolute base) would keep it.
                if !normalized.pop() && !joined.is_absolute() {
                    normalized.push(Component::ParentDir.as_os_str());
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::{Path, PathBuf};

    use super::normalize_path;

    #[test]
    fn a_relative_path_joins_onto_the_base() {
        assert_eq!(
            normalize_path(Path::new("src/App.php"), Path::new("/project")),
            PathBuf::from("/project/src/App.php"),
        );
    }

    #[test]
    fn an_absolute_path_ignores_the_base() {
        assert_eq!(
            normalize_path(Path::new("/elsewhere/lib"), Path::new("/project")),
            PathBuf::from("/elsewhere/lib"),
        );
    }

    #[test]
    fn current_directory_components_are_removed() {
        assert_eq!(
            normalize_path(Path::new("./src/./sub"), Path::new("/project")),
            PathBuf::from("/project/src/sub"),
        );
    }

    #[test]
    fn parent_components_resolve_lexically() {
        assert_eq!(
            normalize_path(
                Path::new("../acme/library"),
                Path::new("/project/vendor/composer")
            ),
            PathBuf::from("/project/vendor/acme/library"),
        );
    }

    #[test]
    fn excess_parents_on_an_absolute_path_stop_at_the_root() {
        assert_eq!(
            normalize_path(Path::new("../../../.."), Path::new("/project")),
            PathBuf::from("/"),
        );
    }

    #[test]
    fn an_already_normalized_path_is_unchanged() {
        assert_eq!(
            normalize_path(Path::new("/project/src"), Path::new("/project")),
            PathBuf::from("/project/src"),
        );
    }
}
