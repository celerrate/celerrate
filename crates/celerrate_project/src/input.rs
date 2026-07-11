//! The analysis configuration as a salsa input: the slice of project
//! discovery that queries are allowed to read. Everything else in
//! `ProjectDiscovery` (walk roots, notices) is push-time state for the
//! composition root, not query-visible input.

use crate::version::PhpVersionRange;

/// Created at the composition root from a [`ProjectDiscovery`], with
/// `salsa::Durability::MEDIUM`: configuration changes are rarer than
/// file edits and more frequent than stub bumps.
///
/// [`ProjectDiscovery`]: crate::ProjectDiscovery
#[salsa::input]
pub struct ProjectConfiguration {
    pub php_version_range: PhpVersionRange,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use salsa::Setter;

    use super::ProjectConfiguration;
    use crate::version::{PhpVersion, PhpVersionRange};

    #[test]
    fn the_configuration_stores_and_updates_the_version_range() {
        let mut db = TestDatabase::default();
        let range = PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5));
        let configuration = ProjectConfiguration::builder(range)
            .durability(salsa::Durability::MEDIUM)
            .new(&db);
        assert_eq!(configuration.php_version_range(&db), range);

        let narrowed = PhpVersionRange::point(PhpVersion::new(8, 2));
        configuration.set_php_version_range(&mut db).to(narrowed);
        assert_eq!(configuration.php_version_range(&db), narrowed);
    }
}
