//! The serialized forms of the cached artifacts. Mirror types rather
//! than derives on the domain types, because the conversion is the
//! schema: a `FileId` is process-local and must be stamped back in at
//! load, and a `DiagnosticId` wraps a `'static` string that must be
//! re-interned through the registry. Every `to_*` conversion is total
//! except identifier re-interning, whose failure discards the entry.

use celerrate_diagnostics::{Diagnostic, Severity, find_identifier};
use celerrate_project::PhpVersion;
use celerrate_semantics::{
    AstId, Declaration, DeclarationKind, ImportKind, ItemTree, ResolutionAnswer, ResolutionRecord,
    SymbolSpace, UseImport,
};
use celerrate_source::{FileId, TextRange, TextSize};
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
    pub fn of(diagnostic: &Diagnostic) -> Self {
        Self {
            id: diagnostic.id.as_str().to_owned(),
            severity: match diagnostic.severity {
                Severity::Warning => StoredSeverity::Warning,
                Severity::Error => StoredSeverity::Error,
            },
            start: diagnostic.range.start().into(),
            end: diagnostic.range.end().into(),
            message: diagnostic.message.clone(),
        }
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
        Some(Diagnostic {
            id: find_identifier(&self.id)?,
            severity: match self.severity {
                StoredSeverity::Warning => Severity::Warning,
                StoredSeverity::Error => Severity::Error,
            },
            file,
            range: TextRange::new(TextSize::from(self.start), TextSize::from(self.end)),
            message: self.message.clone(),
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

/// One reported file's persisted verdict: its composed diagnostics and
/// the records that must revalidate before they may speak again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredVerdict {
    pub diagnostics: Vec<StoredDiagnostic>,
    pub records: Vec<StoredRecord>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_diagnostics::{Diagnostic, DiagnosticId, Severity};
    use celerrate_semantics::{ItemTree, ResolutionAnswer};
    use celerrate_source::{FileId, TextRange, TextSize};
    use celerrate_stubs::{StubAvailability, StubDeprecation};

    use super::{StoredAnswer, StoredDiagnostic, StoredItemTree, StoredRecord, StoredSeverity};

    fn parsed_tree(file: u32, source: &str) -> ItemTree {
        let parse = celerrate_syntax::parse(source);
        ItemTree::from_root(FileId::new(file), &parse.tree())
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
    fn a_diagnostic_round_trips_and_an_unknown_identifier_is_rejected() {
        let original = Diagnostic {
            id: DiagnosticId::new("CEL0018"),
            severity: Severity::Error,
            file: FileId::new(3),
            range: TextRange::new(TextSize::from(5), TextSize::from(12)),
            message: "unknown class Missing".to_owned(),
        };
        let stored = StoredDiagnostic::of(&original);
        let remapped = stored.to_diagnostic(FileId::new(9), 100).unwrap();
        assert_eq!(remapped.id, original.id);
        assert_eq!(remapped.severity, original.severity);
        assert_eq!(remapped.file, FileId::new(9));
        assert_eq!(remapped.range, original.range);
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
            diagnostic.range,
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
