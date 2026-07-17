//! Inheritance linearization: per class, the resolved member table.
//! Own members over trait members over inherited members, PHP's case
//! rules per kind. The walk is iterative with a visited set inside one
//! tracked query — it never demands its own kind recursively, so
//! `class A extends B; class B extends A` is a detected, flagged
//! condition, never a salsa cycle (spec section 2's mechanism).
//! Ancestry edges are kept for the type engine's generic-argument
//! threading (`celerrate_types::inheritance`'s `ancestor_arguments`);
//! stub ancestors are a recorded boundary until the stub signature
//! payload exists (plan 3).
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
use celerrate_stubs::{StubIndexInput, StubMember, StubMemberKind};

use crate::ast_id::AstId;
use crate::index::{StubSignatureTable, stub_frontier, stub_signature_table};
use crate::items::{Declaration, DeclarationKind};
use crate::lookup::{
    SymbolQuery, SymbolResolution, analyzed_file_index, lookup_class_declaration, lookup_symbol,
};
use crate::members::{ClassMembers, Member, MemberKind};
use crate::queries::{item_tree, member_tree};
use crate::resolve::{UseTables, resolve_candidates};
use crate::symbols::{SymbolSpace, folded_symbol_key};
use crate::virtual_symbols::{VirtualMember, VirtualMemberKind, VirtualSymbolRegistry};

/// One class-like to linearize: its **pre-folded** ClassLike key (fold
/// with [`crate::folded_symbol_key`] before interning, so spelling
/// variants of one class share one memo).
#[salsa::interned(debug)]
pub struct ClassQuery<'db> {
    #[returns(ref)]
    pub key: String,
}

/// Where one linearized member came from, relative to the queried
/// class: its own body, a trait, or an ancestor.
///
/// `Trait` carries its **anchor**: the folded key of the class that
/// wrote the `use` clause the member arrived through — reached from the
/// queried class by `extends` edges alone, then trait-use edges the rest
/// of the way. PHP pastes a trait's body into its user at compile time,
/// so `self`, `parent` and the member table a trait body sees are the
/// *user's*, and `self` does not follow late static binding: queried
/// through a subclass of the user, the anchor is still the user, not the
/// queried class. The anchor is carried as data rather than inferred
/// from the tag, because the tag alone cannot tell the direct-use case
/// (where the queried class *is* the user) from a deeper one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MemberOrigin {
    Own,
    Trait { anchor: String },
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
/// generic-argument threading (`ancestor_arguments`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AncestorEdge {
    pub relation: AncestorRelation,
    /// The ancestor name as written at the declaring site.
    pub written: String,
    /// The folded key when the name resolved to a source class-like;
    /// `None` for a stub or an unresolved edge.
    pub resolved: Option<String>,
    /// The folded key when the edge resolved to a stub class-like;
    /// `None` for source and unresolved edges. Exactly one of
    /// `resolved`/`stub` is `Some` on a resolved edge; both `None` means
    /// the edge is unresolved.
    pub stub: Option<String>,
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

/// One annotation-declared member in linearized position. Sorted with
/// the real members' convention: stable by (kind, key), nearest
/// declaration first — the first entry per (kind, key) wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearizedVirtualMember {
    /// Folded member key (method names lowercased, property names
    /// verbatim), from `folded_member_key`.
    pub key: String,
    pub member: VirtualMember,
    /// Folded key of the declaring class-like.
    pub owner: String,
}

/// The per-kind suppression facts a class carries: whether the resolved
/// table defines a magic method that answers otherwise-unknown members,
/// and whether the class opts into dynamic properties.
///
/// `stdClass` is not marked here: it is a compiled stub, so a class
/// extending it records the `stdclass` stub ancestor instead, and the
/// unknown-member family (`celerrate_types::checks::receivers::member_existence`)
/// reads `stub_ancestors` to grant it dynamic properties. This struct
/// only carries facts derivable from the source table and its own
/// attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MagicMarkers {
    /// `__get` is defined: unknown *property* reads are suppressed.
    pub has_magic_get: bool,
    /// `__set` is defined: unknown *property* writes are suppressed too
    /// (`checks::receivers::atom_existence` treats `__get`/`__set`
    /// uniformly, the conservative side of not distinguishing a read
    /// from a write context).
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
    /// The annotation-declared members contributed by the registered
    /// providers, walked and sorted with the same convention as
    /// `members`. Empty when no registry is set (the no-plugin path).
    pub virtual_members: Vec<LinearizedVirtualMember>,
    /// The inheritance edges, in walk order.
    pub ancestry: Vec<AncestorEdge>,
    /// The folded keys of ancestors that resolved to stubs, transitively
    /// through the compiled parent links, sorted and deduplicated.
    pub stub_ancestors: Vec<String>,
    /// A genuine inheritance cycle was detected and broken.
    pub cyclic: bool,
    /// A genuinely opaque boundary remains: an unresolved edge, or a stub
    /// ancestor whose surface (or a transitive parent surface) is missing
    /// from the compiled payload. `false` means the hierarchy is fully
    /// walked.
    pub has_opaque_edge: bool,
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

/// The synthetic folded key of an anonymous class. `@` and `:` are
/// illegal in PHP names, so the form can never collide with a real
/// folded key, and `folded_symbol_key` maps it to itself (already
/// lowercase, no leading backslash).
pub fn anonymous_class_key(ast_id: AstId) -> String {
    format!("class@anonymous:{}:{}", ast_id.file.as_u32(), ast_id.index)
}

/// The inverse of [`anonymous_class_key`]; `None` for any real name.
pub fn parse_anonymous_class_key(key: &str) -> Option<AstId> {
    let rest = key.strip_prefix("class@anonymous:")?;
    let (file, index) = rest.split_once(':')?;
    Some(AstId {
        file: FileId::new(file.parse().ok()?),
        index: index.parse().ok()?,
    })
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
    // Fetched once, up front: both the implicit enum edges below and
    // the stub-frontier expansion at the end of the walk read the same
    // folded signature table.
    let table = stub_signature_table(db, stubs);

    let mut visited: HashSet<String> = HashSet::new();
    let mut members: Vec<LinearizedMember> = Vec::new();
    let mut virtual_entries: Vec<LinearizedVirtualMember> = Vec::new();
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
    // to this trait's key at push time. The origin's anchor rides along
    // inside the tag itself, fixed at the first trait-use edge and
    // carried forward from there (see `next_origin`).
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
            push_member(member, &key, &origin, adaptations.as_ref(), &mut members);
        }

        // Virtual members: annotation-declared, contributed by the
        // registered providers over the class-like's own docblock. The
        // registry is a singleton input set once at the composition root;
        // an unset registry (every plain test database) is the no-plugin
        // path. Walk order here is the ancestry order, so after the stable
        // sort the first entry per (kind, key) is the nearest declaration.
        if let Some(registry) = VirtualSymbolRegistry::try_get(db)
            && let Some(docblock) = &found.group.docblock
        {
            for registration in registry.registrations(db) {
                for member in registration.provider.virtual_members(docblock) {
                    let kind = match member.kind {
                        VirtualMemberKind::Method => MemberKind::Method,
                        VirtualMemberKind::Property => MemberKind::Property,
                    };
                    virtual_entries.push(LinearizedVirtualMember {
                        key: folded_member_key(kind, &member.name),
                        member,
                        owner: key.clone(),
                    });
                }
            }
        }

        // The adaptations of each trait-use clause, keyed by the folded
        // key of every trait the clause names, so a trait edge can pick
        // up the context to hand its members. Resolved at the using
        // site, reusing the same candidate order as the edges.
        let clause_context = resolve_clause_context(db, files, stubs, configuration, &found);
        // Named class-likes derive their inheritance edges from their
        // `Declaration`; a declaration-less anonymous class derives the
        // same edges from its member group's heritage projection
        // instead (`ClassMembers::extends`/`implements`, populated by
        // the same accessors `Declaration` reads).
        let edges = match found.declaration.as_ref() {
            Some(declaration) => edges_of(declaration),
            None => edges_of_group(&found.group),
        };
        for (relation, written) in edges {
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
                stub: answer.stub_key(),
                owner: key.clone(),
            });
            match answer {
                AncestorAnswer::Source { folded_key } => {
                    let next_origin = next_origin(&origin, relation, &key);
                    let next_adaptations = if matches!(next_origin, MemberOrigin::Trait { .. }) {
                        // The clause was written on `key`, the class-like
                        // dequeued here, so its adaptations are exactly
                        // this trait edge's — at any depth.
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

        // Decision 7: every enum implicitly implements `UnitEnum`, and
        // a backed one additionally `BackedEnum` — real ancestor facts
        // no PHP grammar lets a class-like write. Only when the
        // compiled stub graph actually answers the parent key: a stub
        // set carrying neither interface (a from-scratch fixture with
        // no engine stubs) synthesizes nothing, never a synthetic
        // opaque edge that would blanket-silence every enum.
        if found.group.kind == DeclarationKind::Enum {
            for (written, folded_key) in implicit_enum_edges(table) {
                ancestry.push(AncestorEdge {
                    relation: AncestorRelation::Implements,
                    written: written.to_owned(),
                    resolved: None,
                    stub: Some(folded_key.clone()),
                    owner: key.clone(),
                });
                stub_ancestors.push(folded_key);
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

    virtual_entries.sort_by(|left, right| {
        let rank = |member: &VirtualMember| match member.kind {
            VirtualMemberKind::Method => 0u8,
            VirtualMemberKind::Property => 1u8,
        };
        (rank(&left.member), &left.key).cmp(&(rank(&right.member), &right.key))
    });

    // An unresolved edge — neither a source nor a stub class-like — is a
    // genuinely opaque boundary from the start.
    let mut has_opaque_edge = ancestry
        .iter()
        .any(|edge| edge.resolved.is_none() && edge.stub.is_none());

    // Expand the stub frontier: breadth-first through the compiled parent
    // links, seeded by the direct stub edges in walk order (the shared
    // `stub_frontier` walk, which `celerrate_types`' iteration typing
    // reads too). A stub symbol without a compiled surface leaves the
    // boundary opaque; magic methods found on a stub ancestor mark the
    // class.
    let frontier = stub_frontier(table, stub_ancestors.iter().cloned());
    has_opaque_edge |= frontier.opaque;
    let mut stub_magic = MagicMarkers::default();
    for key in &frontier.reached {
        let Some(surface) = table.class(key) else {
            continue;
        };
        for member in &surface.members {
            merge_stub_magic(member, &mut stub_magic);
        }
    }
    let mut stub_ancestors = frontier.reached;
    stub_ancestors.sort();
    stub_ancestors.dedup();

    let mut magic = magic_markers(&members, allows_dynamic);
    magic.has_magic_get |= stub_magic.has_magic_get;
    magic.has_magic_set |= stub_magic.has_magic_set;
    magic.has_magic_call |= stub_magic.has_magic_call;
    magic.has_magic_call_static |= stub_magic.has_magic_call_static;

    Some(LinearizedClass {
        members,
        virtual_members: virtual_entries,
        ancestry,
        stub_ancestors,
        cyclic,
        has_opaque_edge,
        magic,
    })
}

/// The origin one ancestry edge hands its target, given the origin the
/// edge's owner was walked with and the folded key of that owner.
///
/// A trait-use edge crosses into a trait: its target's members are
/// `Trait`-origin, anchored to the class the trait was pasted into.
/// Walking from a class-like — `Own` (the queried class itself) or
/// `Inherited` (an ancestor reached by `extends`) — that class *is* the
/// user, so the edge's owner becomes the anchor. Walking from a trait
/// that itself uses a trait, the anchor is already fixed: PHP flattens
/// the whole trait chain into the same user, so it is carried forward
/// unchanged rather than re-taken at the intermediate trait.
///
/// Everything else inherits. That includes the shapes PHP cannot write —
/// a trait with an `extends` or `implements` edge — which drop the
/// anchor rather than invent one, the conservative direction.
fn next_origin(origin: &MemberOrigin, relation: AncestorRelation, owner: &str) -> MemberOrigin {
    match (origin, relation) {
        (MemberOrigin::Own | MemberOrigin::Inherited, AncestorRelation::UsesTrait) => {
            MemberOrigin::Trait {
                anchor: owner.to_owned(),
            }
        }
        (MemberOrigin::Trait { anchor }, AncestorRelation::UsesTrait) => MemberOrigin::Trait {
            anchor: anchor.clone(),
        },
        (
            MemberOrigin::Own | MemberOrigin::Inherited | MemberOrigin::Trait { .. },
            AncestorRelation::Extends | AncestorRelation::Implements,
        ) => MemberOrigin::Inherited,
    }
}

/// Folds a stub ancestor's magic method into the accumulator: `__get`,
/// `__set`, `__call`, `__callStatic` suppress otherwise-unknown members.
/// Method names fold ASCII case, matching source magic detection.
fn merge_stub_magic(member: &StubMember, magic: &mut MagicMarkers) {
    if member.kind != StubMemberKind::Method {
        return;
    }
    match member.name.to_ascii_lowercase().as_str() {
        "__get" => magic.has_magic_get = true,
        "__set" => magic.has_magic_set = true,
        "__call" => magic.has_magic_call = true,
        "__callstatic" => magic.has_magic_call_static = true,
        _ => {}
    }
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
/// when the member came from a trait. Plain own or inherited members
/// (and trait members with no adaptations) push once, verbatim. PHP's
/// `insteadof`/`as` adapt METHODS only: a property, class constant, or
/// enum case never consults the adaptation context, even when its name
/// coincides with an adapted method's.
///
/// The adaptations handed here are always the ones written on the very
/// clause this member's trait edge came from — `linearized_class`
/// resolves them against the class-like that declared the edge, so they
/// stay the using site's at any depth and never leak across a `use`.
fn push_member(
    member: &Member,
    owner: &str,
    origin: &MemberOrigin,
    adaptations: Option<&ResolvedAdaptations>,
    members: &mut Vec<LinearizedMember>,
) {
    let member_key = folded_member_key(member.kind, &member.name);
    let context = match (origin, adaptations) {
        (MemberOrigin::Trait { .. }, Some(context)) if member.kind == MemberKind::Method => context,
        _ => {
            members.push(LinearizedMember {
                key: member_key,
                member: member.clone(),
                owner: owner.to_owned(),
                origin: origin.clone(),
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
                    origin: origin.clone(),
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
        origin: origin.clone(),
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
                // NOTE: last-clause-wins per trait key. `use T { a as b; }
                // use T { c as d; }` keeps only this clause's adaptations
                // for T, though PHP merges adaptations across clauses.
                // Deterministic; revisit if the corpus proves it matters.
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

/// Decision 7's implicit enum parents: `\UnitEnum` always, `\BackedEnum`
/// additionally — real ancestor facts synthesized because the source
/// member projection carries no fact distinguishing a backed enum from
/// a plain one (an enum's backing type is not part of `ClassMembers`),
/// so both are synthesized uniformly for every source enum:
/// over-suppression is the conservative direction (decision 5), and
/// neither parent can ever declare a property (interfaces cannot), so
/// this can only ever grant methods, never fabricate a property
/// surface. Only the parents the compiled stub graph actually answers
/// are returned, each paired with its already-folded key — a stub set
/// naming neither interface contributes nothing.
fn implicit_enum_edges(table: &StubSignatureTable) -> Vec<(&'static str, String)> {
    [("\\UnitEnum", "UnitEnum"), ("\\BackedEnum", "BackedEnum")]
        .into_iter()
        .filter_map(|(written, bare)| {
            let folded_key = folded_symbol_key(SymbolSpace::ClassLike, bare);
            table
                .class(&folded_key)
                .is_some()
                .then_some((written, folded_key))
        })
        .collect()
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
/// non-class symbol). An anonymous synthetic key (`anonymous_class_key`)
/// loads its member group directly by `AstId`, bypassing the symbol
/// table entirely — an anonymous class declares no name to index.
fn fetch(db: &dyn salsa::Database, files: AnalyzedFileSet, key: &str) -> Option<Fetched> {
    if let Some(ast_id) = parse_anonymous_class_key(key) {
        let file = file_of(db, files, ast_id.file)?;
        let group = group_at(db, file, ast_id)?;
        let namespace = group.namespace.clone();
        return Some(Fetched {
            group,
            declaration: None,
            file,
            namespace,
        });
    }
    let query = SymbolQuery::new(db, SymbolSpace::ClassLike, key.to_owned());
    let (_, ast_id) = lookup_class_declaration(db, files, query)?;
    let file = file_of(db, files, ast_id.file)?;
    let group = group_at(db, file, ast_id)?;
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

/// The member group declared at `ast_id` in `file`, when the tree
/// still carries one there.
fn group_at(db: &dyn salsa::Database, file: SourceFile, ast_id: AstId) -> Option<ClassMembers> {
    member_tree(db, file)
        .classes
        .iter()
        .find(|group| group.ast_id == ast_id)
        .cloned()
}

/// The member group of one class-like by its folded key: the same
/// resolution `fetch` performs (an anonymous synthetic key loads
/// directly by `AstId`; a named key resolves through the symbol
/// table), stopping short of `fetch`'s declaration and namespace
/// lookups since a caller wanting only the group's `kind` has no use
/// for them. Shares `group_at`'s scan with `fetch` rather than
/// duplicating it. Crate-private: `celerrate_types`' receiver surface
/// (`class_kind`) wants only the `kind` field, not the whole group, so
/// it reads that narrower fact through [`class_declaration_kind`]
/// instead of this — decision 17 (no new public API beyond the
/// checks) draws the crate boundary at the narrowest fact a consumer
/// actually needs.
fn class_members_of(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    key: &str,
) -> Option<ClassMembers> {
    if let Some(ast_id) = parse_anonymous_class_key(key) {
        let file = file_of(db, files, ast_id.file)?;
        return group_at(db, file, ast_id);
    }
    let query = SymbolQuery::new(db, SymbolSpace::ClassLike, key.to_owned());
    let (_, ast_id) = lookup_class_declaration(db, files, query)?;
    let file = file_of(db, files, ast_id.file)?;
    group_at(db, file, ast_id)
}

/// The declaring group's `DeclarationKind` of one class-like by its
/// folded key, `None` when the key names no source class-like. The
/// narrow public fact `celerrate_types`' receiver surface needs for
/// enum detection (`class_kind`, `CEL0033`) — reads through
/// [`class_members_of`] rather than duplicating its lookup.
pub fn class_declaration_kind(
    db: &dyn salsa::Database,
    files: AnalyzedFileSet,
    key: &str,
) -> Option<DeclarationKind> {
    class_members_of(db, files, key).map(|group| group.kind)
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

/// The inheritance edges of a declaration-less (anonymous) class, from
/// the member group's heritage projection: traits first, then
/// `extends`, then `implements` — the same precedence as `edges_of`.
fn edges_of_group(group: &ClassMembers) -> Vec<(AncestorRelation, String)> {
    let mut edges = Vec::new();
    for trait_use in &group.trait_uses {
        for name in &trait_use.names {
            edges.push((AncestorRelation::UsesTrait, name.clone()));
        }
    }
    for name in &group.extends {
        edges.push((AncestorRelation::Extends, name.clone()));
    }
    for name in &group.implements {
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

    /// The folded key when the name resolved to a stub class-like;
    /// `None` for a source or unresolved answer.
    fn stub_key(&self) -> Option<String> {
        match self {
            AncestorAnswer::Stub { folded_key } => Some(folded_key.clone()),
            AncestorAnswer::Source { .. } | AncestorAnswer::Unresolved => None,
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
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{
        StubAvailability, StubClassSurface, StubIndex, StubIndexInput, StubMember, StubMemberKind,
        StubSignature, StubSymbol, StubSymbolKind, StubVisibility, VersionedTypeText,
    };

    use super::{
        ClassQuery, LinearizedClass, MemberOrigin, anonymous_class_key, class_declaration_kind,
        linearized_class, parse_anonymous_class_key,
    };
    use crate::ast_id::AstId;
    use crate::items::DeclarationKind;
    use crate::member_lookup::{MemberQuery, MemberResolution, lookup_member};
    use crate::members::MemberKind;
    use crate::plugin::PluginIdentity;
    use crate::symbols::{SymbolSpace, folded_symbol_key};
    use crate::virtual_symbols::{
        VirtualMember, VirtualMemberKind, VirtualSymbolProvider, VirtualSymbolRegistration,
        VirtualSymbolRegistry,
    };

    /// A provider that answers its fixed member set only when the
    /// docblock text carries `@fake`. Duplicated from Task 1's test
    /// module in `virtual_symbols.rs` — the crate has no shared
    /// test-support module yet, which is already recorded debt.
    #[derive(Debug)]
    struct FakeProvider {
        members: Vec<VirtualMember>,
    }

    impl VirtualSymbolProvider for FakeProvider {
        fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember> {
            if class_docblock.contains("@fake") {
                self.members.clone()
            } else {
                Vec::new()
            }
        }
    }

    fn identity(name: &str) -> PluginIdentity {
        PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
    }

    fn register_fake_provider(fixture: &Fixture, members: Vec<VirtualMember>) {
        let _ = VirtualSymbolRegistry::builder(vec![VirtualSymbolRegistration {
            identity: identity("fake"),
            provider: std::sync::Arc::new(FakeProvider { members }),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);
    }

    fn virtual_property(name: &str) -> VirtualMember {
        VirtualMember {
            kind: VirtualMemberKind::Property,
            name: name.to_owned(),
            is_static: false,
            type_text: Some("string".to_owned()),
            parameters: Vec::new(),
        }
    }

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

    /// A fixture whose stub payload carries real class surfaces. Every
    /// surface key and every parent it names becomes a `StubSymbol` too,
    /// so `resolve_ancestor` classifies the source edge as a stub edge;
    /// the default `Exception`/`strlen` symbols ride along for parity
    /// with the plain fixture.
    fn fixture_with_stub_classes(
        sources: &[&str],
        classes: Vec<(String, StubClassSurface)>,
    ) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs =
            StubIndexInput::builder(StubIndex::new(stub_symbols_for(&classes), vec![], classes))
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

    /// The class-space stub symbols a surface set implies: every surface
    /// key, every parent it names, and the default `Exception`/`strlen`
    /// symbols, deduplicated. `StubIndex::new` merges duplicates.
    fn stub_symbols_for(classes: &[(String, StubClassSurface)]) -> Vec<StubSymbol> {
        let mut names: Vec<String> = vec!["Exception".to_owned()];
        for (name, surface) in classes {
            names.push(name.clone());
            for parent in &surface.parents {
                names.push(parent.clone());
            }
        }
        names.sort();
        names.dedup();
        let mut symbols: Vec<StubSymbol> = names
            .into_iter()
            .map(|name| StubSymbol {
                name,
                kind: StubSymbolKind::Class,
                availability: StubAvailability::ALWAYS,
            })
            .collect();
        symbols.push(StubSymbol {
            name: "strlen".to_owned(),
            kind: StubSymbolKind::Function,
            availability: StubAvailability::ALWAYS,
        });
        symbols
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
            .map(|entry| (entry.owner.clone(), entry.origin.clone()))
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
            Some((
                "greets".to_owned(),
                MemberOrigin::Trait {
                    anchor: "child".to_owned(),
                },
            )),
        );
        assert_eq!(
            member_owner(&child, MemberKind::Method, "stays").unwrap().1,
            MemberOrigin::Inherited,
        );
    }

    /// The anchor of a trait member is the class that wrote `use`, seen
    /// from any query root: the direct user answers itself, and a
    /// subclass of it answers the user, not itself and not the trait.
    /// The `owner` stays the trait throughout — the anchor is a second,
    /// independent fact, not a rename of the first.
    #[test]
    fn a_trait_member_anchors_to_the_class_that_used_the_trait() {
        let fixture = fixture(&[
            "<?php trait Maker { public function make() {} }",
            "<?php class Factory { use Maker; }",
            "<?php class Sub extends Factory {}",
        ]);
        let factory = linearize(&fixture, "Factory").unwrap();
        assert_eq!(
            member_owner(&factory, MemberKind::Method, "make"),
            Some((
                "maker".to_owned(),
                MemberOrigin::Trait {
                    anchor: "factory".to_owned(),
                },
            )),
        );
        // One `extends` step out: still the trait's member, still
        // anchored to Factory — the class whose body PHP pasted it into.
        let sub = linearize(&fixture, "Sub").unwrap();
        assert_eq!(
            member_owner(&sub, MemberKind::Method, "make"),
            Some((
                "maker".to_owned(),
                MemberOrigin::Trait {
                    anchor: "factory".to_owned(),
                },
            )),
        );
    }

    /// A trait using a trait: the anchor is fixed at the class boundary
    /// and carried forward across every further trait-use step, so the
    /// innermost trait's members anchor to the class, never to the
    /// intermediate trait.
    #[test]
    fn a_nested_trait_member_keeps_the_using_class_as_its_anchor() {
        let fixture = fixture(&[
            "<?php trait Inner { public function make() {} }",
            "<?php trait Outer { use Inner; }",
            "<?php class C { use Outer; }",
            "<?php class D extends C {}",
        ]);
        for (root, expected) in [("C", "c"), ("D", "c")] {
            let table = linearize(&fixture, root).unwrap();
            assert_eq!(
                member_owner(&table, MemberKind::Method, "make"),
                Some((
                    "inner".to_owned(),
                    MemberOrigin::Trait {
                        anchor: expected.to_owned(),
                    },
                )),
                "root {root}",
            );
        }
    }

    /// An adaptation is written at the using site, so a trait-use clause
    /// on a class reached through `extends` still adapts: `Sub` inherits
    /// `Factory`'s adapted table, not `Maker`'s raw one. Before the
    /// anchor, `(Inherited, UsesTrait)` fell into the catch-all and
    /// dropped the clause context entirely, so `build` did not exist on
    /// `Sub` at all.
    #[test]
    fn a_using_classes_adaptations_survive_one_extends_step_out() {
        let fixture = fixture(&[
            "<?php trait Maker { public function make() {} }",
            "<?php class Factory { use Maker { make as build; } }",
            "<?php class Sub extends Factory {}",
        ]);
        let sub = linearize(&fixture, "Sub").unwrap();
        assert_eq!(
            member_owner(&sub, MemberKind::Method, "build"),
            Some((
                "maker".to_owned(),
                MemberOrigin::Trait {
                    anchor: "factory".to_owned(),
                },
            )),
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
        // The symbol exists but the payload carries no surface for it:
        // the boundary is opaque.
        assert!(class.has_opaque_edge);
    }

    #[test]
    fn stub_ancestry_walks_transitively_through_the_blob() {
        let fixture = fixture_with_stub_classes(
            &["<?php class MyError extends RuntimeException {}"],
            vec![
                (
                    "RuntimeException".to_owned(),
                    StubClassSurface {
                        parents: vec!["Exception".to_owned()],
                        members: vec![],
                    },
                ),
                (
                    "Exception".to_owned(),
                    StubClassSurface {
                        parents: vec!["Throwable".to_owned()],
                        members: vec![],
                    },
                ),
                ("Throwable".to_owned(), StubClassSurface::default()),
            ],
        );
        let table = linearize(&fixture, "MyError").unwrap();
        assert_eq!(
            table.stub_ancestors,
            vec![
                "exception".to_owned(),
                "runtimeexception".to_owned(),
                "throwable".to_owned(),
            ],
        );
        assert!(!table.has_opaque_edge, "fully walked");
        assert_eq!(table.ancestry[0].stub.as_deref(), Some("runtimeexception"));
    }

    #[test]
    fn a_missing_stub_surface_is_an_opaque_edge() {
        // The symbol exists but the payload carries no surface for it
        // (the pre-plan-3 fixtures everywhere): the boundary is recorded.
        let fixture = fixture_one("<?php class MyException extends Exception {}");
        let table = linearize(&fixture, "MyException").unwrap();
        assert_eq!(table.stub_ancestors, vec!["exception".to_owned()]);
        assert!(table.has_opaque_edge);
    }

    #[test]
    fn magic_methods_on_a_stub_ancestor_mark_the_class() {
        let fixture = fixture_with_stub_classes(
            &["<?php class Wrapper extends MagicBase {}"],
            vec![(
                "MagicBase".to_owned(),
                StubClassSurface {
                    parents: vec![],
                    members: vec![StubMember {
                        kind: StubMemberKind::Method,
                        name: "__call".to_owned(),
                        visibility: StubVisibility::Public,
                        is_static: false,
                        availability: StubAvailability::ALWAYS,
                        signature: Some(StubSignature::default()),
                        type_text: VersionedTypeText::default(),
                        value_text: None,
                    }],
                },
            )],
        );
        let table = linearize(&fixture, "Wrapper").unwrap();
        assert!(table.magic.has_magic_call);
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
            Some((
                "b".to_owned(),
                MemberOrigin::Trait {
                    anchor: "a".to_owned(),
                },
            )),
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
    fn adaptations_apply_to_methods_only_never_properties() {
        // `insteadof`/`as` adapt METHODS only. Trait C also declares a
        // property sharing the adapted name; it must survive verbatim,
        // untouched by the method-only `insteadof` and `as`, while the
        // method adaptation still applies.
        let fixture = fixture(&[
            "<?php trait B { public function hello() { return 'b'; } }",
            "<?php trait C { public $hello; public function hello() { return 'c'; } }",
            "<?php class A { use B, C { B::hello insteadof C; C::hello as hi; } }",
        ]);
        let a = linearize(&fixture, "A").unwrap();
        // The property is not excluded: it survives under its own
        // verbatim key, owned by C.
        assert_eq!(
            member_owner(&a, MemberKind::Property, "hello"),
            Some((
                "c".to_owned(),
                MemberOrigin::Trait {
                    anchor: "a".to_owned(),
                },
            )),
        );
        // The method exclusion still applies: B wins.
        assert_eq!(
            member_owner(&a, MemberKind::Method, "hello"),
            Some((
                "b".to_owned(),
                MemberOrigin::Trait {
                    anchor: "a".to_owned(),
                },
            )),
        );
        // The method alias still applies.
        assert!(
            a.members
                .iter()
                .any(|entry| entry.key == "hi" && entry.member.kind == MemberKind::Method)
        );
        // No phantom aliased property entry exists: `as` never applies to
        // a property.
        assert!(
            !a.members
                .iter()
                .any(|entry| entry.key == "hi" && entry.member.kind == MemberKind::Property)
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

    #[test]
    fn a_class_docblock_contributes_virtual_members() {
        let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
        register_fake_provider(&fixture, vec![virtual_property("title")]);
        let linearized = linearize(&fixture, "Post").unwrap();
        assert_eq!(linearized.virtual_members.len(), 1);
        assert_eq!(linearized.virtual_members[0].member.name, "title");
        assert_eq!(linearized.virtual_members[0].owner, "post");
    }

    #[test]
    fn virtual_members_inherit_and_the_nearest_declaration_wins() {
        let fixture = fixture(&[
            "<?php /** @fake */ class Base {}",
            "<?php /** @fake */ class Child extends Base {}",
        ]);
        register_fake_provider(&fixture, vec![virtual_property("title")]);
        let linearized = linearize(&fixture, "Child").unwrap();
        // Both declarations arrive; the walk order puts the child first.
        assert_eq!(linearized.virtual_members.len(), 2);
        assert_eq!(linearized.virtual_members[0].owner, "child");
        assert_eq!(linearized.virtual_members[1].owner, "base");
    }

    #[test]
    fn a_class_without_docblock_contributes_nothing_and_no_registry_means_no_virtual_members() {
        let fixture = fixture(&["<?php class Plain {}"]);
        // No registry set at all: the no-plugin path.
        let linearized = linearize(&fixture, "Plain").unwrap();
        assert!(linearized.virtual_members.is_empty());
    }

    #[test]
    fn providers_are_consulted_in_registered_order() {
        let fixture = fixture(&["<?php /** @fake */ class Post {}"]);
        let _ = VirtualSymbolRegistry::builder(vec![
            VirtualSymbolRegistration {
                identity: identity("first"),
                provider: std::sync::Arc::new(FakeProvider {
                    members: vec![virtual_property("alpha")],
                }),
            },
            VirtualSymbolRegistration {
                identity: identity("second"),
                provider: std::sync::Arc::new(FakeProvider {
                    members: vec![virtual_property("alpha"), virtual_property("beta")],
                }),
            },
        ])
        .durability(salsa::Durability::HIGH)
        .new(&fixture.db);

        let linearized = linearize(&fixture, "Post").unwrap();
        let keys: Vec<(&str, &str)> = linearized
            .virtual_members
            .iter()
            .map(|entry| (entry.key.as_str(), entry.owner.as_str()))
            .collect();
        // Stable sort by (kind, key): both `alpha` entries stay in
        // registered order (first provider's first), `beta` follows.
        assert_eq!(
            keys,
            vec![("alpha", "post"), ("alpha", "post"), ("beta", "post")]
        );
    }

    #[test]
    fn a_source_enum_gains_the_implicit_unitenum_and_backedenum_edges() {
        // Decision 7: the compiled stub graph knows both engine
        // interfaces, so a source enum's linearization gains resolved
        // edges to each and the boundary stays fully walked.
        let fixture = fixture_with_stub_classes(
            &["<?php enum Status: string { case Active = 'active'; }"],
            vec![
                ("UnitEnum".to_owned(), StubClassSurface::default()),
                (
                    "BackedEnum".to_owned(),
                    StubClassSurface {
                        parents: vec!["UnitEnum".to_owned()],
                        members: vec![],
                    },
                ),
            ],
        );
        let status = linearize(&fixture, "Status").unwrap();
        assert!(!status.has_opaque_edge, "the stub graph knows both parents");
        assert!(!status.cyclic);
        assert_eq!(
            status.stub_ancestors,
            vec!["backedenum".to_owned(), "unitenum".to_owned()],
        );
        assert!(
            status.ancestry.iter().any(
                |edge| edge.written == "\\UnitEnum" && edge.stub.as_deref() == Some("unitenum")
            )
        );
        assert!(status.ancestry.iter().any(
            |edge| edge.written == "\\BackedEnum" && edge.stub.as_deref() == Some("backedenum")
        ));
    }

    #[test]
    fn a_source_enum_with_no_compiled_enum_interfaces_gains_no_synthetic_edge() {
        // The default fixture's stub set carries no `UnitEnum`: nothing
        // is synthesized, and — crucially — no synthetic OPAQUE edge
        // either, which would otherwise blanket-silence every enum in
        // a stub-less fixture.
        let fixture = fixture_one("<?php enum Status { case Active; }");
        let status = linearize(&fixture, "Status").unwrap();
        assert!(status.ancestry.is_empty());
        assert!(!status.has_opaque_edge);
        assert!(status.stub_ancestors.is_empty());
    }

    #[test]
    fn class_declaration_kind_answers_the_declaring_groups_kind() {
        let fixture = fixture(&["<?php class Plain {} enum Status { case Active; }"]);
        assert_eq!(
            class_declaration_kind(&fixture.db, fixture.files, "plain"),
            Some(DeclarationKind::Class),
        );
        assert_eq!(
            class_declaration_kind(&fixture.db, fixture.files, "status"),
            Some(DeclarationKind::Enum),
        );
        assert_eq!(
            class_declaration_kind(&fixture.db, fixture.files, "ghost"),
            None,
        );
    }

    #[test]
    fn the_anonymous_key_round_trips_and_never_collides() {
        let ast_id = AstId {
            file: FileId::new(3),
            index: 7,
        };
        let key = anonymous_class_key(ast_id);
        assert_eq!(key, "class@anonymous:3:7");
        assert_eq!(parse_anonymous_class_key(&key), Some(ast_id));
        // Real folded keys never parse: the prefix is not a PHP name.
        assert_eq!(parse_anonymous_class_key("app\\kernel"), None);
        assert_eq!(parse_anonymous_class_key("class@anonymous:x:y"), None);
    }

    #[test]
    fn an_anonymous_class_linearizes_by_its_synthetic_key() {
        let fixture = fixture(&[r#"<?php
function build(): void {
    $listener = new class {
        public function handle(): int { return 1; }
    };
}
"#]);
        // Numbering: function = 0, anonymous class = 1, method = 2.
        let key = anonymous_class_key(AstId {
            file: FileId::new(0),
            index: 1,
        });
        let linearized = linearized_class(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            ClassQuery::new(&fixture.db, key),
        )
        .as_ref()
        .unwrap();
        assert!(
            linearized
                .members
                .iter()
                .any(|member| member.key == "handle")
        );
    }

    #[test]
    fn an_anonymous_class_inherits_through_its_heritage() {
        let fixture = fixture(&[r#"<?php
class Base { public function inherited(): int { return 1; } }
function build(): void {
    $listener = new class extends Base {};
}
"#]);
        // Numbering: Base = 0, its method = 1, build = 2, anonymous = 3.
        let key = anonymous_class_key(AstId {
            file: FileId::new(0),
            index: 3,
        });
        let resolution = lookup_member(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            MemberQuery::new(&fixture.db, key, MemberKind::Method, "inherited".to_owned()),
        );
        assert!(matches!(
            resolution,
            Some(MemberResolution::Source {
                origin: MemberOrigin::Inherited,
                ..
            })
        ));
    }
}
