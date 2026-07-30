//! The serialized forms of the cached artifacts. Mirror types rather
//! than derives on the domain types, because the conversion is the
//! schema: a `FileId` is process-local and must be stamped back in at
//! load, and a `DiagnosticId` wraps a `'static` string that must be
//! re-interned through the registry. Every `to_*` conversion is total
//! except identifier re-interning, whose failure discards the entry.
//!
//! **The stored diagnostic's anatomy (schema 6, unchanged in schema 7).**
//! `StoredDiagnostic` mirrors the domain `Diagnostic` whole: its anchor
//! (`Project` or a bounds-checked `Span`), its labels (a local range or a
//! symbolic name), its notes, and its suggestions (a confidence plus
//! same-file text edits — no file identity of their own, since a
//! suggestion's edits always target the diagnostic's own file).
//! `to_diagnostic` bounds-checks EVERY stored range against
//! `content_length` — the anchor's span, each local label's range, each
//! edit's range — because a blake3 checksum proves only that a pack's bytes
//! were not corrupted in transit, never that whoever wrote them was honest.
//!
//! **The suppression note.** `StoredVerdict.diagnostics`
//! and `StoredTypedVerdict.diagnostics` are both stored POST-suppression
//! (schema 4's convention, unchanged): every persisted diagnostic has
//! already survived `celerrate_semantics::suppression_directives`'s filter.
//! Suppression directives are strictly file-local facts read from the
//! same file the verdict's content-hash key covers, so editing even a
//! comment — never mind the directive itself — moves the hash and
//! discards the WHOLE entry, untyped and typed halves alike (`stale
//! suppression is structurally impossible`, `cache_suppression.rs`'s own
//! module doc). A stale suppression decision can therefore never survive
//! into a served verdict, typed or not.
//!
//! **The directive records (schema 7).** `StoredVerdict.directives`
//! carries every resolved directive on the file, each with the UNTYPED
//! half's own `matched` flag; `StoredTypedVerdict.matched_directives`
//! carries the typed half's own admitting indexes into that same list,
//! separately, so a recomputed typed half never serves the untyped
//! half's stale union. `StoredDirective::to_directive` bounds-checks its
//! ranges exactly like `StoredDiagnostic::to_diagnostic` and re-interns
//! its filter codes, canonicalizing (sorting and deduplicating) them on
//! load, since `ResolvedDirective::admits` binary-searches a `Only`
//! filter's codes and a hand-crafted, checksum-valid pack must not be
//! able to make that lie. `StoredVerdict::directives_convert` extends
//! the same validation to `matched_directives`: every index must be in
//! range and the list strictly increasing, or the whole verdict is
//! discarded - the checksum proves transport, never honesty. On a
//! partial hit, the stored untyped records and a freshly recomputed
//! typed half's indexes are never cross-checked against each other;
//! their alignment rests on the content hash plus the binary-identity
//! pack key alone, the same trust every other stored half already
//! extends.

use celerrate_diagnostics::{
    Anchor, Confidence, Diagnostic, Label, LabelTarget, Severity, Suggestion, find_identifier,
};
use celerrate_project::PhpVersion;
use celerrate_semantics::{
    AstId, ClassMembers, Declaration, DeclarationKind, DirectiveOrigin, FreeFunction, ImportKind,
    ItemTree, Member, MemberFlags, MemberKind, MemberSignature, MemberTree, ParameterSignature,
    ResolutionAnswer, ResolutionRecord, ResolvedDirective, SuppressionFilter, SymbolSpace,
    TraitAdaptation, TraitUse, UseImport, Visibility,
};
use celerrate_source::{FileId, TextEdit, TextRange, TextSize};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredAnchor {
    Project,
    Span { start: u32, end: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredLabelTarget {
    Local { start: u32, end: u32 },
    Symbolic { symbol: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredLabel {
    pub target: StoredLabelTarget,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredConfidence {
    Safe,
    NeedsReview,
}

/// A stored edit carries no file identity: a suggestion's edits target
/// the diagnostic's own file, and the stored form
/// enforces that structurally by having nowhere to write another file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredTextEdit {
    pub start: u32,
    pub end: u32,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSuggestion {
    pub message: String,
    pub confidence: StoredConfidence,
    pub edits: Vec<StoredTextEdit>,
}

/// One diagnostic with its process-local file identity removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDiagnostic {
    pub id: String,
    pub severity: StoredSeverity,
    pub anchor: StoredAnchor,
    pub message: String,
    pub labels: Vec<StoredLabel>,
    pub notes: Vec<String>,
    pub suggestions: Vec<StoredSuggestion>,
}

impl StoredDiagnostic {
    pub fn of(diagnostic: &Diagnostic) -> Self {
        Self {
            id: diagnostic.id.as_str().to_owned(),
            severity: match diagnostic.severity {
                Severity::Warning => StoredSeverity::Warning,
                Severity::Error => StoredSeverity::Error,
            },
            anchor: match diagnostic.anchor {
                Anchor::Project => StoredAnchor::Project,
                Anchor::Span { range, .. } => StoredAnchor::Span {
                    start: range.start().into(),
                    end: range.end().into(),
                },
            },
            message: diagnostic.message.clone(),
            labels: diagnostic
                .labels
                .iter()
                .map(|label| StoredLabel {
                    target: match &label.target {
                        LabelTarget::Local { range } => StoredLabelTarget::Local {
                            start: range.start().into(),
                            end: range.end().into(),
                        },
                        LabelTarget::Symbolic { symbol } => StoredLabelTarget::Symbolic {
                            symbol: symbol.clone(),
                        },
                    },
                    message: label.message.clone(),
                })
                .collect(),
            notes: diagnostic.notes.clone(),
            suggestions: diagnostic
                .suggestions
                .iter()
                .map(|suggestion| StoredSuggestion {
                    message: suggestion.message.clone(),
                    confidence: match suggestion.confidence {
                        Confidence::Safe => StoredConfidence::Safe,
                        Confidence::NeedsReview => StoredConfidence::NeedsReview,
                    },
                    edits: suggestion
                        .edits
                        .iter()
                        .map(|edit| StoredTextEdit {
                            start: edit.range.start().into(),
                            end: edit.range.end().into(),
                            replacement: edit.replacement.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// `None` when the stored identifier is unknown to the registry, or
    /// when ANY stored range (the anchor's, a local label's, an edit's)
    /// is inverted or reaches past `content_length`. The blake3 checksum
    /// a pack carries proves only that its bytes were not corrupted in
    /// transit, never that whoever wrote them was honest, so every range
    /// is checked here rather than trusted.
    pub fn to_diagnostic(&self, file: FileId, content_length: u32) -> Option<Diagnostic> {
        let in_bounds = |start: u32, end: u32| start <= end && end <= content_length;
        let anchor = match self.anchor {
            StoredAnchor::Project => Anchor::Project,
            StoredAnchor::Span { start, end } => {
                if !in_bounds(start, end) {
                    return None;
                }
                Anchor::Span {
                    file,
                    range: TextRange::new(TextSize::from(start), TextSize::from(end)),
                }
            }
        };
        let mut labels = Vec::with_capacity(self.labels.len());
        for label in &self.labels {
            let target = match &label.target {
                StoredLabelTarget::Local { start, end } => {
                    if !in_bounds(*start, *end) {
                        return None;
                    }
                    LabelTarget::Local {
                        range: TextRange::new(TextSize::from(*start), TextSize::from(*end)),
                    }
                }
                StoredLabelTarget::Symbolic { symbol } => LabelTarget::Symbolic {
                    symbol: symbol.clone(),
                },
            };
            labels.push(Label {
                target,
                message: label.message.clone(),
            });
        }
        let mut suggestions = Vec::with_capacity(self.suggestions.len());
        for suggestion in &self.suggestions {
            let mut edits = Vec::with_capacity(suggestion.edits.len());
            for edit in &suggestion.edits {
                if !in_bounds(edit.start, edit.end) {
                    return None;
                }
                edits.push(TextEdit {
                    file,
                    range: TextRange::new(TextSize::from(edit.start), TextSize::from(edit.end)),
                    replacement: edit.replacement.clone(),
                });
            }
            suggestions.push(Suggestion {
                message: suggestion.message.clone(),
                confidence: match suggestion.confidence {
                    StoredConfidence::Safe => Confidence::Safe,
                    StoredConfidence::NeedsReview => Confidence::NeedsReview,
                },
                edits,
            });
        }
        Some(Diagnostic {
            id: find_identifier(&self.id)?,
            severity: match self.severity {
                StoredSeverity::Warning => Severity::Warning,
                StoredSeverity::Error => Severity::Error,
            },
            anchor,
            message: self.message.clone(),
            labels,
            notes: self.notes.clone(),
            suggestions,
        })
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredSuppressionFilter {
    All,
    Only(Vec<String>),
}

/// One resolved directive with its untyped-half match outcome: what
/// the `Reporting` phase replays on the warm path without re-parsing.
/// The typed half's own outcomes live in
/// `StoredTypedVerdict.matched_directives`, indexes into this list, so
/// a recomputed typed half never serves a stale union.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDirective {
    pub anchor_start: u32,
    pub anchor_end: u32,
    pub scope_start: u32,
    pub scope_end: u32,
    pub filter: StoredSuppressionFilter,
    pub identifiers: Vec<String>,
    pub native: bool,
    /// Whether the untyped half's filter admitted any diagnostic.
    pub matched: bool,
}

impl StoredDirective {
    pub fn of(directive: &ResolvedDirective, matched: bool) -> Self {
        Self {
            anchor_start: directive.anchor.start().into(),
            anchor_end: directive.anchor.end().into(),
            scope_start: directive.scope.start().into(),
            scope_end: directive.scope.end().into(),
            filter: match &directive.filter {
                SuppressionFilter::All => StoredSuppressionFilter::All,
                SuppressionFilter::Only(codes) => StoredSuppressionFilter::Only(
                    codes.iter().map(|code| code.as_str().to_owned()).collect(),
                ),
            },
            identifiers: directive.identifiers.clone(),
            native: directive.origin == DirectiveOrigin::Native,
            matched,
        }
    }

    /// `None` when a range is inverted or out of bounds, or a filter
    /// code no longer interns: another era's record, discarded like a
    /// failed diagnostic conversion - the checksum proves transport,
    /// never honesty.
    pub fn to_directive(&self, content_length: u32) -> Option<(ResolvedDirective, bool)> {
        let in_bounds = |start: u32, end: u32| start <= end && end <= content_length;
        if !in_bounds(self.anchor_start, self.anchor_end)
            || !in_bounds(self.scope_start, self.scope_end)
        {
            return None;
        }
        let filter = match &self.filter {
            StoredSuppressionFilter::All => SuppressionFilter::All,
            StoredSuppressionFilter::Only(codes) => {
                let mut interned = Vec::with_capacity(codes.len());
                for code in codes {
                    interned.push(find_identifier(code)?);
                }
                // Canonicalize: `admits` binary-searches this list,
                // and a hand-crafted, checksum-valid pack must not
                // smuggle an unsorted list past validation (decision
                // 8's sharp edge (a)).
                interned.sort();
                interned.dedup();
                SuppressionFilter::Only(interned)
            }
        };
        Some((
            ResolvedDirective {
                anchor: TextRange::new(
                    TextSize::from(self.anchor_start),
                    TextSize::from(self.anchor_end),
                ),
                scope: TextRange::new(
                    TextSize::from(self.scope_start),
                    TextSize::from(self.scope_end),
                ),
                filter,
                identifiers: self.identifiers.clone(),
                // The fates are not persisted: the verbose channel
                // derives widening fresh from `suppression_directives`,
                // never from a reconstructed record, so nothing enters
                // the cache for it.
                widened_by: Vec::new(),
                origin: if self.native {
                    DirectiveOrigin::Native
                } else {
                    DirectiveOrigin::Foreign
                },
            },
            self.matched,
        ))
    }
}

/// One reported file's typed portion, persisted: the
/// CEL0030-CEL0038 families' diagnostics alongside the revalidation
/// records `crate::cache::verdict`'s layered validation checks before
/// serving them again, the file-level counterpart of
/// [`celerrate_types::StoredInferredSignature`] (the per-body
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
    /// The typed half's own admitting directive indexes, into
    /// `StoredVerdict.directives` (schema 7): a directive's
    /// stored `matched` flag is the UNTYPED half's own outcome, so a
    /// recomputed typed half never serves it as if it were the typed
    /// half's own. Indexes here must be strictly increasing on load
    /// (a sharp edge (a)): `StoredVerdict::directives_convert`
    /// discards the whole verdict otherwise.
    pub matched_directives: Vec<u32>,
}

/// One reported file's persisted verdict: its composed diagnostics and
/// the records that must revalidate before they may speak again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredVerdict {
    pub diagnostics: Vec<StoredDiagnostic>,
    pub records: Vec<StoredRecord>,
    /// Every resolved directive on this file, with the untyped half's
    /// own `matched` outcome (schema 7). The typed half's own
    /// outcomes live separately, as indexes into this list
    /// (`StoredTypedVerdict.matched_directives`), so a recomputed typed
    /// half never serves a stale union of the two.
    pub directives: Vec<StoredDirective>,
    /// The typed half: `None` when the persist lever
    /// (`crate::cache::PERSIST_TYPED_ARTIFACTS`) is off, `Some` otherwise
    /// — never a partial `StoredTypedVerdict`, since `composed_verdict`
    /// computes both fields of the option together.
    pub typed: Option<StoredTypedVerdict>,
}

impl StoredVerdict {
    /// Every stored directive record converted, in stored order, with
    /// the typed half's indexes checked against the list's length and
    /// for strictly increasing order. `None` means the whole verdict
    /// is untrustworthy.
    ///
    /// On a partial hit the stored untyped records and the fresh typed
    /// indexes computed elsewhere are never cross-checked against each
    /// other here: their alignment rests on the content hash plus the
    /// binary-identity pack key, the same trust every other stored half
    /// already extends (a sharp edge (b)).
    pub fn directives_convert(
        &self,
        content_length: u32,
    ) -> Option<Vec<(ResolvedDirective, bool)>> {
        let records: Option<Vec<_>> = self
            .directives
            .iter()
            .map(|directive| directive.to_directive(content_length))
            .collect();
        let records = records?;
        if let Some(typed) = &self.typed {
            let in_range = typed
                .matched_directives
                .iter()
                .all(|&index| (index as usize) < records.len());
            // Strictly increasing: `directive_outcomes` binary-searches
            // this list; unsorted or duplicated indexes in a
            // checksum-valid pack must discard, not misattribute
            // (a sharp edge (a)).
            let sorted = typed
                .matched_directives
                .is_sorted_by(|left, right| left < right);
            if !in_range || !sorted {
                return None;
            }
        }
        Some(records)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use celerrate_diagnostics::{
        Confidence, Diagnostic, DiagnosticId, Label, LabelTarget, Severity, Suggestion,
    };
    use celerrate_semantics::{
        DirectiveOrigin, ItemTree, MemberKind, MemberTree, ResolutionAnswer, ResolvedDirective,
        SuppressionFilter, TraitAdaptation,
    };
    use celerrate_source::{FileId, TextEdit, TextRange, TextSize};
    use celerrate_stubs::{StubAvailability, StubDeprecation};

    use super::{
        StoredAnchor, StoredAnswer, StoredConfidence, StoredDiagnostic, StoredDirective,
        StoredItemTree, StoredLabel, StoredLabelTarget, StoredMemberTree, StoredRecord,
        StoredSeverity, StoredSuggestion, StoredSuppressionFilter, StoredTextEdit,
        StoredTypedVerdict, StoredVerdict,
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
        let stored = StoredDiagnostic::of(&original);
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
            anchor: StoredAnchor::Span { start: 17, end: 10 },
            message: "crafted".to_owned(),
            labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
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
            anchor: StoredAnchor::Span { start: 10, end: 10 },
            message: "empty span".to_owned(),
            labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
        };
        let diagnostic = empty.to_diagnostic(FileId::new(9), 100).unwrap();
        assert_eq!(
            diagnostic.span().unwrap().1,
            TextRange::new(TextSize::from(10), TextSize::from(10))
        );
    }

    /// A crafted span past the file's end was accepted
    /// and rendered with an oversized column — a hit that is not
    /// byte-for-byte anything the computation could produce. The content
    /// the entry's key hashes is available at both call sites, so the
    /// length is checked here, like the ordering.
    #[test]
    fn a_span_past_the_files_end_is_rejected() {
        let oversized = StoredDiagnostic {
            id: "CEL0018".to_owned(),
            severity: StoredSeverity::Error,
            anchor: StoredAnchor::Span { start: 10, end: 40 },
            message: "crafted".to_owned(),
            labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
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

    #[test]
    fn an_enriched_diagnostic_round_trips_with_its_anatomy() {
        let diagnostic = Diagnostic {
            labels: vec![
                Label {
                    target: LabelTarget::Local {
                        range: TextRange::new(TextSize::from(2), TextSize::from(5)),
                    },
                    message: "declared `int` here".to_owned(),
                },
                Label {
                    target: LabelTarget::Symbolic {
                        symbol: "App\\User::save".to_owned(),
                    },
                    message: "declared here".to_owned(),
                },
            ],
            notes: vec!["inferred `string|null` on this path".to_owned()],
            suggestions: vec![Suggestion {
                message: "did you mean `format`".to_owned(),
                confidence: Confidence::NeedsReview,
                edits: vec![TextEdit {
                    file: FileId::new(7),
                    range: TextRange::new(TextSize::from(4), TextSize::from(10)),
                    replacement: "format".to_owned(),
                }],
            }],
            ..Diagnostic::spanned(
                DiagnosticId::new("CEL0030"),
                Severity::Error,
                FileId::new(7),
                TextRange::new(TextSize::from(4), TextSize::from(10)),
                "unknown method `fromat`".to_owned(),
            )
        };
        let stored = StoredDiagnostic::of(&diagnostic);
        let restored = stored.to_diagnostic(FileId::new(7), 100).unwrap();
        assert_eq!(restored, diagnostic);
    }

    #[test]
    fn hostile_stored_ranges_discard_the_entry() {
        let sound = StoredDiagnostic::of(&Diagnostic::spanned(
            DiagnosticId::new("CEL0030"),
            Severity::Error,
            FileId::new(7),
            TextRange::new(TextSize::from(4), TextSize::from(10)),
            "unknown method".to_owned(),
        ));
        // A label range past the content length.
        let mut hostile_label = sound.clone();
        hostile_label.labels = vec![StoredLabel {
            target: StoredLabelTarget::Local { start: 0, end: 999 },
            message: "here".to_owned(),
        }];
        assert!(hostile_label.to_diagnostic(FileId::new(7), 100).is_none());
        // An inverted edit range.
        let mut hostile_edit = sound.clone();
        hostile_edit.suggestions = vec![StoredSuggestion {
            message: "did you mean `format`".to_owned(),
            confidence: StoredConfidence::NeedsReview,
            edits: vec![StoredTextEdit {
                start: 10,
                end: 4,
                replacement: "format".to_owned(),
            }],
        }];
        assert!(hostile_edit.to_diagnostic(FileId::new(7), 100).is_none());
    }

    #[test]
    fn a_project_anchored_diagnostic_round_trips_without_bounds() {
        let diagnostic = Diagnostic::project(
            DiagnosticId::new("CEL0025"),
            Severity::Warning,
            "no composer.json found".to_owned(),
        );
        let stored = StoredDiagnostic::of(&diagnostic);
        let restored = stored.to_diagnostic(FileId::new(0), 0).unwrap();
        assert_eq!(restored, diagnostic);
    }

    #[test]
    fn a_directive_record_round_trips() {
        let directive = ResolvedDirective {
            anchor: TextRange::new(TextSize::from(10), TextSize::from(40)),
            scope: TextRange::new(TextSize::from(6), TextSize::from(41)),
            filter: SuppressionFilter::Only(vec![
                celerrate_diagnostics::find_identifier("CEL0018").unwrap(),
            ]),
            identifiers: vec!["CEL0018".to_owned()],
            widened_by: Vec::new(),
            origin: DirectiveOrigin::Native,
        };
        let stored = StoredDirective::of(&directive, true);
        assert_eq!(stored.to_directive(100), Some((directive, true)));
    }

    #[test]
    fn a_directive_record_with_an_out_of_bounds_range_is_discarded() {
        let directive = ResolvedDirective {
            anchor: TextRange::new(TextSize::from(10), TextSize::from(40)),
            scope: TextRange::new(TextSize::from(6), TextSize::from(41)),
            filter: SuppressionFilter::All,
            identifiers: Vec::new(),
            widened_by: Vec::new(),
            origin: DirectiveOrigin::Foreign,
        };
        let stored = StoredDirective::of(&directive, false);
        assert!(stored.to_directive(20).is_none());

        // An inverted anchor (`start > end`), otherwise in bounds, with a
        // valid scope: `start <= end` is its own failure mode, distinct
        // from `end <= content_length`, and the existing case above never
        // exercises it on its own.
        let inverted_anchor = StoredDirective {
            anchor_start: 40,
            anchor_end: 10,
            scope_start: 0,
            scope_end: 5,
            filter: StoredSuppressionFilter::All,
            identifiers: Vec::new(),
            native: false,
            matched: false,
        };
        assert!(
            inverted_anchor.to_directive(100).is_none(),
            "an inverted anchor range must discard even though it is otherwise in bounds",
        );

        // A valid anchor with an out-of-bounds scope: the check must
        // actually reach the scope rather than stop once the anchor
        // passes.
        let bad_scope = StoredDirective {
            anchor_start: 0,
            anchor_end: 5,
            scope_start: 0,
            scope_end: 200,
            filter: StoredSuppressionFilter::All,
            identifiers: Vec::new(),
            native: false,
            matched: false,
        };
        assert!(
            bad_scope.to_directive(100).is_none(),
            "an out-of-bounds scope must discard even though the anchor is valid",
        );
    }

    #[test]
    fn a_directive_record_with_an_unknown_filter_code_is_discarded() {
        let stored = StoredDirective {
            anchor_start: 0,
            anchor_end: 5,
            scope_start: 0,
            scope_end: 5,
            filter: StoredSuppressionFilter::Only(vec!["CEL9999".to_owned()]),
            identifiers: vec!["CEL9999".to_owned()],
            native: true,
            matched: false,
        };
        assert!(stored.to_directive(100).is_none());
    }

    #[test]
    fn a_stored_filter_is_canonicalized_on_load() {
        // A hand-crafted, checksum-valid pack could store an unsorted or
        // duplicated list; `admits` binary-searches, so load canonicalizes
        // (a sharp edge (a)).
        let stored = StoredDirective {
            anchor_start: 0,
            anchor_end: 5,
            scope_start: 0,
            scope_end: 5,
            filter: StoredSuppressionFilter::Only(vec![
                "CEL0030".to_owned(),
                "CEL0018".to_owned(),
                "CEL0030".to_owned(),
            ]),
            identifiers: Vec::new(),
            native: true,
            matched: false,
        };
        let (directive, _) = stored.to_directive(100).unwrap();
        assert_eq!(
            directive.filter,
            SuppressionFilter::Only(vec![
                celerrate_diagnostics::find_identifier("CEL0018").unwrap(),
                celerrate_diagnostics::find_identifier("CEL0030").unwrap(),
            ]),
        );
    }

    /// Two convertible directive records, and a typed half whose
    /// `matched_directives` disagrees with one of the two independent
    /// rules `StoredVerdict::directives_convert` enforces: the
    /// strictly-increasing rule (out of order, and separately
    /// duplicated) and the in-range rule (an index beyond the stored
    /// list, and separately a non-empty index list against an empty
    /// stored list). Every case must discard the whole verdict
    /// (a sharp edge (a)); a sorted, in-range control proves
    /// the rule is actually enforced rather than nothing ever loading.
    #[test]
    fn unsorted_typed_match_indexes_discard_the_verdict() {
        let first = StoredDirective::of(
            &ResolvedDirective {
                anchor: TextRange::new(TextSize::from(0), TextSize::from(5)),
                scope: TextRange::new(TextSize::from(0), TextSize::from(5)),
                filter: SuppressionFilter::All,
                identifiers: Vec::new(),
                widened_by: Vec::new(),
                origin: DirectiveOrigin::Foreign,
            },
            true,
        );
        let second = StoredDirective::of(
            &ResolvedDirective {
                anchor: TextRange::new(TextSize::from(10), TextSize::from(15)),
                scope: TextRange::new(TextSize::from(10), TextSize::from(15)),
                filter: SuppressionFilter::All,
                identifiers: Vec::new(),
                widened_by: Vec::new(),
                origin: DirectiveOrigin::Foreign,
            },
            false,
        );

        let verdict_with = |matched_directives: Vec<u32>| StoredVerdict {
            diagnostics: Vec::new(),
            records: Vec::new(),
            directives: vec![first.clone(), second.clone()],
            typed: Some(StoredTypedVerdict {
                diagnostics: Vec::new(),
                classes: Vec::new(),
                functions: Vec::new(),
                inferred: Vec::new(),
                matched_directives,
            }),
        };

        assert!(
            verdict_with(vec![1, 0]).directives_convert(100).is_none(),
            "out-of-order indexes must discard the verdict",
        );
        assert!(
            verdict_with(vec![0, 0]).directives_convert(100).is_none(),
            "duplicated indexes must discard the verdict too: not strictly increasing",
        );
        assert!(
            verdict_with(vec![0, 5]).directives_convert(100).is_none(),
            "an index beyond the stored directive list must discard the verdict: \
             sorted and increasing, but out of range",
        );
        let verdict_with_no_directives = StoredVerdict {
            diagnostics: Vec::new(),
            records: Vec::new(),
            directives: Vec::new(),
            typed: Some(StoredTypedVerdict {
                diagnostics: Vec::new(),
                classes: Vec::new(),
                functions: Vec::new(),
                inferred: Vec::new(),
                matched_directives: vec![0],
            }),
        };
        assert!(
            verdict_with_no_directives.directives_convert(100).is_none(),
            "a non-empty index list against an empty stored directive list must \
             discard the verdict: sorted and increasing, but out of range",
        );
        let control = verdict_with(vec![0, 1]).directives_convert(100);
        assert!(control.is_some(), "a sorted, in-range index list must load",);
        assert_eq!(control.unwrap().len(), 2);
    }
}
