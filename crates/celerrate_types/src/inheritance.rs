//! Generic-argument threading: class-level
//! annotations parsed at their declaring site, composed transitively
//! along linearization's ancestry into per-ancestor argument lists.
//! This is the delivery path of the Doctrine-on-Symfony repository
//! pattern: `@extends ServiceEntityRepository<User>` reaches
//! `$repository->find($id)` through these queries.
//!
//! **Stub ancestors.** A stub target
//! curated with a class refinement (`celerrate_stubs::RefinedClass`,
//! compiled from `refinements.celerrate`) threads its generic
//! arguments through [`ancestor_arguments`] exactly as a source
//! owner's `@extends`/`@implements` would: [`class_annotations`]
//! answers a stub-resolved class's templates from its `RefinedClass`,
//! and the walk contributes the refined ancestor's fixed arguments
//! (lowered under the owner's own template scope, substituted by the
//! owner's composed substitution) even where linearization records no
//! edge of its own (a stub's ancestry lives in its compiled surface,
//! not in per-edge annotations, so `ArrayIterator`'s curated
//! `implements Iterator<TKey, TValue>` would otherwise never appear
//! in `linearized_class`'s `ancestry` at all). An **uncurated** stub
//! ancestor — no `RefinedClass` on file — still contributes nothing,
//! exactly as before. Curating the phpstorm-stubs surface to carry
//! its own `@template`/`@extends` annotations, class by class, is
//! measurement-driven: an entry
//! earns its place only when the pinned corpus both names it and a
//! measured result sharpens, so most stub classes are deliberately
//! uncurated by that gate, not by omission. Today the
//! curated class seed is `ArrayIterator` alone (`refinements.celerrate`);
//! every other stub class — `SplStack`, `ArrayObject`, and the rest —
//! still contributes no generic ancestors here, recorded debt (owner:
//! curation) revisited only if a future corpus measurement demands a
//! new class entry. A receiver reaching an uncurated stub ancestor
//! still degrades to the protocol-member fallback (`flow.rs`'s
//! iteration-typing chain).

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
/// solver and the receiver substitution both read its
/// template list. `configuration` completes the input quartet the
/// annotation seam shares with `class_annotations`'s siblings
/// (`member_annotations`, `function_annotations`) — a uniform query
/// shape callers never have to special-case; a class's own docblock
/// resolves without consulting it.
///
/// A class key with no source declaration at all (an unresolvable
/// name, or a genuine stub) never has a docblock to parse: `stubs`
/// answers for it instead — a stub class
/// curated with a `RefinedClass` answers ITS templates (declaration
/// order preserved, bounds lowered under an empty scope, scope key =
/// the class key, exactly `norm_templates`'s convention for a refined
/// stub method's own templates); an uncurated stub, like an
/// undocumented source class, answers the default.
#[salsa::tracked(returns(ref))]
pub fn class_annotations<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    _configuration: ProjectConfiguration,
    class: ClassQuery<'db>,
) -> ClassAnnotations<'db> {
    let key = class.key(db);
    if crate::declared::declaring_site(db, files, crate::declared::class_like_query(db, key))
        .is_none()
    {
        // No source class-like at all: either an unresolvable name or
        // a genuine stub. A stub's own `@template`/`@extends` never
        // lives in a docblock (there is none to parse) — only in the
        // curated refinement overlay, when one exists.
        let Some(refined) = stubs.index(db).class_refinement(key) else {
            return ClassAnnotations::default();
        };
        return ClassAnnotations {
            templates: crate::declared::norm_templates(db, key, &refined.templates)
                .into_iter()
                .map(|template| ParsedTemplate {
                    name: template.name,
                    bound: template.bound,
                })
                .collect(),
            // Refined ancestors are not `ParsedAncestor`s: they carry
            // their own name (already folded, possibly transitive)
            // rather than pairing against a linearized edge's already-
            // resolved target, so `ancestor_arguments` consults
            // `class_refinement` directly for them rather than reading
            // this field. Keeping it empty here, rather than a
            // best-effort translation, avoids two representations of
            // the same fact silently drifting apart.
            ancestors: Vec::new(),
        };
    }
    let Some(docblock) = crate::declared::owner_class_docblock(
        db,
        files,
        crate::declared::class_like_query(db, key),
    ) else {
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
/// (this module's conservative-silence rule). The single home for that
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
/// Diamond inheritance resolves first-edge-wins; an uncurated stub or
/// otherwise unresolved ancestor contributes nothing; a missing
/// argument falls to the template's bound, then `mixed` (the same
/// conservative-silence rule).
///
/// A **stub** ancestry edge — `edge.resolved` absent, `edge.stub`
/// present — threads exactly like a source edge from this point on:
/// `class_annotations` already answers a curated stub target's own
/// templates, so the same `zip_templates` composition
/// applies unmodified. Once composed, [`thread_refined_ancestors`]
/// contributes that stub target's OWN curated ancestors too — edges
/// linearization itself never records, since a stub's ancestry lives
/// in its compiled surface, not in per-edge annotations.
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
        let target = match (&edge.resolved, &edge.stub) {
            (Some(resolved), _) => resolved.clone(),
            (None, Some(stub)) => stub.clone(),
            (None, None) => continue,
        };
        let is_stub_edge = edge.resolved.is_none();
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
        substitutions.insert(target.clone(), composed.clone());
        if !fixed.is_empty() {
            answers.push((target.clone(), fixed));
        }
        if is_stub_edge {
            thread_refined_ancestors(
                db,
                RefinementInputs {
                    files,
                    stubs,
                    configuration,
                },
                &target,
                &composed,
                &mut substitutions,
                &mut answers,
            );
        }
    }
    answers
}

/// The `files`/`stubs`/`configuration` triple [`thread_refined_ancestors`]
/// threads through its recursion, bundled so the function stays under
/// clippy's argument-count lint — every field is `Copy` (a salsa input
/// handle), so bundling costs nothing.
#[derive(Clone, Copy)]
struct RefinementInputs {
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
}

/// Threads a stub owner's own curated ancestors: the
/// edges [`ancestor_arguments`]'s outer walk never sees, since
/// `linearized_class` only ever records the DIRECT edge into a stub
/// boundary (`linearize.rs`'s `AncestorAnswer::Stub` arm) — a curated
/// `ArrayIterator implements Iterator<TKey, TValue>` never appears as
/// its own `AncestorEdge`. `owner_key` is a target the outer walk (or
/// a previous call to this same function) just composed a
/// substitution for; `owner_substitution` is that composed
/// substitution, keyed under `owner_key` exactly as `zip_templates`
/// binds it.
///
/// An owner with no `RefinedClass` on file answers immediately —
/// "stub owners without a refinement contribute nothing" holds at
/// every depth, not only the first. A refined ancestor's own text is
/// lowered under `owner_key`'s refined templates (the scope
/// `class_annotations`'s stub arm used to build `owner_key`'s
/// templates in the first place, so a name like `TKey` resolves to
/// the exact template `owner_substitution` has a binding for), then
/// substituted through `owner_substitution` — never re-zipped against
/// the refined ancestor's own arity, since `RefinedAncestor.arguments`
/// already IS the fixed positional list, exactly as written. A text
/// that fails to lower contributes `mixed` for that position (decision
/// 7's conservative-silence rule; the totality test
/// `every_embedded_refinement_text_lowers` keeps this branch dead for
/// the real seed). Recurses into the refined ancestor's own
/// refinement, if it has one, so a chain of curated stub classes
/// threads as far as the curation reaches; first-edge-wins throughout,
/// via the same shared `substitutions` map the outer walk uses.
fn thread_refined_ancestors<'db>(
    db: &'db dyn salsa::Database,
    inputs: RefinementInputs,
    owner_key: &str,
    owner_substitution: &Substitution<'db>,
    substitutions: &mut BTreeMap<String, Substitution<'db>>,
    answers: &mut Vec<(String, Vec<TypeId<'db>>)>,
) {
    let RefinementInputs {
        files,
        stubs,
        configuration,
    } = inputs;
    let Some(refinement) = stubs.index(db).class_refinement(owner_key) else {
        return;
    };
    let owner_templates = crate::declared::norm_templates(db, owner_key, &refinement.templates);
    let owner_scope = crate::norm::NormScope {
        key: owner_key,
        templates: &owner_templates,
    };
    for ancestor in &refinement.ancestors {
        if substitutions.contains_key(&ancestor.name) {
            // First-edge-wins, shared with the outer walk: an ancestor
            // already reached by an earlier edge (or an earlier
            // refined ancestor) keeps it.
            continue;
        }
        let fixed: Vec<TypeId<'db>> = ancestor
            .arguments
            .iter()
            .map(|text| {
                let lowered = crate::norm::lower_norm_text(db, &owner_scope, text)
                    .unwrap_or_else(|| TypeId::mixed(db));
                substitute(
                    db,
                    files,
                    stubs,
                    configuration,
                    lowered,
                    owner_substitution,
                    None,
                )
            })
            .collect();
        // The refined ancestor's own templates (if it is itself
        // curated) zip against `fixed`, so a further curated ancestor
        // can recurse against real bindings rather than an empty
        // substitution — mirroring the outer walk's own composition,
        // not `fixed` itself (that stays the plain positional list
        // above, this function's own contract).
        let ancestor_templates = &class_annotations(
            db,
            files,
            stubs,
            configuration,
            ClassQuery::new(db, ancestor.name.clone()),
        )
        .templates;
        // `zip_templates`' padded `fixed` is deliberately DISCARDED
        // here — only its substitution is kept. Padding it through, the
        // way the outer walk does, would truncate this list to the
        // ancestor's own template count, and a refined ancestor is
        // typically NOT itself curated: `Iterator` carries no
        // `RefinedClass`, so its template count is zero and the padded
        // list would be empty, dropping the very answer this function
        // exists to produce. The raw lowered list is the contract.
        //
        // The consequence, recorded rather than fixed: the
        // `fixed.len() == templates.len()` invariant the outer walk
        // guarantees (via `zip_templates`) is, for entries pushed HERE,
        // conditional on the curation writing the ancestor's arguments
        // at its real arity — `implements Iterator<TKey>` against
        // `Iterator`'s two positions pushes a length-1 entry. Both
        // consumers degrade safely rather than trusting the length:
        // `flow.rs`'s iteration typing reads `.first()`/`.get(1)` and
        // falls back when either is absent, and `solver.rs` `.zip()`s,
        // which stops at the shorter side. The curation is the place to
        // keep the arity honest; the real seed
        // (`refinements.celerrate`) does.
        let (composed, _) = zip_templates(db, &ancestor.name, ancestor_templates, &fixed);
        substitutions.insert(ancestor.name.clone(), composed.clone());
        if !fixed.is_empty() {
            answers.push((ancestor.name.clone(), fixed));
        }
        thread_refined_ancestors(
            db,
            inputs,
            &ancestor.name,
            &composed,
            substitutions,
            answers,
        );
    }
}

/// The ready-to-apply substitution for a member declared on
/// `owner_key`, consulted through `class_key`. `None` when the member
/// is the class's own or nothing is threaded — callers skip the walk.
///
/// Consumed by `declared_member_signature`, which applies this to
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
    use celerrate_semantics::{ClassQuery, linearized_class};
    use celerrate_source::FileId;
    use celerrate_stubs::{
        RefinedAncestor, RefinedClass, RefinedTemplate, StubAvailability, StubClassSurface,
        StubIndex, StubIndexInput, StubRefinements, StubSymbol, StubSymbolKind,
    };

    use super::test_support::register_fake_syntax;
    use super::{ancestor_arguments, ancestor_substitution, class_annotations};
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

    // Stub-class refinements route through these
    // same two queries rather than around them. `stub_refinement_index`
    // is a synthetic index deliberately smaller than
    // `test_support::minimal_stub_index`: `ArrayIterator` carries the
    // exact refinement seeded into `refinements.celerrate`
    // (`TKey`/`TValue`, `implements Iterator<TKey, TValue>`), `Iterator`
    // is a genuine compiled surface (so linearization has a real edge
    // to resolve into), and `SplStack` is a genuine stub symbol that
    // carries NO refinement at all — the fixture
    // `a_stub_class_without_a_refinement_still_contributes_nothing`
    // needs to prove the un-curated case honestly (see that test's own
    // comment for why the naive "assert an empty result" probe alone
    // would be vacuous).
    fn stub_refinement_index() -> StubIndex {
        fn class_like(name: &str, kind: StubSymbolKind) -> StubSymbol {
            StubSymbol {
                name: name.to_owned(),
                kind,
                availability: StubAvailability::ALWAYS,
            }
        }
        let mut index = StubIndex::new(
            vec![
                class_like("ArrayIterator", StubSymbolKind::Class),
                class_like("Iterator", StubSymbolKind::Interface),
                class_like("SplStack", StubSymbolKind::Class),
            ],
            vec![],
            vec![
                (
                    "ArrayIterator".to_owned(),
                    StubClassSurface {
                        parents: vec!["Iterator".to_owned()],
                        members: vec![],
                    },
                ),
                (
                    "Iterator".to_owned(),
                    StubClassSurface {
                        parents: vec![],
                        members: vec![],
                    },
                ),
            ],
        );
        index.set_refinements(StubRefinements::new(
            vec![],
            vec![(
                "arrayiterator".to_owned(),
                RefinedClass {
                    templates: vec![
                        RefinedTemplate {
                            name: "TKey".to_owned(),
                            bound: None,
                        },
                        RefinedTemplate {
                            name: "TValue".to_owned(),
                            bound: None,
                        },
                    ],
                    ancestors: vec![RefinedAncestor {
                        name: "iterator".to_owned(),
                        arguments: vec!["TKey".to_owned(), "TValue".to_owned()],
                    }],
                    methods: vec![],
                },
            )],
        ));
        index
    }

    /// [`fixture`], but wired to [`stub_refinement_index`] instead of
    /// `test_support::minimal_stub_index` — the curated `ArrayIterator`
    /// surface this module's tests need, which the shared minimal index
    /// deliberately does not carry (it has no refinements overlay at
    /// all).
    fn stub_refinement_fixture_with_source(sources: &[&str]) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs = StubIndexInput::builder(stub_refinement_index())
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        register_fake_syntax(&db);
        Fixture {
            db,
            files,
            stubs,
            configuration,
        }
    }

    /// [`stub_refinement_fixture_with_source`] with no source files:
    /// the stub-only test (`a_refined_stub_class_answers_its_templates`)
    /// never consults `files`.
    fn stub_refinement_fixture() -> Fixture {
        stub_refinement_fixture_with_source(&[])
    }

    #[test]
    fn a_refined_stub_class_answers_its_templates() {
        let f = stub_refinement_fixture();
        let annotations = class_annotations(
            &f.db,
            f.files,
            f.stubs,
            f.configuration,
            ClassQuery::new(&f.db, "arrayiterator".to_owned()),
        );
        let names: Vec<&str> = annotations
            .templates
            .iter()
            .map(|template| template.name.as_str())
            .collect();
        assert_eq!(names, ["TKey", "TValue"]);
    }

    #[test]
    fn a_refined_stub_ancestor_threads_through_a_source_subclass() {
        // Source code extends the refined stub class with fixed
        // arguments; the composition must reach Iterator even though
        // `Iterator` never appears as an `AncestorEdge` of its own
        // (linearization only records `RecentPosts -> ArrayIterator`;
        // `ArrayIterator -> Iterator` lives solely in the refinement).
        let f = stub_refinement_fixture_with_source(&[r#"<?php
namespace App;
/** @extends \ArrayIterator<int, \App\Post> */
class RecentPosts extends \ArrayIterator {}
class Post {}
"#]);
        let arguments = arguments_of(&f, "app\\recentposts");
        // The iterator entry carries the substituted arguments:
        // TKey := int, TValue := app\post.
        let iterator = arguments
            .iter()
            .find(|(ancestor, _)| ancestor == "iterator")
            .unwrap();
        assert_eq!(
            iterator.1,
            vec![
                TypeId::int(&f.db),
                TypeId::class(&f.db, "app\\post", vec![]),
            ],
        );
    }

    #[test]
    fn a_stub_class_without_a_refinement_still_contributes_nothing() {
        // The curation boundary stays the default: only curation opens
        // it. `SplStack` carries no refinement in this fixture;
        // extending it threads nothing.
        //
        // Controller adjudication: a probe that merely asserts
        // `ancestor_arguments(..).is_empty()` (or, equivalently,
        // `.iter().all(|entry| entry.arguments.is_empty())`) can never
        // be told apart from a vacuous one — if `SplStack` were never
        // genuinely declared in the fixture's stub index, the
        // `extends \SplStack` edge would resolve as UNRESOLVED (the
        // pre-existing, unrelated "no such name" branch), not as a
        // stub-resolved-but-uncurated one, and the same assertion
        // would still pass while pinning nothing about the curation
        // boundary at all. `ancestor_arguments` also never pushes an entry with an
        // empty argument list in the first place (`fixed.is_empty()`
        // and "not pushed" are the same condition throughout this
        // module and `flow.rs`'s `implements_iteration_protocol` doc
        // leans on that exact invariant) — so "a non-empty result
        // where every entry is empty" is unsatisfiable by
        // construction, not merely hard to arrange. The honest
        // non-vacuousness proof is over the WALK, not over
        // `ancestor_arguments`'s result: assert the `SplStack` edge
        // genuinely resolved as a stub (a non-empty, inspected
        // ancestry list) before asserting the un-curated case
        // contributes nothing.
        let f = stub_refinement_fixture_with_source(&[r#"<?php
namespace App;
class Stack extends \SplStack {}
"#]);
        let class = ClassQuery::new(&f.db, "app\\stack".to_owned());
        let linearized = linearized_class(&f.db, f.files, f.stubs, f.configuration, class)
            .clone()
            .unwrap();
        assert!(
            !linearized.ancestry.is_empty(),
            "the fixture must record a genuine ancestry edge, or this probe is vacuous",
        );
        let stub_edge = linearized
            .ancestry
            .iter()
            .find(|edge| edge.owner == "app\\stack");
        assert_eq!(
            stub_edge.and_then(|edge| edge.stub.as_deref()),
            Some("splstack"),
            "SplStack must genuinely resolve as a stub edge (not fall through the \
             unrelated 'unresolved name' branch), or this probe pins nothing",
        );
        let arguments = arguments_of(&f, "app\\stack");
        assert!(
            arguments.is_empty(),
            "uncurated stub ancestors contribute no arguments: {arguments:?}",
        );
    }
}
