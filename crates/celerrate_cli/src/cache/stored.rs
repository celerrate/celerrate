//! The serialized forms of the cached artifacts. Mirror types rather
//! than derives on the domain types, because the conversion is the
//! schema: a `FileId` is process-local and must be stamped back in at
//! load, and a `DiagnosticId` wraps a `'static` string that must be
//! re-interned through the registry. Every `to_*` conversion is total
//! except identifier re-interning, whose failure discards the entry.
//!
//! **The suppression note (plan 9a, task 9).** `StoredVerdict.diagnostics`
//! and `StoredTypedVerdict.diagnostics` are both stored POST-suppression
//! (schema 4's convention, unchanged): every persisted diagnostic has
//! already survived `celerrate_semantics::suppressed_ranges`'s filter.
//! Suppression directives are strictly file-local facts read from the
//! same file the verdict's content-hash key covers, so editing even a
//! comment — never mind the directive itself — moves the hash and
//! discards the WHOLE entry, untyped and typed halves alike (`stale
//! suppression is structurally impossible`, `cache_suppression.rs`'s own
//! module doc). A stale suppression decision can therefore never survive
//! into a served verdict, typed or not.

use celerrate_diagnostics::{Diagnostic, Severity, find_identifier};
use celerrate_project::PhpVersion;
use celerrate_semantics::{
    AstId, ClassMembers, Declaration, DeclarationKind, FreeFunction, ImportKind, ItemTree, Member,
    MemberFlags, MemberKind, MemberSignature, MemberTree, ParameterSignature, ResolutionAnswer,
    ResolutionRecord, SymbolSpace, TraitAdaptation, TraitUse, UseImport, Visibility,
};
use celerrate_source::{FileId, TextRange, TextSize};
use celerrate_types::{StoredClassDependency, StoredFunctionDependency, StoredInferredEdge};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredDeclarationKind {
    Class,
    Interface,
    Trait,
    Enum,
    Function,
    Constant,
}

impl StoredDeclarationKind {
    fn of(kind: DeclarationKind) -> Self {
        match kind {
            DeclarationKind::Class => Self::Class,
            DeclarationKind::Interface => Self::Interface,
            DeclarationKind::Trait => Self::Trait,
            DeclarationKind::Enum => Self::Enum,
            DeclarationKind::Function => Self::Function,
            DeclarationKind::Constant => Self::Constant,
        }
    }

    fn to_kind(self) -> DeclarationKind {
        match self {
            Self::Class => DeclarationKind::Class,
            Self::Interface => DeclarationKind::Interface,
            Self::Trait => DeclarationKind::Trait,
            Self::Enum => DeclarationKind::Enum,
            Self::Function => DeclarationKind::Function,
            Self::Constant => DeclarationKind::Constant,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDeclaration {
    kind: StoredDeclarationKind,
    name: String,
    namespace: String,
    ast_index: u32,
    extends: Vec<String>,
    implements: Vec<String>,
    trait_uses: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredImportKind {
    Class,
    Function,
    Constant,
}

impl StoredImportKind {
    fn of(kind: ImportKind) -> Self {
        match kind {
            ImportKind::Class => Self::Class,
            ImportKind::Function => Self::Function,
            ImportKind::Constant => Self::Constant,
        }
    }

    fn to_kind(self) -> ImportKind {
        match self {
            Self::Class => ImportKind::Class,
            Self::Function => ImportKind::Function,
            Self::Constant => ImportKind::Constant,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredUseImport {
    kind: StoredImportKind,
    target: String,
    alias: String,
    namespace: String,
    ast_index: u32,
}

/// One file's item tree with its process-local file identity removed:
/// only the declaration indexes survive, and `to_item_tree` stamps the
/// current identity back in. `defines` carries no index of its own to
/// strip: it is already the range-free, position-order list `ItemTree`
/// keeps it as (see `celerrate_semantics::items`'s module doc), and
/// `DefineId` reconstructs its position from this list's order, exactly
/// as the live query does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredItemTree {
    declarations: Vec<StoredDeclaration>,
    imports: Vec<StoredUseImport>,
    defines: Vec<String>,
}

impl StoredItemTree {
    pub fn of(tree: &ItemTree) -> Self {
        Self {
            declarations: tree
                .declarations
                .iter()
                .map(|declaration| StoredDeclaration {
                    kind: StoredDeclarationKind::of(declaration.kind),
                    name: declaration.name.clone(),
                    namespace: declaration.namespace.clone(),
                    ast_index: declaration.ast_id.index,
                    extends: declaration.extends.clone(),
                    implements: declaration.implements.clone(),
                    trait_uses: declaration.trait_uses.clone(),
                })
                .collect(),
            imports: tree
                .imports
                .iter()
                .map(|import| StoredUseImport {
                    kind: StoredImportKind::of(import.kind),
                    target: import.target.clone(),
                    alias: import.alias.clone(),
                    namespace: import.namespace.clone(),
                    ast_index: import.ast_id.index,
                })
                .collect(),
            defines: tree.defines.clone(),
        }
    }

    pub fn to_item_tree(&self, file: FileId) -> ItemTree {
        ItemTree {
            declarations: self
                .declarations
                .iter()
                .map(|declaration| Declaration {
                    kind: declaration.kind.to_kind(),
                    name: declaration.name.clone(),
                    namespace: declaration.namespace.clone(),
                    ast_id: AstId {
                        file,
                        index: declaration.ast_index,
                    },
                    extends: declaration.extends.clone(),
                    implements: declaration.implements.clone(),
                    trait_uses: declaration.trait_uses.clone(),
                })
                .collect(),
            imports: self
                .imports
                .iter()
                .map(|import| UseImport {
                    kind: import.kind.to_kind(),
                    target: import.target.clone(),
                    alias: import.alias.clone(),
                    namespace: import.namespace.clone(),
                    ast_id: AstId {
                        file,
                        index: import.ast_index,
                    },
                })
                .collect(),
            defines: self.defines.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredMemberKind {
    Method,
    Property,
    ClassConstant,
    EnumCase,
}

impl StoredMemberKind {
    fn of(kind: MemberKind) -> Self {
        match kind {
            MemberKind::Method => Self::Method,
            MemberKind::Property => Self::Property,
            MemberKind::ClassConstant => Self::ClassConstant,
            MemberKind::EnumCase => Self::EnumCase,
        }
    }

    fn to_kind(self) -> MemberKind {
        match self {
            Self::Method => MemberKind::Method,
            Self::Property => MemberKind::Property,
            Self::ClassConstant => MemberKind::ClassConstant,
            Self::EnumCase => MemberKind::EnumCase,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredVisibility {
    Public,
    Protected,
    Private,
}

impl StoredVisibility {
    fn of(visibility: Visibility) -> Self {
        match visibility {
            Visibility::Public => Self::Public,
            Visibility::Protected => Self::Protected,
            Visibility::Private => Self::Private,
        }
    }

    fn to_visibility(self) -> Visibility {
        match self {
            Self::Public => Visibility::Public,
            Self::Protected => Visibility::Protected,
            Self::Private => Visibility::Private,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMemberFlags {
    visibility: StoredVisibility,
    is_static: bool,
    is_abstract: bool,
    is_final: bool,
    is_readonly: bool,
}

impl StoredMemberFlags {
    fn of(flags: MemberFlags) -> Self {
        Self {
            visibility: StoredVisibility::of(flags.visibility),
            is_static: flags.is_static,
            is_abstract: flags.is_abstract,
            is_final: flags.is_final,
            is_readonly: flags.is_readonly,
        }
    }

    fn to_flags(self) -> MemberFlags {
        MemberFlags {
            visibility: self.visibility.to_visibility(),
            is_static: self.is_static,
            is_abstract: self.is_abstract,
            is_final: self.is_final,
            is_readonly: self.is_readonly,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredParameterSignature {
    name: String,
    type_text: Option<String>,
    default_text: Option<String>,
    by_reference: bool,
    variadic: bool,
    is_promoted: bool,
}

impl StoredParameterSignature {
    fn of(parameter: &ParameterSignature) -> Self {
        Self {
            name: parameter.name.clone(),
            type_text: parameter.type_text.clone(),
            default_text: parameter.default_text.clone(),
            by_reference: parameter.by_reference,
            variadic: parameter.variadic,
            is_promoted: parameter.is_promoted,
        }
    }

    fn to_parameter(&self) -> ParameterSignature {
        ParameterSignature {
            name: self.name.clone(),
            type_text: self.type_text.clone(),
            default_text: self.default_text.clone(),
            by_reference: self.by_reference,
            variadic: self.variadic,
            is_promoted: self.is_promoted,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoredMemberSignature {
    parameters: Vec<StoredParameterSignature>,
    type_text: Option<String>,
    default_text: Option<String>,
    by_reference: bool,
}

impl StoredMemberSignature {
    fn of(signature: &MemberSignature) -> Self {
        Self {
            parameters: signature
                .parameters
                .iter()
                .map(StoredParameterSignature::of)
                .collect(),
            type_text: signature.type_text.clone(),
            default_text: signature.default_text.clone(),
            by_reference: signature.by_reference,
        }
    }

    fn to_signature(&self) -> MemberSignature {
        MemberSignature {
            parameters: self
                .parameters
                .iter()
                .map(StoredParameterSignature::to_parameter)
                .collect(),
            type_text: self.type_text.clone(),
            default_text: self.default_text.clone(),
            by_reference: self.by_reference,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMember {
    kind: StoredMemberKind,
    name: String,
    flags: StoredMemberFlags,
    signature: StoredMemberSignature,
    docblock: Option<String>,
    ast_index: u32,
}

impl StoredMember {
    fn of(member: &Member) -> Self {
        Self {
            kind: StoredMemberKind::of(member.kind),
            name: member.name.clone(),
            flags: StoredMemberFlags::of(member.flags),
            signature: StoredMemberSignature::of(&member.signature),
            docblock: member.docblock.clone(),
            ast_index: member.ast_id.index,
        }
    }

    fn to_member(&self, file: FileId) -> Member {
        Member {
            kind: self.kind.to_kind(),
            name: self.name.clone(),
            flags: self.flags.to_flags(),
            signature: self.signature.to_signature(),
            docblock: self.docblock.clone(),
            ast_id: AstId {
                file,
                index: self.ast_index,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredTraitAdaptation {
    Precedence {
        trait_name: Option<String>,
        member: String,
        excluded: Vec<String>,
    },
    Alias {
        trait_name: Option<String>,
        member: String,
        visibility: Option<StoredVisibility>,
        alias: Option<String>,
    },
}

impl StoredTraitAdaptation {
    fn of(adaptation: &TraitAdaptation) -> Self {
        match adaptation {
            TraitAdaptation::Precedence {
                trait_name,
                member,
                excluded,
            } => Self::Precedence {
                trait_name: trait_name.clone(),
                member: member.clone(),
                excluded: excluded.clone(),
            },
            TraitAdaptation::Alias {
                trait_name,
                member,
                visibility,
                alias,
            } => Self::Alias {
                trait_name: trait_name.clone(),
                member: member.clone(),
                visibility: visibility.map(StoredVisibility::of),
                alias: alias.clone(),
            },
        }
    }

    fn to_adaptation(&self) -> TraitAdaptation {
        match self {
            Self::Precedence {
                trait_name,
                member,
                excluded,
            } => TraitAdaptation::Precedence {
                trait_name: trait_name.clone(),
                member: member.clone(),
                excluded: excluded.clone(),
            },
            Self::Alias {
                trait_name,
                member,
                visibility,
                alias,
            } => TraitAdaptation::Alias {
                trait_name: trait_name.clone(),
                member: member.clone(),
                visibility: visibility.map(StoredVisibility::to_visibility),
                alias: alias.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredTraitUse {
    names: Vec<String>,
    adaptations: Vec<StoredTraitAdaptation>,
}

impl StoredTraitUse {
    fn of(trait_use: &TraitUse) -> Self {
        Self {
            names: trait_use.names.clone(),
            adaptations: trait_use
                .adaptations
                .iter()
                .map(StoredTraitAdaptation::of)
                .collect(),
        }
    }

    fn to_trait_use(&self) -> TraitUse {
        TraitUse {
            names: self.names.clone(),
            adaptations: self
                .adaptations
                .iter()
                .map(StoredTraitAdaptation::to_adaptation)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredClassMembers {
    kind: StoredDeclarationKind,
    name: Option<String>,
    namespace: String,
    ast_index: u32,
    docblock: Option<String>,
    members: Vec<StoredMember>,
    trait_uses: Vec<StoredTraitUse>,
    attribute_names: Vec<String>,
    extends: Vec<String>,
    implements: Vec<String>,
}

impl StoredClassMembers {
    fn of(class: &ClassMembers) -> Self {
        Self {
            kind: StoredDeclarationKind::of(class.kind),
            name: class.name.clone(),
            namespace: class.namespace.clone(),
            ast_index: class.ast_id.index,
            docblock: class.docblock.clone(),
            members: class.members.iter().map(StoredMember::of).collect(),
            trait_uses: class.trait_uses.iter().map(StoredTraitUse::of).collect(),
            attribute_names: class.attribute_names.clone(),
            extends: class.extends.clone(),
            implements: class.implements.clone(),
        }
    }

    fn to_class_members(&self, file: FileId) -> ClassMembers {
        ClassMembers {
            kind: self.kind.to_kind(),
            name: self.name.clone(),
            namespace: self.namespace.clone(),
            ast_id: AstId {
                file,
                index: self.ast_index,
            },
            docblock: self.docblock.clone(),
            members: self
                .members
                .iter()
                .map(|member| member.to_member(file))
                .collect(),
            trait_uses: self
                .trait_uses
                .iter()
                .map(StoredTraitUse::to_trait_use)
                .collect(),
            attribute_names: self.attribute_names.clone(),
            extends: self.extends.clone(),
            implements: self.implements.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFreeFunction {
    name: String,
    namespace: String,
    signature: StoredMemberSignature,
    docblock: Option<String>,
    ast_index: u32,
}

impl StoredFreeFunction {
    fn of(function: &FreeFunction) -> Self {
        Self {
            name: function.name.clone(),
            namespace: function.namespace.clone(),
            signature: StoredMemberSignature::of(&function.signature),
            docblock: function.docblock.clone(),
            ast_index: function.ast_id.index,
        }
    }

    fn to_free_function(&self, file: FileId) -> FreeFunction {
        FreeFunction {
            name: self.name.clone(),
            namespace: self.namespace.clone(),
            signature: self.signature.to_signature(),
            docblock: self.docblock.clone(),
            ast_id: AstId {
                file,
                index: self.ast_index,
            },
        }
    }
}

/// One file's member tree with its process-local file identity
/// removed, the `StoredItemTree` mirror pattern transposed: every
/// `AstId` reduced to `ast_index: u32`, stamped back with the current
/// identity by `to_member_tree`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoredMemberTree {
    classes: Vec<StoredClassMembers>,
    functions: Vec<StoredFreeFunction>,
}

impl StoredMemberTree {
    pub fn of(tree: &MemberTree) -> Self {
        Self {
            classes: tree.classes.iter().map(StoredClassMembers::of).collect(),
            functions: tree.functions.iter().map(StoredFreeFunction::of).collect(),
        }
    }

    pub fn to_member_tree(&self, file: FileId) -> MemberTree {
        MemberTree {
            classes: self
                .classes
                .iter()
                .map(|class| class.to_class_members(file))
                .collect(),
            functions: self
                .functions
                .iter()
                .map(|function| function.to_free_function(file))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredSeverity {
    Warning,
    Error,
}

/// One diagnostic with its process-local file identity removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDiagnostic {
    pub id: String,
    pub severity: StoredSeverity,
    pub start: u32,
    pub end: u32,
    pub message: String,
}

impl StoredDiagnostic {
    pub fn of(diagnostic: &Diagnostic) -> Option<Self> {
        let (_, range) = diagnostic.span()?;
        Some(Self {
            id: diagnostic.id.as_str().to_owned(),
            severity: match diagnostic.severity {
                Severity::Warning => StoredSeverity::Warning,
                Severity::Error => StoredSeverity::Error,
            },
            start: range.start().into(),
            end: range.end().into(),
            message: diagnostic.message.clone(),
        })
    }

    /// `None` when the stored identifier is unknown to the registry (the
    /// entry comes from another era), the stored range has `start > end`
    /// (the entry cannot come from any real computation: `TextRange::new`
    /// asserts the ordering and panics otherwise), or the range reaches
    /// past `content_length` (no computation over these bytes could have
    /// produced it). Either way the answer is the same: discard the entry
    /// and let the file recompute. The blake3 checksum a pack carries
    /// proves only that its bytes were not corrupted in transit, never
    /// that whoever wrote them was honest, so both bounds must be checked
    /// here rather than trusted.
    pub fn to_diagnostic(&self, file: FileId, content_length: u32) -> Option<Diagnostic> {
        if self.start > self.end || self.end > content_length {
            return None;
        }
        Some(Diagnostic::spanned(
            find_identifier(&self.id)?,
            match self.severity {
                StoredSeverity::Warning => Severity::Warning,
                StoredSeverity::Error => Severity::Error,
            },
            file,
            TextRange::new(TextSize::from(self.start), TextSize::from(self.end)),
            self.message.clone(),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredSpace {
    ClassLike,
    Function,
    Constant,
}

impl StoredSpace {
    fn of(space: SymbolSpace) -> Self {
        match space {
            SymbolSpace::ClassLike => Self::ClassLike,
            SymbolSpace::Function => Self::Function,
            SymbolSpace::Constant => Self::Constant,
        }
    }

    pub fn to_space(self) -> SymbolSpace {
        match self {
            Self::ClassLike => SymbolSpace::ClassLike,
            Self::Function => SymbolSpace::Function,
            Self::Constant => SymbolSpace::Constant,
        }
    }
}

fn stored_version(version: Option<PhpVersion>) -> Option<(u8, u8)> {
    version.map(|version| (version.major, version.minor))
}

/// A resolution answer in stored form. The deprecation nests two
/// options deliberately: the outer one is "is the symbol deprecated at
/// all", the inner one is "does the deprecation name a version".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredAnswer {
    Unknown,
    Source,
    Stub {
        introduced: Option<(u8, u8)>,
        removed: Option<(u8, u8)>,
        deprecated: Option<Option<(u8, u8)>>,
    },
}

impl StoredAnswer {
    pub fn of(answer: ResolutionAnswer) -> Self {
        match answer {
            ResolutionAnswer::Unknown => Self::Unknown,
            ResolutionAnswer::Source => Self::Source,
            ResolutionAnswer::Stub { availability } => Self::Stub {
                introduced: stored_version(availability.introduced),
                removed: stored_version(availability.removed),
                deprecated: availability
                    .deprecated
                    .map(|deprecation| stored_version(deprecation.since)),
            },
        }
    }
}

/// One revalidation record in stored form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRecord {
    pub written: String,
    pub space: StoredSpace,
    pub namespace: String,
    pub answer: StoredAnswer,
}

impl StoredRecord {
    pub fn of(record: &ResolutionRecord) -> Self {
        Self {
            written: record.written.clone(),
            space: StoredSpace::of(record.space),
            namespace: record.namespace.clone(),
            answer: StoredAnswer::of(record.answer),
        }
    }

    /// Whether the recorded answer still holds.
    pub fn matches(&self, answer: ResolutionAnswer) -> bool {
        self.answer == StoredAnswer::of(answer)
    }

    /// The record's symbol space, in domain form.
    pub fn space(&self) -> SymbolSpace {
        self.space.to_space()
    }
}

/// One reported file's typed portion, persisted (plan 9a, task 9): the
/// CEL0030-CEL0038 families' diagnostics alongside the revalidation
/// records `crate::cache::verdict`'s layered validation checks before
/// serving them again — the file-level counterpart of
/// [`celerrate_types::StoredInferredSignature`] (task 7's per-body
/// artifact), shaped the same way (a digest per consulted class and
/// function, an inferred edge's callee key and raw pre-substitution
/// return type) but scoped to a whole file's typed findings rather than
/// one body's inferred return.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredTypedVerdict {
    /// Post-suppression, schema-4 convention (module doc above).
    pub diagnostics: Vec<StoredDiagnostic>,
    pub classes: Vec<StoredClassDependency>,
    pub functions: Vec<StoredFunctionDependency>,
    pub inferred: Vec<StoredInferredEdge>,
}

/// One reported file's persisted verdict: its composed diagnostics and
/// the records that must revalidate before they may speak again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredVerdict {
    pub diagnostics: Vec<StoredDiagnostic>,
    pub records: Vec<StoredRecord>,
    /// The typed half (plan 9a, task 9): `None` when the persist lever
    /// (`crate::cache::PERSIST_TYPED_ARTIFACTS`) is off, `Some` otherwise
    /// — never a partial `StoredTypedVerdict`, since `composed_verdict`
    /// computes both fields of the option together.
    pub typed: Option<StoredTypedVerdict>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
    use celerrate_semantics::{
        ItemTree, MemberKind, MemberTree, ResolutionAnswer, TraitAdaptation,
    };
    use celerrate_source::{FileId, TextRange, TextSize};
    use celerrate_stubs::{StubAvailability, StubDeprecation};

    use super::{
        StoredAnswer, StoredDiagnostic, StoredItemTree, StoredMemberTree, StoredRecord,
        StoredSeverity,
    };

    fn parsed_tree(file: u32, source: &str) -> ItemTree {
        let parse = celerrate_syntax::parse(source);
        ItemTree::from_root(FileId::new(file), &parse.tree())
    }

    fn parsed_member_tree(file: u32, source: &str) -> MemberTree {
        let parse = celerrate_syntax::parse(source);
        MemberTree::from_root(FileId::new(file), &parse.tree())
    }

    #[test]
    fn an_item_tree_round_trips_onto_another_file_identity() {
        let source = "<?php namespace App; use Lib\\Helper as H; \
                      class Service extends Base implements Contract {} \
                      interface Contract {} \
                      trait Sharable {} \
                      enum Status {} \
                      function run() {} const LIMIT = 3; \
                      function boot() { define('APP_ROOT', __DIR__); }";
        let original = parsed_tree(3, source);
        assert_eq!(
            original.defines,
            vec!["APP_ROOT".to_owned()],
            "sanity: the fixture actually carries a define",
        );
        let stored = StoredItemTree::of(&original);
        let remapped = stored.to_item_tree(FileId::new(9));
        assert_eq!(remapped, parsed_tree(9, source));
        assert_eq!(remapped.defines, vec!["APP_ROOT".to_owned()]);
    }

    #[test]
    fn a_member_tree_round_trips_onto_another_file_identity() {
        let source = "<?php namespace App;\n\
             #[AllowDynamicProperties]\n\
             class Service {\n\
                 use Sharable { hello as protected hi; }\n\
                 public function __construct(private readonly int $id) {}\n\
                 /** @return int */\n\
                 public function &compute(int $count, string $label = 'x'): int { return $count; }\n\
             }\n\
             enum Status { case Active; }\n\
             /** doc */\n\
             function build(int $count): void {}\n\
             function wrapper() { return new class { public function f() {} }; }";
        let original = parsed_member_tree(3, source);

        // Sanity: the fixture actually carries every feature the mirror
        // must preserve.
        let service = original
            .classes
            .iter()
            .find(|class| class.name.as_deref() == Some("Service"))
            .unwrap();
        assert_eq!(
            service.attribute_names,
            vec!["AllowDynamicProperties".to_owned()],
        );
        assert_eq!(
            service.trait_uses.first().unwrap().adaptations.first(),
            Some(&TraitAdaptation::Alias {
                trait_name: None,
                member: "hello".to_owned(),
                visibility: Some(celerrate_semantics::Visibility::Protected),
                alias: Some("hi".to_owned()),
            }),
        );
        let promoted = service
            .members
            .iter()
            .find(|member| member.kind == MemberKind::Property)
            .unwrap();
        assert!(promoted.flags.is_readonly);
        let compute = service
            .members
            .iter()
            .find(|member| member.name == "compute")
            .unwrap();
        assert!(compute.signature.by_reference);
        assert_eq!(compute.docblock.as_deref(), Some("/** @return int */"));
        assert_eq!(compute.signature.parameters.len(), 2);
        assert_eq!(
            compute.signature.parameters[1].default_text.as_deref(),
            Some("'x'"),
        );
        let status = original
            .classes
            .iter()
            .find(|class| class.name.as_deref() == Some("Status"))
            .unwrap();
        assert_eq!(
            status.members.first().map(|member| member.kind),
            Some(MemberKind::EnumCase),
        );
        let build = original
            .functions
            .iter()
            .find(|function| function.name == "build")
            .unwrap();
        assert_eq!(build.docblock.as_deref(), Some("/** doc */"));
        assert!(
            original.classes.iter().any(|class| class.name.is_none()),
            "sanity: the anonymous class is projected",
        );

        let stored = StoredMemberTree::of(&original);
        let remapped = stored.to_member_tree(FileId::new(9));
        assert_eq!(remapped, parsed_member_tree(9, source));
    }

    #[test]
    fn a_diagnostic_round_trips_and_an_unknown_identifier_is_rejected() {
        let original = Diagnostic::spanned(
            DiagnosticId::new("CEL0018"),
            Severity::Error,
            FileId::new(3),
            TextRange::new(TextSize::from(5), TextSize::from(12)),
            "unknown class Missing".to_owned(),
        );
        let stored = StoredDiagnostic::of(&original).unwrap();
        let remapped = stored.to_diagnostic(FileId::new(9), 100).unwrap();
        assert_eq!(remapped.id, original.id);
        assert_eq!(remapped.severity, original.severity);
        assert_eq!(
            remapped.span(),
            Some((FileId::new(9), original.span().unwrap().1))
        );
        assert_eq!(remapped.message, original.message);

        let unknown = StoredDiagnostic {
            id: "CEL9999".to_owned(),
            ..stored
        };
        assert!(unknown.to_diagnostic(FileId::new(9), 100).is_none());
    }

    /// A stored range with `start > end` cannot come from any real
    /// computation: `TextRange::new` asserts `start <= end` and panics
    /// otherwise. The blake3 checksum a hostile pack carries only proves
    /// nothing bit-flipped in transit, not that the entry is honest, so
    /// this must be rejected here, like an unknown identifier, rather than
    /// reach `TextRange::new` at all.
    #[test]
    fn a_reversed_range_is_rejected_without_panicking() {
        let reversed = StoredDiagnostic {
            id: "CEL0018".to_owned(),
            severity: StoredSeverity::Error,
            start: 17,
            end: 10,
            message: "crafted".to_owned(),
        };
        assert!(reversed.to_diagnostic(FileId::new(9), 100).is_none());
    }

    /// The boundary itself, an empty range (`start == end`), is a real
    /// shape ordinary diagnostics use and must keep round-tripping.
    #[test]
    fn an_empty_range_round_trips() {
        let empty = StoredDiagnostic {
            id: "CEL0018".to_owned(),
            severity: StoredSeverity::Error,
            start: 10,
            end: 10,
            message: "empty span".to_owned(),
        };
        let diagnostic = empty.to_diagnostic(FileId::new(9), 100).unwrap();
        assert_eq!(
            diagnostic.span().unwrap().1,
            TextRange::new(TextSize::from(10), TextSize::from(10))
        );
    }

    /// Audit finding M4: a crafted span past the file's end was accepted
    /// and rendered with an oversized column — a hit that is not
    /// byte-for-byte anything the computation could produce. The content
    /// the entry's key hashes is available at both call sites, so the
    /// length is checked here, like the ordering.
    #[test]
    fn a_span_past_the_files_end_is_rejected() {
        let oversized = StoredDiagnostic {
            id: "CEL0018".to_owned(),
            severity: StoredSeverity::Error,
            start: 10,
            end: 40,
            message: "crafted".to_owned(),
        };
        assert!(oversized.to_diagnostic(FileId::new(9), 20).is_none());
        assert!(
            oversized.to_diagnostic(FileId::new(9), 40).is_some(),
            "a span ending exactly at the file's end is valid",
        );
    }

    #[test]
    fn every_answer_shape_round_trips_through_matches() {
        let answers = [
            ResolutionAnswer::Unknown,
            ResolutionAnswer::Source,
            ResolutionAnswer::Stub {
                availability: StubAvailability::ALWAYS,
            },
            ResolutionAnswer::Stub {
                availability: StubAvailability {
                    introduced: Some(celerrate_project::PhpVersion::new(8, 2)),
                    removed: Some(celerrate_project::PhpVersion::new(8, 4)),
                    deprecated: Some(StubDeprecation {
                        since: Some(celerrate_project::PhpVersion::new(8, 3)),
                    }),
                },
            },
            ResolutionAnswer::Stub {
                availability: StubAvailability {
                    introduced: None,
                    removed: None,
                    deprecated: Some(StubDeprecation { since: None }),
                },
            },
        ];
        for answer in answers {
            let record = StoredRecord {
                written: "Name".to_owned(),
                space: super::StoredSpace::ClassLike,
                namespace: String::new(),
                answer: StoredAnswer::of(answer),
            };
            assert!(record.matches(answer), "{answer:?} must match itself");
            assert_eq!(record.space(), celerrate_semantics::SymbolSpace::ClassLike);
            for other in answers {
                if other != answer {
                    assert!(
                        !record.matches(other),
                        "{answer:?} must not match {other:?}"
                    );
                }
            }
        }
    }
}
