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

/// The per-kind suppression facts a class carries: whether the resolved
/// table defines a magic method that answers otherwise-unknown members,
/// and whether the class opts into dynamic properties.
///
/// `stdClass` is not marked here: it is a compiled stub, so a class
/// extending it records the `stdclass` stub ancestor instead, and plan 8
/// reads `stub_ancestors` to grant it dynamic properties. This struct
/// only carries facts derivable from the source table and its own
/// attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MagicMarkers {
    /// `__get` is defined: unknown *property* reads are suppressed.
    pub has_magic_get: bool,
    /// `__set` is defined: unknown *property* writes may be suppressed
    /// (plan 8 decides).
    pub has_magic_set: bool,
    /// `__call` is defined: unknown *instance method* calls are
    /// suppressed.
    pub has_magic_call: bool,
    /// `__callStatic` is defined: unknown *static method* calls are
    /// suppressed.
    pub has_magic_call_static: bool,
    /// `#[AllowDynamicProperties]` is present, own or inherited from any
    /// visited source ancestor.
    pub allows_dynamic_properties: bool,
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
    /// The per-kind magic-method and dynamic-property suppression facts.
    pub magic: MagicMarkers,
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
    // Set once any visited source class-like opts into dynamic
    // properties, own or inherited.
    let mut allows_dynamic = false;
    // Whether the queried key itself fetched a source class-like: the
    // root is dequeued first, so the first fetch is the root's.
    let mut root_fetched = false;

    // The queue carries, alongside the folded key and origin, the
    // resolved trait adaptations that the using class attached to this
    // trait edge. Only trait entries carry `Some`; the context filters
    // to this trait's key at push time.
    let mut queue: VecDeque<(String, MemberOrigin, Option<ResolvedAdaptations>)> = VecDeque::new();
    queue.push_back((root_key, MemberOrigin::Own, None));

    while let Some((key, origin, adaptations)) = queue.pop_front() {
        if !visited.insert(key.clone()) {
            // Already linearized: a diamond re-visit or a cycle's
            // closing edge. The post-walk graph check tells them apart.
            continue;
        }
        let Some(found) = fetch(db, files, &key) else {
            continue;
        };
        root_fetched = true;

        if !allows_dynamic
            && found
                .group
                .attribute_names
                .iter()
                .any(|name| is_allow_dynamic_properties(name))
        {
            allows_dynamic = true;
        }

        for member in &found.group.members {
            push_member(member, &key, origin, adaptations.as_ref(), &mut members);
        }

        let Some(declaration) = found.declaration.as_ref() else {
            continue;
        };
        // The adaptations of each trait-use clause, keyed by the folded
        // key of every trait the clause names, so a trait edge can pick
        // up the context to hand its members. Resolved at the using
        // site, reusing the same candidate order as the edges.
        let clause_context = resolve_clause_context(db, files, stubs, configuration, &found);
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
                    let next_adaptations = if next_origin == MemberOrigin::Trait {
                        clause_context.get(&folded_key).cloned()
                    } else {
                        None
                    };
                    queue.push_back((folded_key, next_origin, next_adaptations));
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

    let magic = magic_markers(&members, allows_dynamic);

    Some(LinearizedClass {
        members,
        ancestry,
        stub_ancestors,
        cyclic,
        magic,
    })
}

/// One trait-use clause's `insteadof`/`as` adaptations, resolved at the
/// using site: every trait reference folded to the key the walk uses,
/// so a trait's members can filter by their own key. Cheap to clone;
/// clauses carry a handful of adaptations at most.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ResolvedAdaptations {
    precedences: Vec<ResolvedPrecedence>,
    aliases: Vec<ResolvedAlias>,
}

/// A resolved `insteadof`: the member it names and the folded keys of
/// the traits it excludes. Unresolvable excluded names are dropped, so
/// they exclude nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedPrecedence {
    member: String,
    excluded_keys: Vec<String>,
}

/// A resolved `as`: the member it names, the providing trait when the
/// reference was qualified (`A::m as ...`), the adapted visibility, and
/// the new name. A qualified reference whose trait did not resolve keeps
/// `qualified` true with `trait_key` `None`, so it matches no trait.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedAlias {
    member: String,
    qualified: bool,
    trait_key: Option<String>,
    visibility: Option<crate::members::Visibility>,
    alias: Option<String>,
}

impl ResolvedAlias {
    /// Whether this alias applies to a member of the given kind and
    /// folded key provided by the trait keyed `owner`.
    fn matches(&self, kind: MemberKind, member_key: &str, owner: &str) -> bool {
        if folded_member_key(kind, &self.member) != member_key {
            return false;
        }
        !self.qualified || self.trait_key.as_deref() == Some(owner)
    }
}

/// Whether an `insteadof` excludes the given member provided by trait
/// `owner`: some precedence names the same member key and lists this
/// trait among its excluded (losing) traits.
fn is_excluded(
    context: &ResolvedAdaptations,
    kind: MemberKind,
    member_key: &str,
    owner: &str,
) -> bool {
    context.precedences.iter().any(|precedence| {
        folded_member_key(kind, &precedence.member) == member_key
            && precedence
                .excluded_keys
                .iter()
                .any(|excluded| excluded == owner)
    })
}

/// Pushes one class member into the table, applying trait adaptations
/// when the member came from a directly-used trait. Plain own or
/// inherited members (and trait members with no adaptations) push once,
/// verbatim.
fn push_member(
    member: &Member,
    owner: &str,
    origin: MemberOrigin,
    adaptations: Option<&ResolvedAdaptations>,
    members: &mut Vec<LinearizedMember>,
) {
    let member_key = folded_member_key(member.kind, &member.name);
    let context = match (origin, adaptations) {
        (MemberOrigin::Trait, Some(context)) => context,
        _ => {
            members.push(LinearizedMember {
                key: member_key,
                member: member.clone(),
                owner: owner.to_owned(),
                origin,
            });
            return;
        }
    };

    // An `as` with a new name adds an entry regardless of any
    // `insteadof` on the original; an `as` with only a visibility
    // rewrites the original entry that is about to push.
    let mut visibility_override = None;
    for alias in &context.aliases {
        if !alias.matches(member.kind, &member_key, owner) {
            continue;
        }
        match &alias.alias {
            Some(new_name) => {
                let mut renamed = member.clone();
                new_name.clone_into(&mut renamed.name);
                if let Some(visibility) = alias.visibility {
                    renamed.flags.visibility = visibility;
                }
                members.push(LinearizedMember {
                    key: folded_member_key(member.kind, new_name),
                    member: renamed,
                    owner: owner.to_owned(),
                    origin,
                });
            }
            None => {
                if let Some(visibility) = alias.visibility {
                    visibility_override = Some(visibility);
                }
            }
        }
    }

    if is_excluded(context, member.kind, &member_key, owner) {
        return;
    }

    let mut original = member.clone();
    if let Some(visibility) = visibility_override {
        original.flags.visibility = visibility;
    }
    members.push(LinearizedMember {
        key: member_key,
        member: original,
        owner: owner.to_owned(),
        origin,
    });
}

/// The adaptations of each trait-use clause of one class, resolved and
/// indexed by the folded key of every trait the clause names. A trait
/// edge looks its key up here to carry the context to its members.
fn resolve_clause_context(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    found: &Fetched,
) -> HashMap<String, ResolvedAdaptations> {
    let mut context: HashMap<String, ResolvedAdaptations> = HashMap::new();
    for clause in &found.group.trait_uses {
        if clause.adaptations.is_empty() {
            continue;
        }
        // The folded key of each trait the clause names, so excluded and
        // qualified references resolve through the same candidate order.
        let mut key_of: HashMap<&str, String> = HashMap::new();
        for name in &clause.names {
            if let Some(folded_key) = resolve_ancestor(
                db,
                files,
                stubs,
                configuration,
                found.file,
                &found.namespace,
                name,
            )
            .source_key()
            {
                key_of.insert(name.as_str(), folded_key);
            }
        }
        let resolved = resolve_adaptations(&clause.adaptations, &key_of);
        for name in &clause.names {
            if let Some(folded_key) = key_of.get(name.as_str()) {
                context.insert(folded_key.clone(), resolved.clone());
            }
        }
    }
    context
}

/// Resolves one clause's written adaptations against the folded keys of
/// its traits.
fn resolve_adaptations(
    adaptations: &[crate::members::TraitAdaptation],
    key_of: &HashMap<&str, String>,
) -> ResolvedAdaptations {
    use crate::members::TraitAdaptation;
    let mut resolved = ResolvedAdaptations::default();
    for adaptation in adaptations {
        match adaptation {
            TraitAdaptation::Precedence {
                member, excluded, ..
            } => {
                resolved.precedences.push(ResolvedPrecedence {
                    member: member.clone(),
                    excluded_keys: excluded
                        .iter()
                        .filter_map(|name| key_of.get(name.as_str()).cloned())
                        .collect(),
                });
            }
            TraitAdaptation::Alias {
                trait_name,
                member,
                visibility,
                alias,
            } => {
                resolved.aliases.push(ResolvedAlias {
                    member: member.clone(),
                    qualified: trait_name.is_some(),
                    trait_key: trait_name
                        .as_ref()
                        .and_then(|name| key_of.get(name.as_str()).cloned()),
                    visibility: *visibility,
                    alias: alias.clone(),
                });
            }
        }
    }
    resolved
}

/// The suppression facts of a finished table: a magic method is a member
/// like any other, so its presence is a folded-key lookup.
fn magic_markers(members: &[LinearizedMember], allows_dynamic: bool) -> MagicMarkers {
    let has = |name: &str| {
        members
            .iter()
            .any(|entry| entry.member.kind == MemberKind::Method && entry.key == name)
    };
    MagicMarkers {
        has_magic_get: has("__get"),
        has_magic_set: has("__set"),
        has_magic_call: has("__call"),
        has_magic_call_static: has("__callstatic"),
        allows_dynamic_properties: allows_dynamic,
    }
}

/// Whether a written attribute name is `AllowDynamicProperties`.
/// Attribute names are class names: case-insensitive, and only the last
/// namespace segment is compared.
fn is_allow_dynamic_properties(written: &str) -> bool {
    written
        .rsplit('\\')
        .next()
        .is_some_and(|segment| segment.eq_ignore_ascii_case("AllowDynamicProperties"))
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
    fn insteadof_excludes_and_as_aliases_trait_members() {
        let fixture = fixture(&[
            "<?php trait B { public function hello() { return 'b'; } }",
            "<?php trait C { public function hello() { return 'c'; } }",
            "<?php class A { use B, C { B::hello insteadof C; C::hello as protected hi; } }",
        ]);
        let a = linearize(&fixture, "A").unwrap();
        // B::hello won; C::hello is excluded under its own name…
        assert_eq!(
            member_owner(&a, MemberKind::Method, "hello"),
            Some(("b".to_owned(), MemberOrigin::Trait)),
        );
        // …but re-enters under the alias, with the adapted visibility.
        let aliased = a
            .members
            .iter()
            .find(|entry| entry.key == "hi" && entry.member.kind == MemberKind::Method)
            .unwrap();
        assert_eq!(aliased.owner, "c");
        assert_eq!(
            aliased.member.flags.visibility,
            crate::members::Visibility::Protected,
        );
    }

    #[test]
    fn magic_methods_mark_the_class_own_or_inherited() {
        let fixture = fixture(&[
            "<?php class Base { public function __get($name) {} }",
            "<?php class Child extends Base { public function __call($name, $arguments) {} }",
        ]);
        let child = linearize(&fixture, "Child").unwrap();
        assert!(child.magic.has_magic_get);
        assert!(child.magic.has_magic_call);
        assert!(!child.magic.has_magic_set);
        assert!(!child.magic.has_magic_call_static);
    }

    #[test]
    fn the_allow_dynamic_properties_attribute_marks_the_class() {
        let fixture = fixture(&[
            "<?php #[AllowDynamicProperties] class Loose {}",
            "<?php class Child extends Loose {}",
        ]);
        assert!(
            linearize(&fixture, "Loose")
                .unwrap()
                .magic
                .allows_dynamic_properties
        );
        assert!(
            linearize(&fixture, "Child")
                .unwrap()
                .magic
                .allows_dynamic_properties
        );
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
