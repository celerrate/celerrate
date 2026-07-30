//! The statically named references of one file, resolved once. Two
//! conservative stances are documented engine semantics: dynamic
//! references are out of scope, and a symbol declared anywhere in
//! project, vendor, or stubs counts as declared, no reachability
//! analysis of conditional declarations.
//!
//! No diagnostic is constructed here. The unknown-symbol family
//! (CEL0018-CEL0020) reads a [`ResolutionOutcome::Unresolved`] outcome
//! through `celerrate_rules::rules::unknown_symbols`, and the symbol
//! version-gating family (CEL0021-CEL0023) judges a
//! [`ResolutionOutcome::Stub`] outcome's availability window against
//! the project's supported range through
//! `celerrate_rules::rules::symbol_version_gating`. What moved is
//! construction, never the walk.
//!
//! [`reference_outcomes`] walks the file's references exactly once,
//! resolving each name a single time and deriving both outputs from
//! that same resolution: the plain-data outcomes the semantic-phase
//! rules consume, and the revalidation records of `revalidation.rs`.
//! [`reference_resolutions`] and
//! `crate::revalidation::resolution_records` are thin projections over
//! it: one walk produces outcomes and answers, so drift between them is
//! structurally impossible, the `composed_diagnostics` closure applied
//! to the second mirror.

use std::collections::HashMap;

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::ProjectConfiguration;
use celerrate_source::TextRange;
use celerrate_stubs::{StubAvailability, StubIndexInput};

use crate::lookup::SymbolResolution;
use crate::queries::item_tree;
use crate::references::collect_references;
use crate::resolve::{SymbolSources, UseTables, resolve_name};
use crate::revalidation::{ResolutionRecord, answer_of};
use crate::symbols::SymbolSpace;

/// How one statically named reference resolved, as plain data: the
/// outcome the semantic-phase rules consume. The version policy
/// (comparing an availability window against the supported range)
/// belongs to the rules, mirroring `GatedSyntaxUse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceOutcome {
    pub written: String,
    pub space: SymbolSpace,
    pub range: TextRange,
    pub resolution: ResolutionOutcome,
}

/// The three ways a reference resolves. `Stub` carries the window so
/// the gating rule can judge it against the project's range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionOutcome {
    /// No declaration anywhere in project, vendor, or stubs.
    Unresolved,
    /// A stub declaration, with its availability window.
    Stub { availability: StubAvailability },
    /// A source declaration (never gated).
    Source,
}

/// The outcomes and answers of one file's reference walk, produced by
/// [`reference_outcomes`] from the same pass over the same resolutions:
/// `outcomes` is the plain-data co-product the semantic-phase rules
/// consume, `records` is what `resolution_records` used to compute
/// alone. See the module doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceOutcomes {
    pub outcomes: Vec<ReferenceOutcome>,
    pub records: Vec<ResolutionRecord>,
}

/// The single walk over the statically named references of one file:
/// for every reference, `resolve_name` runs exactly once, and its
/// result feeds both outputs, the plain-data outcome the semantic-phase
/// rules consume and the revalidation record that reduces the same
/// resolution to its answer. Outcomes and records keep walk (tree)
/// order, the convention `resolution_records`' tests pin.
#[salsa::tracked(returns(ref))]
pub fn reference_outcomes(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> ReferenceOutcomes {
    let sources = SymbolSources {
        files,
        stubs,
        configuration,
    };
    let tree = item_tree(db, file);
    let root = celerrate_db::parse(db, file).tree();
    let mut tables_by_namespace: HashMap<String, UseTables> = HashMap::new();
    let mut outcomes = Vec::new();
    let mut records = Vec::new();
    for reference in collect_references(&root) {
        let tables = tables_by_namespace
            .entry(reference.namespace.clone())
            .or_insert_with(|| UseTables::for_namespace(tree, &reference.namespace));
        let resolution = resolve_name(
            db,
            sources,
            &reference.namespace,
            tables,
            &reference.written,
            reference.space,
        );
        records.push(ResolutionRecord {
            written: reference.written.clone(),
            space: reference.space,
            namespace: reference.namespace.clone(),
            answer: answer_of(resolution),
        });
        let outcome_of = |resolution: ResolutionOutcome| ReferenceOutcome {
            written: reference.written.clone(),
            space: reference.space,
            range: reference.range,
            resolution,
        };
        match resolution {
            None => {
                outcomes.push(outcome_of(ResolutionOutcome::Unresolved));
            }
            Some(SymbolResolution::Stub { availability, .. }) => {
                outcomes.push(outcome_of(ResolutionOutcome::Stub { availability }));
            }
            Some(SymbolResolution::Source { .. }) => {
                outcomes.push(outcome_of(ResolutionOutcome::Source));
            }
        }
    }
    ReferenceOutcomes { outcomes, records }
}

/// The per-reference resolution outcomes of one file, as plain data: a
/// thin, independently backdating projection over
/// [`reference_outcomes`], the sibling of
/// `crate::revalidation::resolution_records`. The semantic-phase
/// context consumes this, so the phase query's early cutoff is
/// independent of the revalidation records.
#[salsa::tracked(returns(ref))]
pub fn reference_resolutions(
    db: &dyn salsa::Database,
    file: SourceFile,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
) -> Vec<ReferenceOutcome> {
    reference_outcomes(db, file, files, stubs, configuration)
        .outcomes
        .clone()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]

    use super::*;
    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_source::FileId;
    use celerrate_stubs::{
        StubAvailability, StubIndex, StubIndexInput, StubSymbol, StubSymbolKind,
    };

    fn stub(name: &str, kind: StubSymbolKind) -> StubSymbol {
        StubSymbol {
            name: name.to_owned(),
            kind,
            availability: StubAvailability::ALWAYS,
        }
    }

    fn stub_with(name: &str, kind: StubSymbolKind, availability: StubAvailability) -> StubSymbol {
        StubSymbol {
            name: name.to_owned(),
            kind,
            availability,
        }
    }

    /// The full supported range used by tests that do not exercise
    /// version gating.
    fn full_range() -> PhpVersionRange {
        PhpVersionRange::new(PhpVersion::new(8, 1), PhpVersion::new(8, 5))
    }

    /// The outcomes of the FIRST source, with the given stubs and range.
    fn resolutions_in_range(
        sources: &[&str],
        stub_symbols: Vec<StubSymbol>,
        range: PhpVersionRange,
    ) -> Vec<ReferenceOutcome> {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        let file = *handles.first().unwrap();
        let files = AnalyzedFileSet::new(&db, handles);
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(stub_symbols))
            .durability(salsa::Durability::HIGH)
            .new(&db);
        let configuration = ProjectConfiguration::builder(range)
            .durability(salsa::Durability::MEDIUM)
            .new(&db);
        reference_resolutions(&db, file, files, stubs, configuration).clone()
    }

    #[test]
    fn the_walk_produces_one_plain_outcome_per_reference() {
        let source = "<?php namespace App; $x = new Missing(); strlen('a'); $h = new Helper();";
        let outcomes = resolutions_in_range(
            &[source, "<?php namespace App; class Helper {}"],
            vec![stub("strlen", StubSymbolKind::Function)],
            full_range(),
        );
        assert_eq!(outcomes.len(), 3, "{outcomes:?}");
        assert_eq!(outcomes[0].written, "Missing");
        assert_eq!(outcomes[0].space, SymbolSpace::ClassLike);
        assert_eq!(outcomes[0].resolution, ResolutionOutcome::Unresolved);
        let start: usize = outcomes[0].range.start().into();
        let end: usize = outcomes[0].range.end().into();
        assert_eq!(&source[start..end], "Missing");
        assert_eq!(outcomes[1].written, "strlen");
        assert!(matches!(
            outcomes[1].resolution,
            ResolutionOutcome::Stub { .. }
        ));
        assert_eq!(outcomes[2].resolution, ResolutionOutcome::Source);
    }

    #[test]
    fn a_stub_outcome_carries_its_availability_window() {
        let outcomes = resolutions_in_range(
            &["<?php json_validate('{}');"],
            vec![stub_with(
                "json_validate",
                StubSymbolKind::Function,
                StubAvailability {
                    introduced: Some(PhpVersion::new(8, 3)),
                    removed: None,
                    deprecated: None,
                },
            )],
            full_range(),
        );
        let ResolutionOutcome::Stub { availability } = &outcomes[0].resolution else {
            panic!("expected a stub outcome: {outcomes:?}");
        };
        assert_eq!(availability.introduced, Some(PhpVersion::new(8, 3)));
    }
}
