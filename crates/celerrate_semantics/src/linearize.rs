//! Inheritance linearization: per class, the resolved member table.
//! Own members over trait members over inherited members, PHP's case
//! rules per kind. The walk is iterative with a visited set inside one
//! tracked query — it never demands its own kind recursively, so
//! `class A extends B; class B extends A` is a detected, flagged
//! condition, never a salsa cycle (spec section 2's mechanism).
//! Ancestry edges are kept for the type engine's generic-argument
//! threading (plan 6); stub ancestors are a recorded boundary until
//! the stub signature payload exists (plan 3).
//!
//! Precedence between two *transitive* sources of one member follows
//! walk order, which is declaration order per level: traits first (they
//! beat parents), then `extends`, then `implements`, breadth-first. That
//! order plus the final stable sort realizes own > trait > parent >
//! interfaces for the depth-one case exactly, and a deterministic order
//! for deep mixed hierarchies — refined only if the corpus proves PHP's
//! exact C3-ish order matters in practice (YAGNI: PHPStan does the same
//! simplification).
//!
//! The `cyclic` flag is true exactly when the resolved inheritance
//! graph the walk recorded — every ancestry edge whose target resolved
//! to a source class-like, as a directed graph from owner to target —
//! contains a directed cycle: some walked class-like is reachable from
//! itself. The check runs after the walk, over the edges already
//! recorded, by iteratively peeling zero-in-degree nodes (Kahn's
//! algorithm — iterative, no recursion). A diamond — one ancestor
//! reached by two distinct paths — is a DAG and stays unflagged, so a
//! legal diamond is never mistaken for a cycle.

use std::collections::{HashMap, HashSet, VecDeque};

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::ProjectConfiguration;
use celerrate_source::FileId;
use celerrate_stubs::StubIndexInput;

use crate::items::Declaration;
use crate::lookup::{
    SymbolQuery, SymbolResolution, analyzed_file_index, lookup_class_declaration, lookup_symbol,
};
use crate::members::{ClassMembers, Member, MemberKind};
use crate::queries::{item_tree, member_tree};
use crate::resolve::{UseTables, resolve_candidates};
use crate::symbols::{SymbolSpace, folded_symbol_key};

/// One class-like to linearize: its **pre-folded** ClassLike key (fold
/// with [`crate::folded_symbol_key`] before interning, so spelling
/// variants of one class share one memo).
#[salsa::interned(debug)]
pub struct ClassQuery<'db> {
    #[returns(ref)]
    pub key: String,
}

/// Where one linearized member came from, relative to the queried
/// class: its own body, a trait it uses directly, or an ancestor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberOrigin {
    Own,
    Trait,
    Inherited,
}

/// The inheritance relation an ancestry edge expresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AncestorRelation {
    Extends,
    Implements,
    UsesTrait,
}

/// One inheritance edge, kept in walk order for the type engine's
/// generic-argument threading (plan 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AncestorEdge {
    pub relation: AncestorRelation,
    /// The ancestor name as written at the declaring site.
    pub written: String,
    /// The folded key when the name resolved to a source class-like;
    /// `None` for a stub or an unresolved edge.
    pub resolved: Option<String>,
    /// The folded key of the class that declared the edge.
    pub owner: String,
}

/// One member of the linearized table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearizedMember {
    /// The folded member key (methods lowercased, everything else
    /// verbatim).
    pub key: String,
    /// The cloned member payload.
    pub member: Member,
    /// The folded key of the declaring class.
    pub owner: String,
    pub origin: MemberOrigin,
}

/// The linearized view of one class-like: its resolved member table,
/// the ancestry it walked, the stub boundary it reached, and whether an
/// inheritance cycle was broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearizedClass {
    /// Sorted by `(kind, key)`; the first entry per `(kind, key)` wins.
    pub members: Vec<LinearizedMember>,
    /// The inheritance edges, in walk order.
    pub ancestry: Vec<AncestorEdge>,
    /// The folded keys of ancestors that resolved to stubs only, sorted
    /// and deduplicated.
    pub stub_ancestors: Vec<String>,
    /// A genuine inheritance cycle was detected and broken.
    pub cyclic: bool,
}

/// The folded lookup key of one member: methods fold ASCII case (PHP
/// resolves method names case-insensitively), everything else keeps its
/// spelling (property, constant, and enum-case names are
/// case-sensitive).
pub fn folded_member_key(kind: MemberKind, name: &str) -> String {
    match kind {
        MemberKind::Method => name.to_ascii_lowercase(),
        MemberKind::Property | MemberKind::ClassConstant | MemberKind::EnumCase => name.to_owned(),
    }
}

/// The linearized member table of one class-like, or `None` when the
/// queried key is not a source class-like. The walk is iterative with a
/// visited set inside this one tracked query, so an inheritance cycle is
/// a flagged condition, never a salsa cycle.
#[salsa::tracked(returns(ref))]
pub fn linearized_class<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    class: ClassQuery<'db>,
) -> Option<LinearizedClass> {
    let root_key = class.key(db).clone();

    let mut visited: HashSet<String> = HashSet::new();
    let mut members: Vec<LinearizedMember> = Vec::new();
    let mut ancestry: Vec<AncestorEdge> = Vec::new();
    let mut stub_ancestors: Vec<String> = Vec::new();
    // Whether the queried key itself fetched a source class-like: the
    // root is dequeued first, so the first fetch is the root's.
    let mut root_fetched = false;

    let mut queue: VecDeque<(String, MemberOrigin)> = VecDeque::new();
    queue.push_back((root_key, MemberOrigin::Own));

    while let Some((key, origin)) = queue.pop_front() {
        if !visited.insert(key.clone()) {
            // Already linearized: a diamond re-visit or a cycle's
            // closing edge. The post-walk graph check tells them apart.
            continue;
        }
        let Some(found) = fetch(db, files, &key) else {
            continue;
        };
        root_fetched = true;

        for member in &found.group.members {
            members.push(LinearizedMember {
                key: folded_member_key(member.kind, &member.name),
                member: member.clone(),
                owner: key.clone(),
                origin,
            });
        }

        let Some(declaration) = found.declaration.as_ref() else {
            continue;
        };
        for (relation, written) in edges_of(declaration) {
            let answer = resolve_ancestor(
                db,
                files,
                stubs,
                configuration,
                found.file,
                &found.namespace,
                &written,
            );
            ancestry.push(AncestorEdge {
                relation,
                written,
                resolved: answer.source_key(),
                owner: key.clone(),
            });
            match answer {
                AncestorAnswer::Source { folded_key } => {
                    let next_origin = match (origin, relation) {
                        (MemberOrigin::Own, AncestorRelation::UsesTrait) => MemberOrigin::Trait,
                        _ => MemberOrigin::Inherited,
                    };
                    queue.push_back((folded_key, next_origin));
                }
                AncestorAnswer::Stub { folded_key } => stub_ancestors.push(folded_key),
                AncestorAnswer::Unresolved => {}
            }
        }
    }

    if !root_fetched {
        return None;
    }

    let cyclic = contains_cycle(&ancestry);

    members.sort_by(|a, b| {
        (a.member.kind as u8, a.key.as_str()).cmp(&(b.member.kind as u8, b.key.as_str()))
    });
    stub_ancestors.sort();
    stub_ancestors.dedup();

    Some(LinearizedClass {
        members,
        ancestry,
        stub_ancestors,
        cyclic,
    })
}

/// Whether the resolved inheritance graph the walk recorded contains a
/// directed cycle. The nodes are the folded keys the edges mention; an
/// edge runs from its owner to its resolved target (stub and unresolved
/// edges resolve to `None` and take no part). Kahn's algorithm,
/// iteratively: peel every zero-in-degree node; whatever cannot be
/// peeled sits on or behind a cycle. Only existence is asked, so map
/// iteration order cannot change the answer.
fn contains_cycle(ancestry: &[AncestorEdge]) -> bool {
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for edge in ancestry {
        let Some(target) = edge.resolved.as_deref() else {
            continue;
        };
        outgoing
            .entry(edge.owner.as_str())
            .or_default()
            .push(target);
        in_degree.entry(edge.owner.as_str()).or_insert(0);
        let degree = in_degree.entry(target).or_insert(0);
        *degree = degree.saturating_add(1);
    }
    let mut ready: Vec<&str> = in_degree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(node, _)| *node)
        .collect();
    let mut peeled = 0_usize;
    while let Some(node) = ready.pop() {
        peeled = peeled.saturating_add(1);
        for &target in outgoing.get(node).into_iter().flatten() {
            if let Some(degree) = in_degree.get_mut(target) {
                *degree = degree.saturating_sub(1);
                if *degree == 0 {
                    ready.push(target);
                }
            }
        }
    }
    peeled < in_degree.len()
}

/// One source class-like loaded by its folded key: its member group,
/// its top-level declaration (absent for an anonymous class), the file
/// that declared it, and the namespace to resolve its ancestors in.
struct Fetched {
    group: ClassMembers,
    declaration: Option<Declaration>,
    file: SourceFile,
    namespace: String,
}

/// Loads the source class-like named by one folded key: `None` when the
/// key names no source class-like (a stub, an unknown name, or a
/// non-class symbol).
fn fetch(db: &dyn salsa::Database, files: AnalyzedFileSet, key: &str) -> Option<Fetched> {
    let query = SymbolQuery::new(db, SymbolSpace::ClassLike, key.to_owned());
    let (_, ast_id) = lookup_class_declaration(db, files, query)?;
    let file = file_of(db, files, ast_id.file)?;
    let group = member_tree(db, file)
        .classes
        .iter()
        .find(|group| group.ast_id == ast_id)?
        .clone();
    let declaration = item_tree(db, file)
        .declarations
        .iter()
        .find(|declaration| declaration.ast_id == ast_id)
        .cloned();
    let namespace = group.namespace.clone();
    Some(Fetched {
        group,
        declaration,
        file,
        namespace,
    })
}

/// The salsa handle of one file, found by binary search in the sorted
/// file index.
fn file_of(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    file_id: FileId,
) -> Option<SourceFile> {
    let index = analyzed_file_index(db, files);
    let position = index.binary_search_by_key(&file_id, |(id, _)| *id).ok()?;
    index.get(position).map(|(_, file)| *file)
}

/// The inheritance edges of one declaration, in precedence order:
/// traits first (they beat parents), then `extends`, then `implements`.
fn edges_of(declaration: &Declaration) -> Vec<(AncestorRelation, String)> {
    let mut edges = Vec::new();
    for name in &declaration.trait_uses {
        edges.push((AncestorRelation::UsesTrait, name.clone()));
    }
    for name in &declaration.extends {
        edges.push((AncestorRelation::Extends, name.clone()));
    }
    for name in &declaration.implements {
        edges.push((AncestorRelation::Implements, name.clone()));
    }
    edges
}

/// What one written ancestor name resolved to at its declaring site.
enum AncestorAnswer {
    /// A source class-like, keyed by its folded name.
    Source { folded_key: String },
    /// A compiled stub class-like: a recorded boundary, keyed by its
    /// folded name.
    Stub { folded_key: String },
    /// Neither: an unresolved edge.
    Unresolved,
}

impl AncestorAnswer {
    /// The folded key when the name resolved to a source class-like;
    /// `None` for a stub or unresolved answer.
    fn source_key(&self) -> Option<String> {
        match self {
            AncestorAnswer::Source { folded_key } => Some(folded_key.clone()),
            AncestorAnswer::Stub { .. } | AncestorAnswer::Unresolved => None,
        }
    }
}

/// Resolves one written ancestor name at its declaring site: PHP's
/// candidate order, the first candidate that a source class-like or a
/// stub class-like answers wins (mirroring `resolve_name`, but keeping
/// the origin).
fn resolve_ancestor(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    namespace: &str,
    written: &str,
) -> AncestorAnswer {
    let tables = UseTables::for_namespace(item_tree(db, file), namespace);
    for candidate in resolve_candidates(written, SymbolSpace::ClassLike, namespace, &tables) {
        let folded_key = folded_symbol_key(SymbolSpace::ClassLike, &candidate);
        let query = SymbolQuery::new(db, SymbolSpace::ClassLike, folded_key.clone());
        if lookup_class_declaration(db, files, query).is_some() {
            return AncestorAnswer::Source { folded_key };
        }
        if let Some(SymbolResolution::Stub { kind, .. }) =
            lookup_symbol(db, files, stubs, configuration, query)
            && SymbolSpace::of_stub_kind(kind) == SymbolSpace::ClassLike
        {
            return AncestorAnswer::Stub { folded_key };
        }
    }
    AncestorAnswer::Unresolved
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    use super::{ClassQuery, LinearizedClass, MemberOrigin, linearized_class};
    use crate::members::MemberKind;
    use crate::symbols::{SymbolSpace, folded_symbol_key};

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
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![
            StubSymbol {
                name: "Exception".to_owned(),
                kind: StubSymbolKind::Class,
                availability: StubAvailability::ALWAYS,
            },
            StubSymbol {
                name: "strlen".to_owned(),
                kind: StubSymbolKind::Function,
                availability: StubAvailability::ALWAYS,
            },
        ]))
        .durability(salsa::Durability::HIGH)
        .new(&db);
        let configuration = ProjectConfiguration::builder(PhpVersionRange::new(
            PhpVersion::new(8, 1),
            PhpVersion::new(8, 5),
        ))
        .durability(salsa::Durability::MEDIUM)
        .new(&db);
        Fixture {
            db,
            files,
            stubs,
            configuration,
        }
    }

    fn fixture_one(source: &str) -> Fixture {
        fixture(&[source])
    }

    fn linearize(fixture: &Fixture, written: &str) -> Option<LinearizedClass> {
        let query = ClassQuery::new(
            &fixture.db,
            folded_symbol_key(SymbolSpace::ClassLike, written),
        );
        linearized_class(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
        .clone()
    }

    fn member_owner(
        class: &LinearizedClass,
        kind: MemberKind,
        key: &str,
    ) -> Option<(String, MemberOrigin)> {
        class
            .members
            .iter()
            .find(|entry| entry.member.kind == kind && entry.key == key)
            .map(|entry| (entry.owner.clone(), entry.origin))
    }

    #[test]
    fn own_members_shadow_inherited_ones() {
        let fixture = fixture(&[
            "<?php class Base { public function hello() {} public function only() {} }",
            "<?php class Child extends Base { public function hello() {} }",
        ]);
        let child = linearize(&fixture, "Child").unwrap();
        assert_eq!(
            member_owner(&child, MemberKind::Method, "hello"),
            Some(("child".to_owned(), MemberOrigin::Own)),
        );
        assert_eq!(
            member_owner(&child, MemberKind::Method, "only"),
            Some(("base".to_owned(), MemberOrigin::Inherited)),
        );
        assert!(!child.cyclic);
    }

    #[test]
    fn trait_members_beat_inherited_and_lose_to_own() {
        let fixture = fixture(&[
            "<?php trait Greets { public function hello() {} public function bye() {} }",
            "<?php class Base { public function hello() {} public function bye() {} public function stays() {} }",
            "<?php class Child extends Base { use Greets; public function hello() {} }",
        ]);
        let child = linearize(&fixture, "Child").unwrap();
        assert_eq!(
            member_owner(&child, MemberKind::Method, "hello").unwrap().1,
            MemberOrigin::Own,
        );
        assert_eq!(
            member_owner(&child, MemberKind::Method, "bye"),
            Some(("greets".to_owned(), MemberOrigin::Trait)),
        );
        assert_eq!(
            member_owner(&child, MemberKind::Method, "stays").unwrap().1,
            MemberOrigin::Inherited,
        );
    }

    #[test]
    fn method_keys_fold_case_and_property_keys_do_not() {
        let fixture = fixture(&[
            "<?php class Base { public function CamelCase() {} public $Exact; }",
            "<?php class Child extends Base {}",
        ]);
        let child = linearize(&fixture, "Child").unwrap();
        assert!(member_owner(&child, MemberKind::Method, "camelcase").is_some());
        assert!(member_owner(&child, MemberKind::Property, "Exact").is_some());
        assert!(member_owner(&child, MemberKind::Property, "exact").is_none());
    }

    #[test]
    fn interface_constants_and_methods_inherit_through_extends_chains() {
        let fixture = fixture(&[
            "<?php interface Upper { const K = 1; public function f(); }",
            "<?php interface Lower extends Upper {}",
            "<?php class Impl implements Lower { public function f() {} }",
        ]);
        let implementation = linearize(&fixture, "Impl").unwrap();
        assert_eq!(
            member_owner(&implementation, MemberKind::ClassConstant, "K"),
            Some(("upper".to_owned(), MemberOrigin::Inherited)),
        );
        assert_eq!(
            member_owner(&implementation, MemberKind::Method, "f")
                .unwrap()
                .1,
            MemberOrigin::Own,
        );
    }

    #[test]
    fn imports_resolve_ancestor_names_at_the_declaring_site() {
        // The extends name resolves in Child's file with Child's
        // imports and namespace — not the asker's.
        let fixture = fixture(&[
            "<?php namespace Lib; class Base { public function inherited() {} }",
            "<?php namespace App; use Lib\\Base; class Child extends Base {}",
        ]);
        let child = linearize(&fixture, "App\\Child").unwrap();
        assert_eq!(
            member_owner(&child, MemberKind::Method, "inherited"),
            Some(("lib\\base".to_owned(), MemberOrigin::Inherited)),
        );
    }

    #[test]
    fn an_inheritance_cycle_is_broken_and_flagged() {
        let fixture = fixture(&[
            "<?php class A extends B { public function fromA() {} }",
            "<?php class B extends A { public function fromB() {} }",
        ]);
        let a = linearize(&fixture, "A").unwrap();
        assert!(a.cyclic);
        // The walk terminates and still linearizes what it saw once.
        assert!(member_owner(&a, MemberKind::Method, "froma").is_some());
        assert!(member_owner(&a, MemberKind::Method, "fromb").is_some());
        let self_cycle_fixture = fixture_one("<?php class Selfish extends Selfish {}");
        let selfish = linearize(&self_cycle_fixture, "Selfish").unwrap();
        assert!(selfish.cyclic);
    }

    #[test]
    fn a_cycle_closing_off_the_first_traversed_path_is_still_flagged() {
        // R reaches C first through A, so C's edge to B closes a
        // genuine cycle (B -> C -> B) that no single traversal path
        // from R contains when C's edges are examined. A per-path
        // check misses it; the post-walk graph check must not.
        let fixture = fixture(&[
            "<?php interface R extends A, B {}",
            "<?php interface A extends C {}",
            "<?php interface B extends C {}",
            "<?php interface C extends B {}",
        ]);
        let root = linearize(&fixture, "R").unwrap();
        assert!(root.cyclic);
    }

    #[test]
    fn a_stub_ancestor_is_a_recorded_boundary() {
        let fixture = fixture(&["<?php class AppException extends \\Exception {}"]);
        let class = linearize(&fixture, "AppException").unwrap();
        assert_eq!(class.stub_ancestors, vec!["exception".to_owned()]);
        assert!(!class.cyclic);
    }

    #[test]
    fn an_unresolvable_ancestor_leaves_an_unresolved_edge() {
        let fixture = fixture(&["<?php class Child extends Missing {}"]);
        let child = linearize(&fixture, "Child").unwrap();
        let edge = child.ancestry.first().unwrap();
        assert_eq!(edge.written, "Missing");
        assert_eq!(edge.resolved, None);
        assert!(child.stub_ancestors.is_empty());
    }

    #[test]
    fn a_non_class_key_answers_none() {
        let fixture = fixture(&["<?php function free() {}"]);
        assert!(linearize(&fixture, "free").is_none());
    }

    #[test]
    fn diamond_interfaces_keep_the_first_edge_deterministically() {
        let fixture = fixture(&[
            "<?php interface Left { const K = 1; }",
            "<?php interface Right { const K = 2; }",
            "<?php class Both implements Left, Right {}",
        ]);
        let both = linearize(&fixture, "Both").unwrap();
        // Edge order is declaration order: Left wins, always.
        assert_eq!(
            member_owner(&both, MemberKind::ClassConstant, "K"),
            Some(("left".to_owned(), MemberOrigin::Inherited)),
        );
    }
}
