//! Generic-argument threading (design sections 2 and 6): class-level
//! annotations parsed at their declaring site, composed transitively
//! along linearization's ancestry into per-ancestor argument lists.
//! This is the delivery path of the Doctrine-on-Symfony repository
//! pattern: `@extends ServiceEntityRepository<User>` reaches
//! `$repository->find($id)` through these queries.
//!
//! **Recorded debt.** Stub ancestors carry no generic arguments:
//! [`ancestor_arguments`] skips any ancestry edge whose `resolved` key
//! is absent (a stub target), so a class extending a stub generic
//! ancestor (`ArrayIterator`, `ArrayObject`, and the rest of the
//! SPL/stdlib generics) never threads that ancestor's arguments
//! through here — a receiver reaching such an ancestor degrades to the
//! protocol-member fallback (`flow.rs`'s iteration-typing chain,
//! decision 12) instead of a threaded generic. Curating the
//! phpstorm-stubs surface to carry its own `@template`/`@extends`
//! annotations is plan 7's debt, not this plan's.

use std::collections::BTreeMap;

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{ClassQuery, linearized_class};
use celerrate_stubs::StubIndexInput;

use crate::representation::TypeId;
use crate::substitution::{Substitution, substitute};
use crate::type_syntax::{ParsedAncestor, ParsedTemplate};

/// One class-like's own docblock, parsed: its ordered `@template`
/// declarations and its inheritance-position fixed arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq, salsa::Update)]
pub struct ClassAnnotations<'db> {
    pub templates: Vec<ParsedTemplate<'db>>,
    pub ancestors: Vec<ParsedAncestor<'db>>,
}

/// Parses `class`'s own docblock at its declaring site. No docblock,
/// no registered syntax, or an unresolvable class all answer the
/// default (no templates, no ancestors) — never an error. Runs even
/// for classes with only a `@template` docblock and no ancestors: the
/// solver (task 8) and the receiver substitution (task 5) read its
/// template list. `stubs` and `configuration` complete the input
/// quartet the annotation seam shares with `class_annotations`'s
/// siblings (`member_annotations`, `function_annotations`) — a
/// uniform query shape callers never have to special-case; a class's
/// own docblock resolves without consulting either.
#[salsa::tracked(returns(ref))]
pub fn class_annotations<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    _stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    class: ClassQuery<'db>,
) -> ClassAnnotations<'db> {
    let key = class.key(db);
    let Some(docblock) = crate::declared::owner_class_docblock(db, files, key) else {
        return ClassAnnotations::default();
    };
    crate::declared::with_declaring_site(db, files, key, |site| {
        let context = crate::type_syntax::AnnotationContext {
            declaring_scope: key,
            enclosing_class_scope: None,
            enclosing_class_docblock: None,
        };
        let parsed = crate::type_syntax::annotations_for_docblock(db, site, &context, &docblock);
        ClassAnnotations {
            templates: parsed.templates,
            ancestors: parsed.ancestors,
        }
    })
}

/// Zips `arguments` positionally against `templates` (declaration
/// order), binding each into a `Substitution` scoped to `scope`: a
/// missing argument falls to the template's own bound, then `mixed`
/// (decision 7's conservative-silence rule). The single home for that
/// two-step fallback — both `ancestor_arguments` (composing a target's
/// substitution while walking the ancestry) and `ancestor_substitution`
/// (re-deriving the same map for a single owner, on demand) zip through
/// here, so a future change to the fallback rule cannot drift between
/// two copies. Returns the substitution alongside the fixed argument
/// list in declaration order, since `ancestor_arguments` needs both.
fn zip_templates<'db>(
    db: &'db dyn salsa::Database,
    scope: &str,
    templates: &[ParsedTemplate<'db>],
    arguments: &[TypeId<'db>],
) -> (Substitution<'db>, Vec<TypeId<'db>>) {
    let mut substitution = Substitution::default();
    let mut fixed = Vec::with_capacity(templates.len());
    for (position, template) in templates.iter().enumerate() {
        let argument = arguments
            .get(position)
            .copied()
            .unwrap_or_else(|| template.bound.unwrap_or_else(|| TypeId::mixed(db)));
        substitution.bind(scope, &template.name, argument);
        fixed.push(argument);
    }
    (substitution, fixed)
}

/// The fixed generic arguments of every ancestor of `class`, composed
/// transitively along linearization's ancestry, in walk order.
/// Diamond inheritance resolves first-edge-wins; a stub or otherwise
/// unresolved ancestor contributes nothing; a missing argument falls
/// to the template's bound, then `mixed` (decision 7).
#[salsa::tracked(returns(ref))]
pub fn ancestor_arguments<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    class: ClassQuery<'db>,
) -> Vec<(String, Vec<TypeId<'db>>)> {
    let Some(linearized) = linearized_class(db, files, stubs, configuration, class) else {
        return Vec::new();
    };
    let class_key = class.key(db).clone();
    let mut substitutions: BTreeMap<String, Substitution<'db>> = BTreeMap::new();
    substitutions.insert(class_key, Substitution::default());
    let mut answers = Vec::new();
    for edge in &linearized.ancestry {
        let Some(target) = edge.resolved.clone() else {
            continue;
        };
        if substitutions.contains_key(&target) {
            // Diamond inheritance: the first edge in walk order wins.
            continue;
        }
        let Some(owner_substitution) = substitutions.get(&edge.owner).cloned() else {
            continue;
        };
        let owner_query = ClassQuery::new(db, edge.owner.clone());
        let written_arguments: Vec<TypeId<'db>> =
            class_annotations(db, files, stubs, configuration, owner_query)
                .ancestors
                .iter()
                .find(|ancestor| ancestor.class_name == target)
                .map(|ancestor| ancestor.arguments.clone())
                .unwrap_or_default();
        let substituted: Vec<TypeId<'db>> = written_arguments
            .iter()
            .map(|argument| {
                substitute(
                    db,
                    files,
                    stubs,
                    configuration,
                    *argument,
                    &owner_substitution,
                    None,
                )
            })
            .collect();
        let target_query = ClassQuery::new(db, target.clone());
        let templates = &class_annotations(db, files, stubs, configuration, target_query).templates;
        let (composed, fixed) = zip_templates(db, &target, templates, &substituted);
        substitutions.insert(target.clone(), composed);
        if !fixed.is_empty() {
            answers.push((target, fixed));
        }
    }
    answers
}

/// The ready-to-apply substitution for a member declared on
/// `owner_key`, consulted through `class_key`. `None` when the member
/// is the class's own or nothing is threaded — callers skip the walk.
///
/// Decision 8's consumer: `declared_member_signature` applies this to
/// substitute an inherited member's owner-scoped templates with the
/// receiver class's fixed arguments.
pub(crate) fn ancestor_substitution<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    class_key: &str,
    owner_key: &str,
) -> Option<Substitution<'db>> {
    if class_key == owner_key {
        return None;
    }
    let class = ClassQuery::new(db, class_key.to_owned());
    // `ancestor_arguments` already zips every template of `owner_key`
    // against its written arguments (falling back to bound-then-`mixed`
    // as needed) before it ever records an entry, so `arguments` here
    // always carries exactly `templates.len()` entries: the zip below
    // can never take its fallback arm, but goes through the shared
    // helper anyway so the rule has one home.
    let arguments = ancestor_arguments(db, files, stubs, configuration, class)
        .iter()
        .find(|(ancestor, _)| ancestor == owner_key)
        .map(|(_, arguments)| arguments.clone())?;
    let owner = ClassQuery::new(db, owner_key.to_owned());
    let templates = &class_annotations(db, files, stubs, configuration, owner).templates;
    let (map, _fixed) = zip_templates(db, owner_key, templates, &arguments);
    if map.is_empty() { None } else { Some(map) }
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::ClassQuery;
    use celerrate_source::FileId;
    use celerrate_stubs::StubIndexInput;

    use super::test_support::register_fake_syntax;
    use super::{ancestor_arguments, ancestor_substitution};
    use crate::representation::TypeId;

    struct Fixture {
        db: TestDatabase,
        files: AnalyzedFileSet,
        stubs: StubIndexInput,
        configuration: ProjectConfiguration,
    }

    fn fixture(sources: &[&str]) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs = StubIndexInput::builder(crate::inheritance::test_support::minimal_stub_index())
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        // Register the fake exactly the way inference.rs's
        // `register_fake_assertions` registers its fake: same
        // registration struct, same HIGH durability, implementation
        // `Arc::new(FakeSyntax)`.
        register_fake_syntax(&db);
        Fixture {
            db,
            files,
            stubs,
            configuration,
        }
    }

    fn arguments_of<'db>(f: &'db Fixture, class_key: &str) -> &'db Vec<(String, Vec<TypeId<'db>>)> {
        let class = ClassQuery::new(&f.db, class_key.to_owned());
        ancestor_arguments(&f.db, f.files, f.stubs, f.configuration, class)
    }

    #[test]
    fn a_direct_extends_threads_its_fixed_arguments() {
        let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Base {}
/** @extends Base<User> */
class Child extends Base {}
class User {}
"#]);
        let user = TypeId::class(&f.db, "app\\user", vec![]);
        assert_eq!(
            arguments_of(&f, "app\\child"),
            &vec![("app\\base".to_owned(), vec![user])],
        );
    }

    #[test]
    fn arguments_compose_transitively_through_the_chain() {
        let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Grand {}
/**
 * @template U
 * @extends Grand<U>
 */
class Middle extends Grand {}
/** @extends Middle<User> */
class Leaf extends Middle {}
class User {}
"#]);
        let user = TypeId::class(&f.db, "app\\user", vec![]);
        assert_eq!(
            arguments_of(&f, "app\\leaf"),
            &vec![
                ("app\\middle".to_owned(), vec![user]),
                ("app\\grand".to_owned(), vec![user]),
            ],
        );
    }

    #[test]
    fn a_missing_argument_falls_to_mixed_when_the_template_is_boundless() {
        let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Base {}
class Child extends Base {}
"#]);
        // No `@extends` tag at all: the template zips against nothing
        // and, having no bound of its own, falls straight to `mixed`.
        assert_eq!(
            arguments_of(&f, "app\\child"),
            &vec![("app\\base".to_owned(), vec![TypeId::mixed(&f.db)])],
        );
    }

    #[test]
    fn a_missing_argument_falls_to_the_templates_bound_when_it_has_one() {
        let f = fixture(&[r#"<?php
namespace App;
/** @template T of Bound */
class Base {}
class Child extends Base {}
class Bound {}
"#]);
        // No `@extends` tag at all: the template zips against nothing,
        // but this time it declares its own bound, so the fallback must
        // stop there instead of falling through to `mixed`.
        let bound = TypeId::class(&f.db, "app\\bound", vec![]);
        assert_eq!(
            arguments_of(&f, "app\\child"),
            &vec![("app\\base".to_owned(), vec![bound])],
        );
    }

    #[test]
    fn diamond_inheritance_takes_the_first_edge_in_walk_order() {
        let f = fixture(&[r#"<?php
namespace App;
/** @template T */
interface Shared {}
/** @implements Shared<User> */
class Left implements Shared {}
/** @implements Shared<Admin> */
interface Right extends Shared {}
/** @extends Left<User> */
class Diamond extends Left implements Right {}
class User {}
class Admin {}
"#]);
        let user = TypeId::class(&f.db, "app\\user", vec![]);
        let shared = arguments_of(&f, "app\\diamond")
            .iter()
            .find(|(key, _)| key == "app\\shared")
            .cloned();
        assert_eq!(
            shared,
            Some(("app\\shared".to_owned(), vec![user])),
            "the first edge in linearization walk order fixes the diamond",
        );
    }

    #[test]
    fn an_unresolved_ancestor_contributes_nothing() {
        let f = fixture(&[r#"<?php
namespace App;
/** @extends Vanished<User> */
class Child extends Vanished {}
class User {}
"#]);
        assert!(arguments_of(&f, "app\\child").is_empty());
    }

    #[test]
    fn ancestor_substitution_maps_the_owner_templates() {
        let f = fixture(&[r#"<?php
namespace App;
/** @template T */
class Base {}
/** @extends Base<User> */
class Child extends Base {}
class User {}
"#]);
        let map = ancestor_substitution(
            &f.db,
            f.files,
            f.stubs,
            f.configuration,
            "app\\child",
            "app\\base",
        )
        .unwrap();
        assert_eq!(
            map.binding("app\\base", "T"),
            Some(TypeId::class(&f.db, "app\\user", vec![])),
        );
        assert!(
            ancestor_substitution(
                &f.db,
                f.files,
                f.stubs,
                f.configuration,
                "app\\child",
                "app\\child",
            )
            .is_none(),
            "an own member never substitutes",
        );
    }
}
