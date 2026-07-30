//! The shared receiver surface: decompose a
//! receiver into atoms, resolve placeholders against the owner, judge
//! member existence ternarily. `PossiblyExists` is always silence —
//! the guillotine's currency. The union rule (missing on all non-null
//! constituents) and the intersection dual (exists on any
//! intersectand, suppressed by any) coincide in the reduction "missing
//! iff every part is missing", which is why flattening both into one
//! atom list is sound.
//!
use celerrate_semantics::{
    AncestorRelation, ClassQuery, ClassSurface, DeclarationKind, MemberKind, MemberQuery,
    SymbolSpace, class_declaration_kind, class_surface, folded_member_key, linearized_class,
    lookup_member, source_symbol_table, stub_symbol_table,
};
use celerrate_stubs::StubSymbolKind;

use super::CheckContext;
use crate::TypeId;
use crate::inference::BodyOwner;
use crate::representation::TypeData;

/// The ternary member-existence judgment. `PossiblyExists` is always
/// silence: an undecidable receiver never becomes a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberExistence {
    Exists,
    PossiblyExists,
    Missing,
}

/// One atomic constituent of a decomposed receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReceiverAtom {
    Class { key: String },
    Case { enum_key: String },
    Null,
    Undecidable,
}

/// Decomposes a receiver into atoms. Placeholders resolve against the
/// owner (`self`/`static` -> the owner class, `parent` -> its first
/// `Extends` ancestor); unions and intersections flatten; every silent
/// form (`mixed`, `object`, an unresolvable class name, a
/// template, `class-string`, a scalar, an array, a callable) lands on
/// `Undecidable`. Debt ledger: scalar, array, and callable receivers
/// are silent here — a "call on non-object" family is future work.
/// Debt ledger: template receivers are silent too; through-bound
/// reporting (resolving a call against a template's bound rather than
/// treating it as undecidable) is the stated `CannotProve` posture,
/// not a gap this walk closes.
pub(crate) fn atoms_of<'db>(
    context: &CheckContext<'db, '_>,
    receiver: TypeId<'db>,
) -> Vec<ReceiverAtom> {
    let db = context.db;
    if receiver.is_null(db) {
        return vec![ReceiverAtom::Null];
    }
    let constituents = receiver.constituents(db);
    if constituents.len() > 1 {
        return constituents
            .into_iter()
            .flat_map(|part| atoms_of(context, part))
            .collect();
    }
    let intersectands = receiver.intersectands(db);
    if intersectands.len() > 1 {
        return intersectands
            .into_iter()
            .flat_map(|part| atoms_of(context, part))
            .collect();
    }
    if let Some((enum_name, _)) = receiver.enum_case_parts(db) {
        return vec![ReceiverAtom::Case {
            enum_key: enum_name,
        }];
    }
    if receiver.is_mixed(db) {
        return vec![ReceiverAtom::Undecidable];
    }
    if let Some(resolved) = resolve_placeholder(context, receiver) {
        return resolved;
    }
    match receiver.class_name(db) {
        // `object` has no key; `class_name` answers `None` for it —
        // like every scalar, array, callable, template, and
        // class-string: all `Undecidable` by falling through.
        Some(key) => vec![ReceiverAtom::Class { key }],
        None => vec![ReceiverAtom::Undecidable],
    }
}

/// `self`/`static` -> the owner class key; `parent` -> the owner's
/// first `Extends` ancestor; `None` when the receiver is no
/// placeholder. An owner-less body (a free function) or an
/// unresolvable parent answers `Undecidable` rather than `None`, so
/// the placeholder is still consumed here and never falls through to
/// `class_name` (which would answer `None` for it regardless).
fn resolve_placeholder<'db>(
    context: &CheckContext<'db, '_>,
    receiver: TypeId<'db>,
) -> Option<Vec<ReceiverAtom>> {
    match receiver.data(context.db) {
        TypeData::SelfPlaceholder | TypeData::StaticPlaceholder => {
            Some(vec![match owner_class_key(context) {
                Some(key) => ReceiverAtom::Class { key },
                None => ReceiverAtom::Undecidable,
            }])
        }
        TypeData::ParentPlaceholder => {
            let parent = owner_class_key(context).and_then(|key| parent_key(context, &key));
            Some(vec![match parent {
                Some(key) => ReceiverAtom::Class { key },
                None => ReceiverAtom::Undecidable,
            }])
        }
        _ => None,
    }
}

/// The body's owner class key, when the body is a method of a
/// resolvable class-like (`None` for a free function, or a method
/// whose owner itself failed to resolve). Shared by the placeholder
/// resolution above and by `members.rs`'s scoped-subject folding
/// (`self`/`static` in `Foo::m()` resolve to this same owner).
pub(crate) fn owner_class_key(context: &CheckContext<'_, '_>) -> Option<String> {
    match context.owner {
        Some(BodyOwner::Method {
            class_key: Some(key),
            ..
        }) => Some(key.clone()),
        _ => None,
    }
}

/// The owner's first resolved `Extends` ancestor, if any — resolved
/// whether that ancestor is a source or a compiled stub class-like, so
/// `parent::` still answers when the parent is itself a stub (the
/// traits-behind-`extends` blind spot applies here too: a subclass
/// asking through `parent` gets its own direct parent, never the
/// asker's).
pub(crate) fn parent_key(context: &CheckContext<'_, '_>, class_key: &str) -> Option<String> {
    let linearized = linearized_class(
        context.db,
        context.files,
        context.stubs,
        context.configuration,
        ClassQuery::new(context.db, class_key.to_owned()),
    )
    .as_ref()?;
    linearized
        .ancestry
        .iter()
        .find(|edge| edge.relation == AncestorRelation::Extends)
        .and_then(|edge| edge.resolved.clone().or_else(|| edge.stub.clone()))
}

/// The ternary judgment of one atomic class or enum-case constituent.
/// `pub(crate)`: scoped subjects are already folded class keys
/// (`members.rs`'s `scoped_subject_keys`), so scoped checks call this
/// directly with no atom decomposition needed.
pub(crate) fn atom_existence(
    context: &CheckContext<'_, '_>,
    key: &str,
    kind: MemberKind,
    member_name: &str,
    scoped: bool,
) -> MemberExistence {
    let db = context.db;
    // The choke point every `member_existence` atom
    // and every scoped-member check (`members.rs`'s direct
    // `atom_existence` calls) funnels through — recorded here rather
    // than at each caller so both paths share one recording site.
    context.dependencies.borrow_mut().insert(key.to_owned());
    let lookup = |kind: MemberKind, name: &str| {
        lookup_member(
            db,
            context.files,
            context.stubs,
            context.configuration,
            MemberQuery::new(db, key.to_owned(), kind, folded_member_key(kind, name)),
        )
    };
    if lookup(kind, member_name).is_some() {
        return MemberExistence::Exists;
    }
    // The engine-provided enum instance properties:
    // interfaces cannot declare properties, so no stub can ever carry
    // `name`/`value` — they exist on every enum by fiat.
    if kind == MemberKind::Property
        && (member_name == "name" || member_name == "value")
        && is_enum_key(context, key)
    {
        return MemberExistence::Exists;
    }
    // The surface decides whether absence is provable at all.
    match class_surface(db, context.files, context.stubs, context.configuration, key) {
        ClassSurface::Unknown => return MemberExistence::PossiblyExists,
        ClassSurface::Source => {
            let linearized = linearized_class(
                db,
                context.files,
                context.stubs,
                context.configuration,
                ClassQuery::new(db, key.to_owned()),
            );
            let Some(linearized) = linearized.as_ref() else {
                return MemberExistence::PossiblyExists;
            };
            if linearized.has_opaque_edge || linearized.cyclic {
                return MemberExistence::PossiblyExists;
            }
            if kind == MemberKind::Property
                && (linearized.magic.allows_dynamic_properties
                    || linearized
                        .stub_ancestors
                        .iter()
                        .any(|ancestor| ancestor == "stdclass"))
            {
                return MemberExistence::PossiblyExists;
            }
        }
        ClassSurface::Stub => {}
    }
    // stdClass itself (not merely inherited) always accepts dynamic
    // properties.
    if kind == MemberKind::Property && key == "stdclass" {
        return MemberExistence::PossiblyExists;
    }
    // Magic, per kind, uniformly through `lookup_member` (which walks
    // source linearization and the stub graph alike). Scoped calls
    // consult both call interceptors: `self::m()` may target an
    // instance method — over-suppression is the conservative side.
    let magic_names: &[&str] = match kind {
        MemberKind::Method if scoped => &["__call", "__callstatic"],
        MemberKind::Method => &["__call"],
        MemberKind::Property => &["__get", "__set"],
        MemberKind::ClassConstant | MemberKind::EnumCase => &[],
    };
    for magic in magic_names {
        if lookup(MemberKind::Method, magic).is_some() {
            return MemberExistence::PossiblyExists;
        }
    }
    MemberExistence::Missing
}

/// The composed judgment: `Missing` iff every non-null atom is
/// `Missing`; any `Exists` wins immediately; a receiver with no
/// decidable atom (mixed, only-null, scalars…) is `PossiblyExists`.
/// Debt ledger: a union receiver missing on SOME constituents and
/// existing on others also composes to `PossiblyExists` (the union
/// rule only reports `Missing` when every constituent agrees) — a
/// future "possibly undefined member" diagnostic for that narrower
/// case is not built here.
pub(crate) fn member_existence<'db>(
    context: &CheckContext<'db, '_>,
    receiver: TypeId<'db>,
    kind: MemberKind,
    member_name: &str,
    scoped: bool,
) -> MemberExistence {
    let mut considered = 0usize;
    let mut all_missing = true;
    for atom in atoms_of(context, receiver) {
        let verdict = match &atom {
            ReceiverAtom::Null => continue,
            ReceiverAtom::Undecidable => MemberExistence::PossiblyExists,
            ReceiverAtom::Class { key } => atom_existence(context, key, kind, member_name, scoped),
            ReceiverAtom::Case { enum_key } => {
                atom_existence(context, enum_key, kind, member_name, scoped)
            }
        };
        if verdict == MemberExistence::Exists {
            return MemberExistence::Exists;
        }
        considered += 1;
        all_missing &= verdict == MemberExistence::Missing;
    }
    if considered > 0 && all_missing {
        MemberExistence::Missing
    } else {
        MemberExistence::PossiblyExists
    }
}

/// The message spelling of a receiver: the non-null part it was looked
/// up on (messages name the part the member was looked up on, not the
/// nullable whole — the nullability family's own beat).
pub(crate) fn receiver_display<'db>(
    context: &CheckContext<'db, '_>,
    receiver: TypeId<'db>,
) -> String {
    let stripped = receiver.without_null(context.db);
    if stripped.is_never(context.db) {
        written_type_display(context, receiver)
    } else {
        written_type_display(context, stripped)
    }
}

/// `TypeId::display` with written spellings: class and
/// enum names map through the symbol index — the source table's
/// `original`, then the stub table's `StubSymbol.name` — and fall back
/// to the folded key when nothing answers. Anonymous keys keep the
/// coordinate-stripped `class@anonymous` rendering.
pub(crate) fn written_type_display<'db>(
    context: &CheckContext<'db, '_>,
    of: TypeId<'db>,
) -> String {
    let resolve = |key: &str| -> Option<String> {
        if key.starts_with("class@anonymous:") {
            return None; // display.rs's stripping rule applies unconditionally.
        }
        if let Some(entry) =
            source_symbol_table(context.db, context.files).lookup(SymbolSpace::ClassLike, key)
        {
            return Some(entry.original.clone());
        }
        stub_symbol_table(context.db, context.stubs, context.configuration)
            .lookup(SymbolSpace::ClassLike, key)
            .map(|entry| entry.symbol.name.clone())
    };
    of.display_with_names(context.db, &resolve)
}

/// The declaring group's kind, source declarations only — `None` for a
/// class-like with no source declaration (a stub-only class or enum).
/// `CEL0033`'s enum-kind classification does not call this directly: a
/// stub-only enum (a PHP built-in, never written in source) would
/// otherwise answer `None` and be mislabeled a class constant, so that
/// call site goes through `is_enum_key` instead, which also consults the
/// stub symbol table. `class_kind` stays as is for callers that only
/// need the source group's kind.
pub(crate) fn class_kind(context: &CheckContext<'_, '_>, key: &str) -> Option<DeclarationKind> {
    class_declaration_kind(context.db, context.files, key)
}

/// Whether `key` names an enum: `class_kind` answers `Enum` for source
/// keys; a stub key answers through the stub symbol table's own
/// `StubSymbolKind::Enum` (the `name`/`value` engine-property rule
/// needs both sides).
pub(crate) fn is_enum_key(context: &CheckContext<'_, '_>, key: &str) -> bool {
    if class_kind(context, key) == Some(DeclarationKind::Enum) {
        return true;
    }
    stub_symbol_table(context.db, context.stubs, context.configuration)
        .lookup(SymbolSpace::ClassLike, key)
        .is_some_and(|entry| entry.symbol.kind == StubSymbolKind::Enum)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_semantics::MemberKind;

    use super::{MemberExistence, member_existence, written_type_display};
    use crate::TypeId;
    use crate::checks::test_support::{context_for, fixture, fixture_with_stub_enum_interfaces};

    const SURFACE_SOURCES: &str = r#"<?php
class Plain { public function known(): int { return 1; } }
class Magic { public function __call(string $n, array $a): mixed {} }
class Getter { public function __get(string $n): mixed {} }
#[AllowDynamicProperties]
class Bag {}
class Opaque extends GhostBase {}
enum Status { case Active; }
function scene(Plain $p, Magic $m, Getter $g, Bag $b, Opaque $o, Status $s): void {}
"#;

    #[test]
    fn the_ternary_existence_judgment() {
        let fixture = fixture(&[SURFACE_SOURCES]);
        let context = context_for(&fixture, /* scene's body */ 10);
        let plain = TypeId::class(&fixture.db, "plain", vec![]);
        let magic = TypeId::class(&fixture.db, "magic", vec![]);
        let getter = TypeId::class(&fixture.db, "getter", vec![]);
        let bag = TypeId::class(&fixture.db, "bag", vec![]);
        let opaque = TypeId::class(&fixture.db, "opaque", vec![]);
        let ghost = TypeId::class(&fixture.db, "app\\ghost", vec![]);
        let mixed = TypeId::mixed(&fixture.db);
        let judge =
            |receiver, kind, name: &str| member_existence(&context, receiver, kind, name, false);
        use MemberExistence::*;
        use MemberKind::*;
        // A resolvable member exists; a missing one on a closed source
        // surface is provably missing.
        assert!(matches!(judge(plain, Method, "known"), Exists));
        assert!(matches!(judge(plain, Method, "ghost"), Missing));
        assert!(matches!(judge(plain, Property, "ghost"), Missing));
        // Magic suppression is per kind: __call silences methods only.
        assert!(matches!(judge(magic, Method, "anything"), PossiblyExists));
        assert!(matches!(judge(magic, Property, "anything"), Missing));
        // __get silences properties only.
        assert!(matches!(
            judge(getter, Property, "anything"),
            PossiblyExists
        ));
        assert!(matches!(judge(getter, Method, "anything"), Missing));
        // #[AllowDynamicProperties] silences properties only.
        assert!(matches!(judge(bag, Property, "anything"), PossiblyExists));
        assert!(matches!(judge(bag, Method, "anything"), Missing));
        // An opaque inheritance edge silences every kind.
        assert!(matches!(judge(opaque, Method, "anything"), PossiblyExists));
        // Undecidable atoms are silence.
        assert!(matches!(judge(mixed, Method, "anything"), PossiblyExists));
        assert!(matches!(judge(ghost, Method, "anything"), PossiblyExists));
    }

    #[test]
    fn unions_and_intersections_reduce_over_their_parts() {
        let fixture = fixture(&[SURFACE_SOURCES]);
        let context = context_for(&fixture, 10);
        let db = &fixture.db;
        let plain = TypeId::class(db, "plain", vec![]);
        let magic = TypeId::class(db, "magic", vec![]);
        let judge = |receiver, name: &str| {
            member_existence(&context, receiver, MemberKind::Method, name, false)
        };
        use MemberExistence::*;
        // Union: report only if missing on every non-null constituent.
        let with_null = TypeId::union(db, [plain, TypeId::null(db)]);
        assert!(matches!(judge(with_null, "ghost"), Missing));
        assert!(matches!(judge(with_null, "known"), Exists));
        let with_magic = TypeId::union(db, [plain, magic]);
        assert!(matches!(judge(with_magic, "ghost"), PossiblyExists));
        // Intersection: exists if any intersectand has it, suppressed
        // if any suppresses — the dual, same reduction.
        let narrowed = TypeId::intersection(db, [plain, magic]);
        assert!(matches!(judge(narrowed, "known"), Exists));
        assert!(matches!(judge(narrowed, "anything"), PossiblyExists));
        // A receiver that is only null: the nullability family's beat.
        assert!(matches!(
            judge(TypeId::null(db), "anything"),
            PossiblyExists
        ));
    }

    #[test]
    fn placeholders_resolve_against_the_owner() {
        let fixture = fixture(&[r#"<?php
class Base { public function up(): int { return 1; } }
class Child extends Base {
    public function probe(): void {}
}
"#]);
        let context = context_for(&fixture, /* probe's body */ 3);
        let db = &fixture.db;
        use MemberExistence::*;
        let judge = |receiver, name: &str| {
            member_existence(&context, receiver, MemberKind::Method, name, false)
        };
        assert!(matches!(
            judge(TypeId::static_placeholder(db), "probe"),
            Exists
        ));
        assert!(matches!(
            judge(TypeId::static_placeholder(db), "up"),
            Exists
        ));
        assert!(matches!(
            judge(TypeId::static_placeholder(db), "ghost"),
            Missing
        ));
        assert!(matches!(
            judge(TypeId::parent_placeholder(db), "up"),
            Exists
        ));
        assert!(matches!(
            judge(TypeId::self_placeholder(db), "ghost"),
            Missing
        ));
    }

    #[test]
    fn the_implicit_enum_surface_counts_as_existing() {
        let fixture = fixture_with_stub_enum_interfaces(&[r#"<?php
enum Status: string { case Active = 'active'; }
function scene(Status $s): void {}
"#]);
        let context = context_for(&fixture, /* scene's body */ 2);
        let status = TypeId::class(&fixture.db, "status", vec![]);
        use MemberExistence::*;
        // The implicit methods resolve through the synthesized edges.
        assert!(matches!(
            member_existence(&context, status, MemberKind::Method, "cases", true),
            Exists
        ));
        assert!(matches!(
            member_existence(&context, status, MemberKind::Method, "from", true),
            Exists
        ));
        // The engine-provided instance properties always exist on enums.
        assert!(matches!(
            member_existence(&context, status, MemberKind::Property, "value", false),
            Exists
        ));
        assert!(matches!(
            member_existence(&context, status, MemberKind::Property, "name", false),
            Exists
        ));
        // The surface stays closed: a genuine ghost still proves missing.
        assert!(matches!(
            member_existence(&context, status, MemberKind::Method, "ghost", true),
            Missing
        ));
    }

    #[test]
    fn message_displays_recover_written_spellings() {
        let fixture = fixture(&[r#"<?php
namespace App;
class User {}
function scene(): void {}
"#]);
        // Numbering: the statement-form `namespace App;` itself is
        // numbered (index 0), then `class User` (1), then `scene` (2).
        let context = context_for(&fixture, /* scene's body */ 2);
        let db = &fixture.db;
        let user = TypeId::class(db, "app\\user", vec![]);
        assert_eq!(written_type_display(&context, user), "App\\User");
        let with_null = TypeId::union(db, [user, TypeId::null(db)]);
        assert_eq!(written_type_display(&context, with_null), "App\\User|null");
        // Nothing answers the key: the folded key is the fallback.
        let ghost = TypeId::class(db, "app\\ghost", vec![]);
        assert_eq!(written_type_display(&context, ghost), "app\\ghost");
        // Anonymous keys keep their coordinate-stripped rendering.
        let anonymous = TypeId::class(db, "class@anonymous:0:1", vec![]);
        assert_eq!(written_type_display(&context, anonymous), "class@anonymous");
    }
}
