//! Class-generic delivery: `new C(...)` solving class templates from
//! constructor arguments (`constructor_solved_class`), and inline
//! `@var` docblocks binding declared types into the environment.

use super::*;

impl<'db> Walker<'db, '_, '_> {
    /// Constructor inference: `new Foo(...)`
    /// solves `Foo`'s class-level templates from its `__construct`
    /// parameters, reusing `solver_pairs`/`solve` exactly as an
    /// ordinary call does — the constructor is simply the call whose
    /// result is the class itself rather than a declared return.
    /// `of` is the plain class already resolved for `class` (a
    /// concrete class name, not a placeholder — `new self()`/`new
    /// static()` resolved to their owner before this runs); returned
    /// unchanged when it names no class, `class_annotations` finds no
    /// `@template`, or the call passes no arguments.
    ///
    /// Bound under the class's *own* scope key — `class_annotations`'s
    /// own convention (the bare class key, no member suffix) — because
    /// that is exactly the scope `member_boundary_type`'s
    /// receiver-argument zip binds a receiver's `class_arguments`
    /// against later (it keys on the receiver's class name alone);
    /// solving under any other scope (the constructor's own
    /// `<class>::__construct` member scope, say) would silently never
    /// match there, and every solved argument would fall through to
    /// its bound then `mixed`.
    ///
    /// The `any_bound` guard keeps an unconstrained `new Box()` the
    /// plain class `of` already is, rather than minting a `Box<mixed>`
    /// spelling of the same thing — the canonical receiver everywhere
    /// else in the corpus.
    pub(super) fn constructor_solved_class(
        &self,
        of: TypeId<'db>,
        arguments: &[celerrate_semantics::CallArgument],
        argument_types: &[TypeId<'db>],
    ) -> TypeId<'db> {
        let db = self.db();
        if arguments.is_empty() {
            return of;
        }
        let Some(key) = of.class_name(db) else {
            return of;
        };
        let templates = &crate::inheritance::class_annotations(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            ClassQuery::new(db, key.clone()),
        )
        .templates;
        if templates.is_empty() {
            return of;
        }
        let Some(signature) = declared_member_signature(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            MemberQuery::new(
                db,
                key.clone(),
                MemberKind::Method,
                folded_member_key(MemberKind::Method, "__construct"),
            ),
        ) else {
            return of;
        };
        let pairs = self.solver_pairs(&signature.parameters, arguments, argument_types);
        let solved = crate::solver::solve(
            db,
            self.context.files,
            self.context.stubs,
            self.context.configuration,
            &pairs,
        );
        let mut any_bound = false;
        let arguments: Vec<TypeId<'db>> = templates
            .iter()
            .map(|template| match solved.binding(&key, &template.name) {
                Some(argument) => {
                    any_bound = true;
                    argument
                }
                None => template.bound.unwrap_or_else(|| TypeId::mixed(db)),
            })
            .collect();
        if any_bound {
            TypeId::class(db, &key, arguments)
        } else {
            of
        }
    }

    /// The named inline `@var` entries one docblock text
    /// parses to, at the body's own declaring scope: `Collection<User>
    /// $c` and the like, read straight from `ParsedAnnotations.variables`
    /// — never from trivia or a syntax tree, `text` already came from
    /// `BodyIr.annotations`. Mirrors `member_annotations`'s own site
    /// construction (`declared.rs`), except the declaring scope is the
    /// body's own (`FlowContext::scope_key`), not a member looked up
    /// fresh by key.
    fn inline_variables(&self, text: &str) -> Vec<(String, TypeId<'db>)> {
        let db = self.db();
        let tables = &self.context.tables;
        let site = crate::declared::NameSite::Source {
            namespace: &self.context.namespace,
            tables,
        };
        let owner_docblock = self.context.owner_class_key.as_deref().and_then(|owner| {
            crate::declared::owner_class_docblock(
                db,
                self.context.files,
                crate::declared::class_like_query(db, owner),
            )
        });
        let context = crate::type_syntax::AnnotationContext {
            declaring_scope: &self.context.scope_key,
            enclosing_class_scope: self.context.owner_class_key.as_deref(),
            enclosing_class_docblock: owner_docblock.as_deref(),
        };
        crate::type_syntax::annotations_for_docblock(db, &site, &context, text).variables
    }

    /// Binds every named inline `@var` entry anchored at `anchor` into
    /// `environment`: `None` for a body-entry
    /// declaration (bound once, before any statement), `Some(id)` for
    /// one anchored to statement `id` — the caller applies this both
    /// immediately before and immediately after walking that
    /// statement, so the declaration survives the statement's own
    /// assignment.
    pub(super) fn bind_inline_variables(
        &self,
        anchor: Option<StatementId>,
        environment: &mut Environment<'db>,
    ) {
        let Some(texts) = self.inline_variable_texts.get(&anchor).cloned() else {
            return;
        };
        for text in texts {
            for (name, of) in self.inline_variables(text) {
                environment.bind(NarrowingSubject::Local { name }, of);
            }
        }
    }
}
