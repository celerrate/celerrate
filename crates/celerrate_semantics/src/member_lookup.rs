//! The per-member lookup: the firewall between the linearized tables
//! and their consumers, the same pattern as `lookup_symbol`. A member
//! added anywhere re-runs the affected linearization, but a lookup
//! whose answer did not change backdates, and the files that asked it
//! are spared.

use celerrate_db::AnalyzedFileSet;
use celerrate_project::ProjectConfiguration;
use celerrate_stubs::StubIndexInput;

use crate::linearize::{ClassQuery, MemberOrigin, linearized_class};
use crate::members::{Member, MemberKind};

/// One member to look up: its class (a **pre-folded** ClassLike key),
/// its kind, and its **pre-folded** member key (fold with
/// `folded_member_key`, using the queried kind, before interning — so
/// spelling and case variants of one member share one memo).
#[salsa::interned(debug)]
pub struct MemberQuery<'db> {
    #[returns(ref)]
    pub class_key: String,
    pub kind: MemberKind,
    #[returns(ref)]
    pub member_key: String,
}

/// Where one queried member resolved: the member itself, the folded key
/// of its declaring class, and its origin relative to the queried
/// class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberResolution {
    pub member: Member,
    pub owner: String,
    pub origin: MemberOrigin,
}

/// Resolves one (class, kind, member) query against the queried class's
/// linearized table: `None` when the class is not a source class-like
/// or the member is absent. The table's first entry per `(kind, key)`
/// is the precedence winner (`find` on the sorted table).
#[salsa::tracked]
pub fn lookup_member<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    query: MemberQuery<'db>,
) -> Option<MemberResolution> {
    let class = ClassQuery::new(db, query.class_key(db).clone());
    let table = linearized_class(db, files, stubs, configuration, class).as_ref()?;
    let kind = query.kind(db);
    let key = query.member_key(db);
    table
        .members
        .iter()
        .find(|entry| entry.member.kind == kind && entry.key == *key)
        .map(|entry| MemberResolution {
            member: entry.member.clone(),
            owner: entry.owner.clone(),
            origin: entry.origin,
        })
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

    use super::{MemberQuery, MemberResolution, lookup_member};
    use crate::linearize::{MemberOrigin, folded_member_key};
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

    fn lookup(
        fixture: &Fixture,
        class_written: &str,
        kind: MemberKind,
        member_written: &str,
    ) -> Option<MemberResolution> {
        let query = MemberQuery::new(
            &fixture.db,
            folded_symbol_key(SymbolSpace::ClassLike, class_written),
            kind,
            folded_member_key(kind, member_written),
        );
        lookup_member(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            query,
        )
    }

    #[test]
    fn a_member_resolves_through_the_linearized_table() {
        let fixture = fixture(&[
            "<?php class Base { public function hello() {} }",
            "<?php class Child extends Base {}",
        ]);
        let resolution = lookup(&fixture, "Child", MemberKind::Method, "HELLO").unwrap();
        assert_eq!(resolution.owner, "base");
        assert_eq!(resolution.origin, MemberOrigin::Inherited);
    }

    #[test]
    fn an_unknown_member_or_class_answers_none() {
        let fixture = fixture(&["<?php class A { public function f() {} }"]);
        assert!(lookup(&fixture, "A", MemberKind::Method, "missing").is_none());
        assert!(lookup(&fixture, "Ghost", MemberKind::Method, "f").is_none());
        // Kinds are distinct spaces: a method never answers a property.
        assert!(lookup(&fixture, "A", MemberKind::Property, "f").is_none());
    }
}
