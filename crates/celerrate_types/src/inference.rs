//! Demand-driven inference: the per-body flow walk and (Task 10) the
//! interprocedural fixpoint. One query per body produces the full
//! expression type table plus the joined return type; nothing here
//! ever touches a syntax tree — the walker consumes the range-free
//! body IR, so LRU eviction of parse trees is structurally safe.
//!
//! Memory lever, named for plan 9b (design section 6): the full
//! expression type tables produced by [`inferred_body_types`] are the
//! LRU candidates (`salsa` supports `lru = N` on tracked functions),
//! while inferred returns stay resident — small, hot, and the
//! fixpoint's currency. No capacity is set here: plan 9b owns the
//! peak-memory measurement against its budget and pulls the lever
//! with a number.

use celerrate_db::{AnalyzedFileSet, SourceFile};
use celerrate_project::ProjectConfiguration;
use celerrate_semantics::{
    BodyQuery, ExpressionId, FreeFunction, Member, MemberKind, SymbolSpace, body_ir,
    folded_symbol_key, fully_qualified_name, member_tree,
};
use celerrate_stubs::StubIndexInput;

use crate::representation::TypeId;

/// The interprocedural edge classes one body's inference took, as
/// pure data: the design's residual instrument ("how many results
/// depend on *inferred* returns"), aggregated by the first
/// orchestration-layer consumer (plan 8).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, salsa::Update)]
pub struct InterproceduralEdgeCounts {
    /// Call results taken from a declared (native or annotated) return.
    pub declared_return_edges: u32,
    /// Call results taken from another body's inferred return.
    pub inferred_return_edges: u32,
    /// Call results taken from a dynamic type provider's claim.
    pub provider_edges: u32,
}

/// The inference result of one body: a type per arena expression, the
/// joined return type, and the edge-count instrument. `Eq`-comparable
/// on purpose: a body edit that leaves every inferred result identical
/// backdates, and dependents are spared.
#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub struct InferredBody<'db> {
    pub expression_types: Vec<TypeId<'db>>,
    pub return_type: TypeId<'db>,
    pub edge_counts: InterproceduralEdgeCounts,
}

impl<'db> InferredBody<'db> {
    pub fn expression_type(&self, id: ExpressionId) -> Option<TypeId<'db>> {
        self.expression_types.get(id.index() as usize).copied()
    }
}

/// The declaration a body belongs to: a free function, or a method of
/// a class-like (whose folded key is `None` for an anonymous class —
/// decision 12: no folded symbol key exists to resolve members
/// against). `Eq` so the tracked projection backdates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BodyOwner {
    Function(FreeFunction),
    Method {
        class_key: Option<String>,
        namespace: String,
        member: Member,
    },
}

/// Resolves the owning declaration of one body through the member
/// projection. `None` when the identity names no function or method
/// of `file`. A tracked query on purpose, and load-bearing for the
/// invalidation story: `member_tree` changes whenever *any* member of
/// the file changes, but this per-body projection backdates for every
/// body whose own declaration did not — so editing one signature
/// re-infers that member's body and no other (the design's harness-2
/// contract, pinned in Task 12).
#[salsa::tracked(returns(ref))]
pub(crate) fn body_owner<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<BodyOwner> {
    let ast_id = body.ast_id(db);
    let tree = member_tree(db, file);
    if let Some(function) = tree
        .functions
        .iter()
        .find(|function| function.ast_id == ast_id)
    {
        return Some(BodyOwner::Function(function.clone()));
    }
    for class in &tree.classes {
        let Some(member) = class.members.iter().find(|member| member.ast_id == ast_id) else {
            continue;
        };
        if member.kind != MemberKind::Method {
            return None;
        }
        let class_key = class.name.as_deref().map(|name| {
            folded_symbol_key(
                SymbolSpace::ClassLike,
                &fully_qualified_name(&class.namespace, name),
            )
        });
        return Some(BodyOwner::Method {
            class_key,
            namespace: class.namespace.clone(),
            member: member.clone(),
        });
    }
    None
}

/// The inference of one body: `None` when the identity carries no
/// body in `file` (mirroring `body_ir`). Task 3 replaces the
/// all-`mixed` table with the flow walk.
#[salsa::tracked(returns(ref))]
pub fn inferred_body_types<'db>(
    db: &'db dyn salsa::Database,
    files: AnalyzedFileSet,
    stubs: StubIndexInput,
    configuration: ProjectConfiguration,
    file: SourceFile,
    body: BodyQuery<'db>,
) -> Option<InferredBody<'db>> {
    let _ = (files, stubs, configuration);
    let ir = body_ir(db, file, body).as_ref()?;
    let mixed = TypeId::mixed(db);
    Some(InferredBody {
        expression_types: vec![mixed; ir.expressions.len()],
        return_type: mixed,
        edge_counts: InterproceduralEdgeCounts::default(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_db::{AnalyzedFileSet, SourceFile};
    use celerrate_project::{PhpVersion, PhpVersionRange, ProjectConfiguration};
    use celerrate_semantics::{AstId, BodyQuery, body_ir};
    use celerrate_source::FileId;
    use celerrate_stubs::{StubIndex, StubIndexInput};

    use super::{BodyOwner, body_owner, inferred_body_types};

    struct Fixture {
        db: TestDatabase,
        handles: Vec<SourceFile>,
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
        let files = AnalyzedFileSet::new(&db, handles.clone());
        let stubs = StubIndexInput::builder(StubIndex::from_symbols(vec![]))
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
            handles,
            files,
            stubs,
            configuration,
        }
    }

    /// The body of the declaration numbered `index` in file 0.
    fn body_query(fixture: &Fixture, index: u32) -> BodyQuery<'_> {
        BodyQuery::new(
            &fixture.db,
            AstId {
                file: FileId::new(0),
                index,
            },
        )
    }

    #[test]
    fn the_query_answers_a_table_sized_to_the_body_arena() {
        let fixture = fixture(&["<?php function f() { return 1 + 2; }"]);
        let file = fixture.handles[0];
        let body = body_query(&fixture, 0);
        let inferred = inferred_body_types(
            &fixture.db,
            fixture.files,
            fixture.stubs,
            fixture.configuration,
            file,
            body,
        )
        .as_ref()
        .unwrap();
        let ir = body_ir(&fixture.db, file, body).as_ref().unwrap();
        assert_eq!(inferred.expression_types.len(), ir.expressions.len());
        assert!(!ir.expressions.is_empty(), "the fixture has expressions");
    }

    #[test]
    fn a_non_body_identity_answers_none() {
        let fixture = fixture(&["<?php class A { public function m() {} }"]);
        let file = fixture.handles[0];
        // Numbering: class = 0 (no body), method = 1 (the 1a contract).
        let class = body_query(&fixture, 0);
        assert!(
            inferred_body_types(
                &fixture.db,
                fixture.files,
                fixture.stubs,
                fixture.configuration,
                file,
                class,
            )
            .is_none()
        );
    }

    #[test]
    fn body_owner_resolves_free_functions_and_methods() {
        let fixture =
            fixture(&["<?php namespace App; function f() {} class A { public function m() {} }"]);
        let file = fixture.handles[0];
        // Numbering: the namespace declaration is itself a numbered item
        // (namespace = 0, f = 1, class = 2, m = 3).
        let function = body_owner(&fixture.db, file, body_query(&fixture, 1))
            .clone()
            .unwrap();
        let BodyOwner::Function(free_function) = function else {
            panic!("expected a free function owner");
        };
        assert_eq!(free_function.name, "f");
        assert_eq!(free_function.namespace, "App");

        let method = body_owner(&fixture.db, file, body_query(&fixture, 3))
            .clone()
            .unwrap();
        let BodyOwner::Method {
            class_key, member, ..
        } = method
        else {
            panic!("expected a method owner");
        };
        assert_eq!(class_key.as_deref(), Some("app\\a"));
        assert_eq!(member.name, "m");
    }

    #[test]
    fn an_anonymous_class_method_owner_has_no_key() {
        let fixture =
            fixture(&["<?php function wrapper() { return new class { public function m() {} }; }"]);
        let file = fixture.handles[0];
        // Numbering: wrapper = 0, anonymous class = 1, method = 2.
        let owner = body_owner(&fixture.db, file, body_query(&fixture, 2))
            .clone()
            .unwrap();
        let BodyOwner::Method { class_key, .. } = owner else {
            panic!("expected a method owner");
        };
        assert!(class_key.is_none());
    }
}
