//! Declared types: lowering written native type text into the lattice
//! at a declaring site. The keyword table is total over the native
//! grammar; unknown names lower to class types (the judgment layer
//! answers `CannotProve` for unresolvable classes). Bare `callable`
//! lowers to `mixed`: a documented sound widening (no top-of-callables
//! form exists in the lattice; recorded debt, revisited by plan 8).

use celerrate_semantics::{SymbolSpace, UseTables, resolve_candidates};

use crate::representation::TypeId;
use crate::written::{WrittenType, parse_written};

/// Where a written name qualifies: a source declaring site (namespace
/// plus `use` tables) or the global context (stub type texts).
#[allow(dead_code)]
pub(crate) enum NameSite<'a> {
    Source {
        namespace: &'a str,
        tables: &'a UseTables,
    },
    Global,
}

#[allow(dead_code)]
pub(crate) fn lower_written_text<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    text: &str,
) -> Option<TypeId<'db>> {
    Some(lower_written(db, site, &parse_written(text)?))
}

#[allow(dead_code)]
pub(crate) fn lower_written<'db>(
    db: &'db dyn salsa::Database,
    site: &NameSite<'_>,
    written: &WrittenType,
) -> TypeId<'db> {
    match written {
        WrittenType::Nullable(inner) => {
            TypeId::union(db, [lower_written(db, site, inner), TypeId::null(db)])
        }
        WrittenType::Union(parts) => {
            TypeId::union(db, parts.iter().map(|part| lower_written(db, site, part)))
        }
        WrittenType::Intersection(parts) => {
            TypeId::intersection(db, parts.iter().map(|part| lower_written(db, site, part)))
        }
        WrittenType::Name(name) => lower_name(db, site, name),
    }
}

#[allow(clippy::collapsible_if)]
fn lower_name<'db>(db: &'db dyn salsa::Database, site: &NameSite<'_>, name: &str) -> TypeId<'db> {
    if !name.contains('\\') {
        if let Some(keyword) = lower_keyword(db, name) {
            return keyword;
        }
    }
    TypeId::class(db, &qualified_class_name(site, name), vec![])
}

/// The keyword table: total over the native grammar (decision 3 for
/// `callable`). `None` means "an ordinary class name".
fn lower_keyword<'db>(db: &'db dyn salsa::Database, name: &str) -> Option<TypeId<'db>> {
    let folded = name.to_ascii_lowercase();
    Some(match folded.as_str() {
        "int" => TypeId::int(db),
        "float" => TypeId::float(db),
        "string" => TypeId::string(db),
        "bool" => TypeId::bool(db),
        "true" => TypeId::bool_literal(db, true),
        "false" => TypeId::bool_literal(db, false),
        "null" => TypeId::null(db),
        "mixed" => TypeId::mixed(db),
        "never" => TypeId::never(db),
        "void" => TypeId::void(db),
        "object" => TypeId::object(db),
        "resource" => TypeId::resource(db),
        "array" => TypeId::array(
            db,
            TypeId::union(db, [TypeId::int(db), TypeId::string(db)]),
            TypeId::mixed(db),
        ),
        "iterable" => TypeId::iterable(db, TypeId::mixed(db), TypeId::mixed(db)),
        // Decision 3: no top-of-callables form exists; `mixed` is the
        // documented sound widening (recorded debt for plan 8).
        "callable" => TypeId::mixed(db),
        "self" => TypeId::self_placeholder(db),
        "static" => TypeId::static_placeholder(db),
        "parent" => TypeId::parent_placeholder(db),
        _ => return None,
    })
}

/// PHP class-name resolution is static: the first candidate is the
/// fully qualified name whether or not the class exists.
fn qualified_class_name(site: &NameSite<'_>, written: &str) -> String {
    match site {
        NameSite::Source { namespace, tables } => {
            resolve_candidates(written, SymbolSpace::ClassLike, namespace, tables)
                .into_iter()
                .next()
                .unwrap_or_else(|| written.trim_start_matches('\\').to_owned())
        }
        NameSite::Global => written.trim_start_matches('\\').to_owned(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use super::{NameSite, lower_written_text};
    use crate::representation::TypeId;

    fn lower<'db>(db: &'db TestDatabase, text: &str) -> Option<TypeId<'db>> {
        lower_written_text(db, &NameSite::Global, text)
    }

    #[test]
    fn the_keyword_table_is_total_over_the_native_grammar() {
        let db = TestDatabase::default();
        let cases: &[(&str, TypeId<'_>)] = &[
            ("int", TypeId::int(&db)),
            ("INT", TypeId::int(&db)),
            ("float", TypeId::float(&db)),
            ("string", TypeId::string(&db)),
            ("bool", TypeId::bool(&db)),
            ("true", TypeId::bool_literal(&db, true)),
            ("false", TypeId::bool_literal(&db, false)),
            ("null", TypeId::null(&db)),
            ("mixed", TypeId::mixed(&db)),
            ("never", TypeId::never(&db)),
            ("void", TypeId::void(&db)),
            ("object", TypeId::object(&db)),
            ("resource", TypeId::resource(&db)),
            ("self", TypeId::self_placeholder(&db)),
            ("static", TypeId::static_placeholder(&db)),
            ("parent", TypeId::parent_placeholder(&db)),
        ];
        for (text, expected) in cases {
            assert_eq!(lower(&db, text), Some(*expected), "keyword {text}");
        }
    }

    #[test]
    fn array_iterable_and_callable_lower_to_their_documented_forms() {
        let db = TestDatabase::default();
        let array_key = TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]);
        assert_eq!(
            lower(&db, "array"),
            Some(TypeId::array(&db, array_key, TypeId::mixed(&db))),
        );
        assert_eq!(
            lower(&db, "iterable"),
            Some(TypeId::iterable(
                &db,
                TypeId::mixed(&db),
                TypeId::mixed(&db)
            )),
        );
        // Decision 3: bare `callable` is a documented sound widening.
        assert_eq!(lower(&db, "callable"), Some(TypeId::mixed(&db)));
    }

    #[test]
    fn nullable_union_and_intersection_lower_through_the_lattice() {
        let db = TestDatabase::default();
        assert_eq!(
            lower(&db, "?int"),
            Some(TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)])),
        );
        assert_eq!(
            lower(&db, "int|string"),
            Some(TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)])),
        );
        let countable = TypeId::class(&db, "Countable", vec![]);
        let iterator = TypeId::class(&db, "Iterator", vec![]);
        assert_eq!(
            lower(&db, "Countable&Iterator"),
            Some(TypeId::intersection(&db, [countable, iterator])),
        );
    }

    #[test]
    fn global_names_lower_to_class_types_with_the_backslash_trimmed() {
        let db = TestDatabase::default();
        assert_eq!(
            lower(&db, "\\DateTime"),
            Some(TypeId::class(&db, "DateTime", vec![])),
        );
        // A qualified name is never a keyword.
        assert_eq!(
            lower(&db, "Foo\\int"),
            Some(TypeId::class(&db, "Foo\\int", vec![])),
        );
    }

    #[test]
    fn source_site_names_qualify_through_namespace_and_imports() {
        use celerrate_db::{AnalyzedFileSet, SourceFile};
        use celerrate_semantics::{UseTables, item_tree};
        use celerrate_source::FileId;

        let db = TestDatabase::default();
        let file = SourceFile::new(
            &db,
            FileId::new(0),
            b"<?php namespace App; use Psr\\Log\\LoggerInterface as Logger; class C {}".to_vec(),
        );
        let _files = AnalyzedFileSet::new(&db, vec![file]);
        let tree = item_tree(&db, file);
        let tables = UseTables::for_namespace(tree, "App");
        let site = super::NameSite::Source {
            namespace: "App",
            tables: &tables,
        };
        // The import expands.
        assert_eq!(
            lower_written_text(&db, &site, "Logger"),
            Some(TypeId::class(&db, "Psr\\Log\\LoggerInterface", vec![])),
        );
        // An unimported name qualifies into the namespace, existing or not.
        assert_eq!(
            lower_written_text(&db, &site, "Repository"),
            Some(TypeId::class(&db, "App\\Repository", vec![])),
        );
        // Absolute names ignore the namespace.
        assert_eq!(
            lower_written_text(&db, &site, "\\Throwable"),
            Some(TypeId::class(&db, "Throwable", vec![])),
        );
    }

    #[test]
    fn malformed_text_lowers_to_none() {
        let db = TestDatabase::default();
        assert_eq!(lower(&db, ""), None);
        assert_eq!(lower(&db, "A|"), None);
    }
}
