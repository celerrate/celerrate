//! The sealed type facade: the one surface through which plugins
//! construct and interrogate types. Owns the database reference
//! privately — the native embodiment of the WASM host-interface
//! families (construction, interrogation), sketch sections 6 and 7.

use crate::representation::{CallableParameter, ShapeField, TypeId};

/// The sealed facade plugins construct and interrogate types through.
/// `Copy` and `'db`-bound: implementations are `'static`
/// (`Arc<dyn Trait>`), so retaining one in plugin state is a compile
/// error — "never retain" is structural, not reviewed. The surface is
/// exactly what the first-party plugins consume (the YAGNI criterion
/// of the design); a new need extends the facade, never bypasses it.
#[derive(Clone, Copy)]
pub struct TypeContext<'db> {
    db: &'db dyn salsa::Database,
}

impl<'db> TypeContext<'db> {
    /// Constructed only by the engine's dispatch and consumption
    /// points. No accessor returns the database.
    #[allow(dead_code)]
    pub(crate) fn new(db: &'db dyn salsa::Database) -> Self {
        Self { db }
    }

    // --- Construction: atoms ---
    pub fn mixed(self) -> TypeId<'db> {
        TypeId::mixed(self.db)
    }
    pub fn never(self) -> TypeId<'db> {
        TypeId::never(self.db)
    }
    pub fn null(self) -> TypeId<'db> {
        TypeId::null(self.db)
    }
    pub fn object(self) -> TypeId<'db> {
        TypeId::object(self.db)
    }
    pub fn resource(self) -> TypeId<'db> {
        TypeId::resource(self.db)
    }
    pub fn bool(self) -> TypeId<'db> {
        TypeId::bool(self.db)
    }
    pub fn int(self) -> TypeId<'db> {
        TypeId::int(self.db)
    }
    pub fn float(self) -> TypeId<'db> {
        TypeId::float(self.db)
    }
    pub fn string(self) -> TypeId<'db> {
        TypeId::string(self.db)
    }
    pub fn non_empty_string(self) -> TypeId<'db> {
        TypeId::non_empty_string(self.db)
    }
    pub fn numeric_string(self) -> TypeId<'db> {
        TypeId::numeric_string(self.db)
    }
    pub fn literal_string_type(self) -> TypeId<'db> {
        TypeId::literal_string_type(self.db)
    }
    pub fn static_placeholder(self) -> TypeId<'db> {
        TypeId::static_placeholder(self.db)
    }

    // --- Construction: literals and ranges ---
    pub fn bool_literal(self, value: bool) -> TypeId<'db> {
        TypeId::bool_literal(self.db, value)
    }
    pub fn int_literal(self, value: i64) -> TypeId<'db> {
        TypeId::int_literal(self.db, value)
    }
    pub fn int_range(self, minimum: Option<i64>, maximum: Option<i64>) -> TypeId<'db> {
        TypeId::int_range(self.db, minimum, maximum)
    }
    pub fn float_literal(self, value: f64) -> TypeId<'db> {
        TypeId::float_literal(self.db, value)
    }
    pub fn string_literal(self, value: &str) -> TypeId<'db> {
        TypeId::string_literal(self.db, value)
    }

    // --- Construction: composites ---
    pub fn union(self, constituents: impl IntoIterator<Item = TypeId<'db>>) -> TypeId<'db> {
        TypeId::union(self.db, constituents)
    }
    pub fn intersection(self, intersectands: impl IntoIterator<Item = TypeId<'db>>) -> TypeId<'db> {
        TypeId::intersection(self.db, intersectands)
    }
    pub fn array(self, key: TypeId<'db>, value: TypeId<'db>) -> TypeId<'db> {
        TypeId::array(self.db, key, value)
    }
    pub fn non_empty_array(self, key: TypeId<'db>, value: TypeId<'db>) -> TypeId<'db> {
        TypeId::non_empty_array(self.db, key, value)
    }
    pub fn list(self, value: TypeId<'db>) -> TypeId<'db> {
        TypeId::list(self.db, value)
    }
    pub fn non_empty_list(self, value: TypeId<'db>) -> TypeId<'db> {
        TypeId::non_empty_list(self.db, value)
    }
    pub fn shape(self, fields: Vec<ShapeField<'db>>) -> TypeId<'db> {
        TypeId::shape(self.db, fields)
    }
    pub fn iterable(self, key: TypeId<'db>, value: TypeId<'db>) -> TypeId<'db> {
        TypeId::iterable(self.db, key, value)
    }
    pub fn callable(
        self,
        parameters: Vec<CallableParameter<'db>>,
        return_type: TypeId<'db>,
    ) -> TypeId<'db> {
        TypeId::callable(self.db, parameters, return_type)
    }

    // --- Construction: classes, templates, type operators ---
    pub fn class(self, name: &str, arguments: Vec<TypeId<'db>>) -> TypeId<'db> {
        TypeId::class(self.db, name, arguments)
    }
    pub fn class_string(self, argument: Option<TypeId<'db>>) -> TypeId<'db> {
        TypeId::class_string(self.db, argument)
    }
    pub fn template(self, scope: &str, name: &str, bound: TypeId<'db>) -> TypeId<'db> {
        TypeId::template(self.db, scope, name, bound)
    }
    pub fn key_of(self, subject: TypeId<'db>) -> TypeId<'db> {
        TypeId::key_of(self.db, subject)
    }
    pub fn value_of(self, subject: TypeId<'db>) -> TypeId<'db> {
        TypeId::value_of(self.db, subject)
    }
    pub fn conditional(
        self,
        subject: TypeId<'db>,
        matches: TypeId<'db>,
        then_branch: TypeId<'db>,
        otherwise_branch: TypeId<'db>,
        negated: bool,
    ) -> TypeId<'db> {
        TypeId::conditional(
            self.db,
            subject,
            matches,
            then_branch,
            otherwise_branch,
            negated,
        )
    }

    // --- Interrogation ---
    pub fn is_null(self, subject: TypeId<'db>) -> bool {
        subject.is_null(self.db)
    }
    pub fn is_list(self, subject: TypeId<'db>) -> bool {
        subject.is_list(self.db)
    }
    pub fn bool_literal_value(self, subject: TypeId<'db>) -> Option<bool> {
        subject.bool_literal_value(self.db)
    }
    pub fn int_literal_value(self, subject: TypeId<'db>) -> Option<i64> {
        subject.int_literal_value(self.db)
    }
    pub fn float_literal_value(self, subject: TypeId<'db>) -> Option<f64> {
        subject.float_literal_value(self.db)
    }
    pub fn string_literal_value(self, subject: TypeId<'db>) -> Option<String> {
        subject.string_literal_value(self.db)
    }
    pub fn constituents(self, subject: TypeId<'db>) -> Vec<TypeId<'db>> {
        subject.constituents(self.db)
    }
    pub fn array_key(self, subject: TypeId<'db>) -> Option<TypeId<'db>> {
        subject.array_key(self.db)
    }
    pub fn array_value(self, subject: TypeId<'db>) -> Option<TypeId<'db>> {
        subject.array_value(self.db)
    }
    pub fn class_name(self, subject: TypeId<'db>) -> Option<String> {
        subject.class_name(self.db)
    }
    pub fn class_arguments(self, subject: TypeId<'db>) -> Vec<TypeId<'db>> {
        subject.class_arguments(self.db)
    }
    pub fn callable_return(self, subject: TypeId<'db>) -> Option<TypeId<'db>> {
        subject.callable_return(self.db)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use super::TypeContext;
    use crate::representation::TypeId;

    #[test]
    fn construction_delegates_to_the_type_id_builders() {
        let db = TestDatabase::default();
        let context = TypeContext::new(&db);
        // One spot check per construction shape: atom, literal,
        // parameterized, aggregate.
        assert_eq!(context.int(), TypeId::int(&db));
        assert_eq!(
            context.string_literal("active"),
            TypeId::string_literal(&db, "active")
        );
        assert_eq!(
            context.list(context.string()),
            TypeId::list(&db, TypeId::string(&db))
        );
        assert_eq!(
            context.union([context.int(), context.null()]),
            TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)])
        );
        assert_eq!(
            context.class("App\\User", Vec::new()),
            TypeId::class(&db, "App\\User", Vec::new())
        );
    }

    #[test]
    fn interrogation_delegates_to_the_type_id_queries() {
        let db = TestDatabase::default();
        let context = TypeContext::new(&db);
        let int_literal = context.int_literal(42);
        assert_eq!(context.int_literal_value(int_literal), Some(42));
        assert!(context.is_null(context.null()));
        assert_eq!(
            context.class_name(context.class("App\\User", Vec::new())),
            Some("app\\user".to_owned())
        );
        let union = context.union([context.int(), context.null()]);
        assert_eq!(context.constituents(union).len(), 2);
    }

    #[test]
    fn the_context_is_copy_so_helpers_can_pass_it_by_value() {
        let db = TestDatabase::default();
        let context = TypeContext::new(&db);
        let copy = context;
        assert_eq!(context.int(), copy.int());
    }
}
