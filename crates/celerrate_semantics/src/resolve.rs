//! Top-level PHP name resolution: from a written reference to the
//! candidate fully qualified names, in the order PHP tries them. The
//! rules are the real ones: a leading backslash is absolute, a
//! `namespace\` prefix is relative, a qualified name resolves its
//! first segment through the class imports and never falls back to the
//! global namespace, an unqualified name resolves through its own
//! space's imports, then the current namespace, with a global fallback
//! for functions and constants only.

use std::collections::HashMap;

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::StubIndexInput;

use crate::items::{ImportKind, ItemTree};
use crate::lookup::{SymbolQuery, SymbolResolution, lookup_symbol};
use crate::symbols::{SymbolSpace, folded_symbol_key, fully_qualified_name};

/// The import tables of one namespace within one file: alias to
/// written absolute target, one map per symbol space. Class and
/// function aliases match case-insensitively (the map keys are
/// folded); constant aliases match case-sensitively (verbatim keys). A
/// duplicate alias keeps the last import: PHP rejects the redefinition
/// outright, tolerance picks a deterministic winner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UseTables {
    classes: HashMap<String, String>,
    functions: HashMap<String, String>,
    constants: HashMap<String, String>,
}

impl UseTables {
    /// The tables of `namespace`: every import the item tree carries
    /// for it. Imports apply to their whole namespace block; position
    /// within the block does not matter.
    pub fn for_namespace(tree: &ItemTree, namespace: &str) -> Self {
        let mut tables = Self::default();
        for import in tree
            .imports
            .iter()
            .filter(|import| import.namespace == namespace)
        {
            match import.kind {
                ImportKind::Class => {
                    tables
                        .classes
                        .insert(import.alias.to_ascii_lowercase(), import.target.clone());
                }
                ImportKind::Function => {
                    tables
                        .functions
                        .insert(import.alias.to_ascii_lowercase(), import.target.clone());
                }
                ImportKind::Constant => {
                    tables
                        .constants
                        .insert(import.alias.clone(), import.target.clone());
                }
            }
        }
        tables
    }

    /// The target a class-space alias names, case-insensitively. The
    /// class table also resolves the first segment of qualified names
    /// of every space: `use` imports name classes or namespaces.
    fn class_target(&self, alias: &str) -> Option<&str> {
        self.classes
            .get(&alias.to_ascii_lowercase())
            .map(String::as_str)
    }

    fn function_target(&self, alias: &str) -> Option<&str> {
        self.functions
            .get(&alias.to_ascii_lowercase())
            .map(String::as_str)
    }

    fn constant_target(&self, alias: &str) -> Option<&str> {
        self.constants.get(alias).map(String::as_str)
    }
}

/// The candidate fully qualified names of one written reference, in
/// the order PHP tries them. Empty input (error-recovery wreckage)
/// produces no candidates.
pub fn resolve_candidates(
    written: &str,
    space: SymbolSpace,
    namespace: &str,
    tables: &UseTables,
) -> Vec<String> {
    if written.is_empty() {
        return Vec::new();
    }
    if let Some(absolute) = written.strip_prefix('\\') {
        return if absolute.is_empty() {
            Vec::new()
        } else {
            vec![absolute.to_owned()]
        };
    }
    match written.split_once('\\') {
        Some((first, rest)) => {
            if first.eq_ignore_ascii_case("namespace") {
                return vec![fully_qualified_name(namespace, rest)];
            }
            match tables.class_target(first) {
                Some(target) => vec![format!("{target}\\{rest}")],
                None => vec![fully_qualified_name(namespace, written)],
            }
        }
        None => {
            let imported = match space {
                SymbolSpace::ClassLike => tables.class_target(written),
                SymbolSpace::Function => tables.function_target(written),
                SymbolSpace::Constant => tables.constant_target(written),
            };
            if let Some(target) = imported {
                return vec![target.to_owned()];
            }
            let in_namespace = fully_qualified_name(namespace, written);
            match space {
                SymbolSpace::ClassLike => vec![in_namespace],
                SymbolSpace::Function | SymbolSpace::Constant => {
                    if namespace.is_empty() {
                        vec![in_namespace]
                    } else {
                        vec![in_namespace, written.to_owned()]
                    }
                }
            }
        }
    }
}

/// The three inputs the global symbol index reads, bundled so
/// consumers thread one handle set through resolution calls. A plain
/// value, not a salsa struct: tracked functions take the three handles
/// separately.
#[derive(Clone, Copy)]
pub struct SymbolSources {
    pub files: AnalyzedFileSet,
    pub stubs: StubIndexInput,
    pub configuration: ProjectConfiguration,
}

// The three fields are salsa input handles; none of them implement
// `Debug` (matching `celerrate_db::SourceFile`'s own precedent), so
// this impl is written by hand rather than derived.
impl std::fmt::Debug for SymbolSources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SymbolSources").finish()
    }
}

/// Resolves one written reference: candidate names in PHP's order,
/// each looked up through the per-name firewall; the first hit wins.
pub fn resolve_name(
    db: &dyn salsa::Database,
    sources: SymbolSources,
    namespace: &str,
    tables: &UseTables,
    written: &str,
    space: SymbolSpace,
) -> Option<SymbolResolution> {
    resolve_candidates(written, space, namespace, tables)
        .into_iter()
        .find_map(|candidate| {
            let query = SymbolQuery::new(db, space, folded_symbol_key(space, &candidate));
            lookup_symbol(
                db,
                sources.files,
                sources.stubs,
                sources.configuration,
                query,
            )
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_source::FileId;

    use super::{UseTables, resolve_candidates};
    use crate::items::ItemTree;
    use crate::symbols::SymbolSpace;

    fn candidates(source: &str, namespace: &str, written: &str, space: SymbolSpace) -> Vec<String> {
        let tree = ItemTree::from_root(FileId::new(0), &celerrate_syntax::parse(source).tree());
        let tables = UseTables::for_namespace(&tree, namespace);
        resolve_candidates(written, space, namespace, &tables)
    }

    #[test]
    fn a_fully_qualified_name_is_its_own_only_candidate() {
        assert_eq!(
            candidates(
                "<?php namespace App;",
                "App",
                "\\Core\\Base",
                SymbolSpace::ClassLike
            ),
            vec!["Core\\Base".to_owned()],
        );
        assert_eq!(
            candidates(
                "<?php namespace App;",
                "App",
                "\\strlen",
                SymbolSpace::Function
            ),
            vec!["strlen".to_owned()],
        );
    }

    #[test]
    fn the_namespace_keyword_prefix_is_relative_to_the_current_namespace() {
        assert_eq!(
            candidates(
                "<?php namespace App;",
                "App",
                "namespace\\Child\\Node",
                SymbolSpace::ClassLike
            ),
            vec!["App\\Child\\Node".to_owned()],
        );
        // Case-insensitive keyword, global namespace collapses it.
        assert_eq!(
            candidates("<?php", "", "NAMESPACE\\Node", SymbolSpace::ClassLike),
            vec!["Node".to_owned()],
        );
    }

    #[test]
    fn a_qualified_name_prefixes_the_current_namespace_and_never_falls_back() {
        assert_eq!(
            candidates(
                "<?php namespace App;",
                "App",
                "Sub\\Helper",
                SymbolSpace::ClassLike
            ),
            vec!["App\\Sub\\Helper".to_owned()],
        );
        // Qualified function names do not fall back to the global
        // namespace either: one candidate only.
        assert_eq!(
            candidates(
                "<?php namespace App;",
                "App",
                "Sub\\greet",
                SymbolSpace::Function
            ),
            vec!["App\\Sub\\greet".to_owned()],
        );
    }

    #[test]
    fn a_qualified_first_segment_resolves_through_the_class_imports() {
        let source = "<?php namespace App; use Lib\\Collections as Col;";
        assert_eq!(
            candidates(source, "App", "Col\\ArrayList", SymbolSpace::ClassLike),
            vec!["Lib\\Collections\\ArrayList".to_owned()],
        );
        // Alias matching is case-insensitive.
        assert_eq!(
            candidates(source, "App", "col\\ArrayList", SymbolSpace::ClassLike),
            vec!["Lib\\Collections\\ArrayList".to_owned()],
        );
        // The class table serves qualified names of every space.
        assert_eq!(
            candidates(source, "App", "Col\\format", SymbolSpace::Function),
            vec!["Lib\\Collections\\format".to_owned()],
        );
    }

    #[test]
    fn an_unqualified_class_import_wins_outright() {
        let source = "<?php namespace App; use Lib\\Helper;";
        assert_eq!(
            candidates(source, "App", "Helper", SymbolSpace::ClassLike),
            vec!["Lib\\Helper".to_owned()],
        );
        assert_eq!(
            candidates(source, "App", "HELPER", SymbolSpace::ClassLike),
            vec!["Lib\\Helper".to_owned()],
        );
    }

    #[test]
    fn an_unqualified_class_has_no_global_fallback() {
        assert_eq!(
            candidates(
                "<?php namespace App;",
                "App",
                "Helper",
                SymbolSpace::ClassLike
            ),
            vec!["App\\Helper".to_owned()],
        );
    }

    #[test]
    fn unqualified_functions_and_constants_fall_back_to_the_global_namespace() {
        assert_eq!(
            candidates(
                "<?php namespace App;",
                "App",
                "greet",
                SymbolSpace::Function
            ),
            vec!["App\\greet".to_owned(), "greet".to_owned()],
        );
        assert_eq!(
            candidates(
                "<?php namespace App;",
                "App",
                "LIMIT",
                SymbolSpace::Constant
            ),
            vec!["App\\LIMIT".to_owned(), "LIMIT".to_owned()],
        );
    }

    #[test]
    fn in_the_global_namespace_the_fallback_collapses_to_one_candidate() {
        assert_eq!(
            candidates("<?php", "", "greet", SymbolSpace::Function),
            vec!["greet".to_owned()],
        );
    }

    #[test]
    fn function_imports_match_case_insensitively() {
        let source = "<?php namespace App; use function Lib\\greet as hello;";
        assert_eq!(
            candidates(source, "App", "HELLO", SymbolSpace::Function),
            vec!["Lib\\greet".to_owned()],
        );
    }

    #[test]
    fn constant_imports_match_case_sensitively() {
        let source = "<?php namespace App; use const Lib\\LIMIT as L;";
        assert_eq!(
            candidates(source, "App", "L", SymbolSpace::Constant),
            vec!["Lib\\LIMIT".to_owned()],
        );
        // The lowercase spelling misses the import and takes the
        // normal fallback path.
        assert_eq!(
            candidates(source, "App", "l", SymbolSpace::Constant),
            vec!["App\\l".to_owned(), "l".to_owned()],
        );
    }

    #[test]
    fn imports_of_another_namespace_do_not_apply() {
        let source = "<?php namespace First { use Lib\\Helper; } namespace Second {}";
        assert_eq!(
            candidates(source, "Second", "Helper", SymbolSpace::ClassLike),
            vec!["Second\\Helper".to_owned()],
        );
    }

    #[test]
    fn imports_of_every_space_stay_separate() {
        // A class import never answers a function reference.
        let source = "<?php namespace App; use Lib\\Helper;";
        assert_eq!(
            candidates(source, "App", "Helper", SymbolSpace::Function),
            vec!["App\\Helper".to_owned(), "Helper".to_owned()],
        );
    }

    #[test]
    fn wreckage_produces_no_candidates() {
        assert_eq!(
            candidates("<?php", "App", "", SymbolSpace::ClassLike),
            Vec::<String>::new(),
        );
        assert_eq!(
            candidates("<?php", "App", "\\", SymbolSpace::ClassLike),
            Vec::<String>::new(),
        );
    }

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    use super::{SymbolSources, resolve_name};
    use crate::items::DeclarationKind;
    use crate::lookup::SymbolResolution;
    use crate::queries::item_tree;

    fn sources_of(db: &TestDatabase, sources: &[&str]) -> SymbolSources {
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        SymbolSources {
            files: AnalyzedFileSet::new(db, handles),
            stubs: StubIndexInput::builder(StubIndex::from_symbols(vec![StubSymbol {
                name: "strlen".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            }]))
            .durability(salsa::Durability::HIGH)
            .new(db),
            configuration: ProjectConfiguration::builder(PhpVersionRange::new(
                PhpVersion::new(8, 1),
                PhpVersion::new(8, 5),
            ))
            .durability(salsa::Durability::MEDIUM)
            .new(db),
        }
    }

    #[test]
    fn a_reference_resolves_through_its_import_to_the_declaration() {
        let db = TestDatabase::default();
        let sources = sources_of(
            &db,
            &[
                "<?php namespace App; use Lib\\Helper;",
                "<?php namespace Lib; class Helper {}",
            ],
        );
        let file = *sources.files.files(&db).first().unwrap();
        let tables = UseTables::for_namespace(item_tree(&db, file), "App");
        assert_eq!(
            resolve_name(
                &db,
                sources,
                "App",
                &tables,
                "Helper",
                SymbolSpace::ClassLike
            ),
            Some(SymbolResolution::Source {
                kind: DeclarationKind::Class,
            }),
        );
    }

    #[test]
    fn an_unqualified_function_falls_back_to_the_global_stub() {
        let db = TestDatabase::default();
        let sources = sources_of(&db, &["<?php namespace App;"]);
        assert_eq!(
            resolve_name(
                &db,
                sources,
                "App",
                &UseTables::default(),
                "strlen",
                SymbolSpace::Function,
            ),
            Some(SymbolResolution::Stub {
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            }),
        );
    }

    #[test]
    fn an_unresolvable_reference_answers_none() {
        let db = TestDatabase::default();
        let sources = sources_of(&db, &["<?php namespace App;"]);
        assert_eq!(
            resolve_name(
                &db,
                sources,
                "App",
                &UseTables::default(),
                "Missing",
                SymbolSpace::ClassLike,
            ),
            None,
        );
    }
}
