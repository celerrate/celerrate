//! Constructors and interrogation methods: the only way in and out of
//! the lattice. Every constructor canonicalizes before interning.

use crate::ordering::structural_order;
use crate::representation::{
    CallableParameter, FloatBits, ShapeField, ShapeKey, StringConstraint, TypeData, TypeId,
};
use celerrate_semantics::{SymbolSpace, folded_symbol_key};

impl<'db> TypeId<'db> {
    fn intern(db: &'db dyn salsa::Database, data: TypeData<'db>) -> Self {
        Self::new(db, data)
    }

    pub fn mixed(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Mixed)
    }

    pub fn never(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Never)
    }

    pub fn void(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Void)
    }

    pub fn null(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Null)
    }

    pub fn object(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Object)
    }

    pub fn resource(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Resource)
    }

    pub fn bool(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Bool { literal: None })
    }

    pub fn bool_literal(db: &'db dyn salsa::Database, value: bool) -> Self {
        Self::intern(
            db,
            TypeData::Bool {
                literal: Some(value),
            },
        )
    }

    pub fn int(db: &'db dyn salsa::Database) -> Self {
        Self::int_range(db, None, None)
    }

    pub fn int_literal(db: &'db dyn salsa::Database, value: i64) -> Self {
        Self::int_range(db, Some(value), Some(value))
    }

    /// The unified integer representation: `int` is the unbounded
    /// range, a literal is a singleton, an inverted range is `never`.
    pub fn int_range(
        db: &'db dyn salsa::Database,
        minimum: Option<i64>,
        maximum: Option<i64>,
    ) -> Self {
        if let (Some(low), Some(high)) = (minimum, maximum)
            && low > high
        {
            return Self::never(db);
        }
        Self::intern(db, TypeData::Int { minimum, maximum })
    }

    pub fn float(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::Float { literal: None })
    }

    pub fn float_literal(db: &'db dyn salsa::Database, value: f64) -> Self {
        Self::intern(
            db,
            TypeData::Float {
                literal: Some(FloatBits::from_value(value)),
            },
        )
    }

    pub fn string(db: &'db dyn salsa::Database) -> Self {
        Self::intern(
            db,
            TypeData::String {
                constraint: StringConstraint::General,
            },
        )
    }

    pub fn non_empty_string(db: &'db dyn salsa::Database) -> Self {
        Self::intern(
            db,
            TypeData::String {
                constraint: StringConstraint::NonEmpty,
            },
        )
    }

    pub fn numeric_string(db: &'db dyn salsa::Database) -> Self {
        Self::intern(
            db,
            TypeData::String {
                constraint: StringConstraint::Numeric,
            },
        )
    }

    pub fn literal_string_type(db: &'db dyn salsa::Database) -> Self {
        Self::intern(
            db,
            TypeData::String {
                constraint: StringConstraint::LiteralMarker,
            },
        )
    }

    pub fn string_literal(db: &'db dyn salsa::Database, value: &str) -> Self {
        Self::intern(
            db,
            TypeData::String {
                constraint: StringConstraint::Literal(value.to_owned()),
            },
        )
    }

    pub fn is_mixed(self, db: &'db dyn salsa::Database) -> bool {
        matches!(self.data(db), TypeData::Mixed)
    }

    pub fn is_never(self, db: &'db dyn salsa::Database) -> bool {
        matches!(self.data(db), TypeData::Never)
    }

    pub fn is_null(self, db: &'db dyn salsa::Database) -> bool {
        matches!(self.data(db), TypeData::Null)
    }

    pub fn is_void(self, db: &'db dyn salsa::Database) -> bool {
        matches!(self.data(db), TypeData::Void)
    }

    pub fn bool_literal_value(self, db: &'db dyn salsa::Database) -> Option<bool> {
        match self.data(db) {
            TypeData::Bool { literal } => *literal,
            _ => None,
        }
    }

    pub fn int_literal_value(self, db: &'db dyn salsa::Database) -> Option<i64> {
        match self.data(db) {
            TypeData::Int {
                minimum: Some(low),
                maximum: Some(high),
            } if low == high => Some(*low),
            _ => None,
        }
    }

    pub fn int_bounds(self, db: &'db dyn salsa::Database) -> Option<(Option<i64>, Option<i64>)> {
        match self.data(db) {
            TypeData::Int { minimum, maximum } => Some((*minimum, *maximum)),
            _ => None,
        }
    }

    pub fn float_literal_value(self, db: &'db dyn salsa::Database) -> Option<f64> {
        match self.data(db) {
            TypeData::Float { literal } => literal.map(FloatBits::value),
            _ => None,
        }
    }

    pub fn string_literal_value(self, db: &'db dyn salsa::Database) -> Option<String> {
        match self.data(db) {
            TypeData::String {
                constraint: StringConstraint::Literal(value),
            } => Some(value.clone()),
            _ => None,
        }
    }

    /// The canonical union: flatten, drop `never`, absorb into `mixed`,
    /// deduplicate, collapse the `true`/`false` pair, sort structurally,
    /// unwrap singletons. No subsumption elimination (recorded scope
    /// decision): `int|int<1,3>` keeps both constituents.
    pub fn union(
        db: &'db dyn salsa::Database,
        constituents: impl IntoIterator<Item = TypeId<'db>>,
    ) -> Self {
        let mut flat: Vec<TypeId<'db>> = Vec::new();
        for constituent in constituents {
            let constituent = crate::widening::capped_child(db, constituent);
            match constituent.data(db) {
                TypeData::Mixed => return Self::mixed(db),
                TypeData::Never => {}
                TypeData::Union {
                    constituents: nested,
                } => flat.extend(nested.iter().copied()),
                _ => flat.push(constituent),
            }
        }
        let true_type = Self::bool_literal(db, true);
        let false_type = Self::bool_literal(db, false);
        if flat.contains(&true_type) && flat.contains(&false_type) {
            flat.retain(|part| *part != true_type && *part != false_type);
            flat.push(Self::bool(db));
        }
        flat.sort_by(|left, right| structural_order(db, *left, *right));
        flat.dedup();
        // cap point: an oversized union collapses to its join, a common
        // supertype, never a truncated subset (order independence).
        if flat.len() > crate::widening::UNION_ARITY_CAP {
            return flat
                .iter()
                .copied()
                .fold(Self::never(db), |accumulated, part| {
                    crate::widening::join(db, accumulated, part)
                });
        }
        match flat.len() {
            0 => Self::never(db),
            1 => flat.swap_remove(0),
            _ => Self::intern(db, TypeData::Union { constituents: flat }),
        }
    }

    /// The canonical intersection: the dual rules (`mixed` disappears,
    /// `never` absorbs).
    pub fn intersection(
        db: &'db dyn salsa::Database,
        intersectands: impl IntoIterator<Item = TypeId<'db>>,
    ) -> Self {
        let mut flat: Vec<TypeId<'db>> = Vec::new();
        for intersectand in intersectands {
            let intersectand = crate::widening::capped_child(db, intersectand);
            match intersectand.data(db) {
                TypeData::Never => return Self::never(db),
                TypeData::Mixed => {}
                TypeData::Intersection {
                    intersectands: nested,
                } => {
                    flat.extend(nested.iter().copied());
                }
                _ => flat.push(intersectand),
            }
        }
        flat.sort_by(|left, right| structural_order(db, *left, *right));
        flat.dedup();
        // cap point: an oversized intersection truncates (sorted, so
        // deterministic; dropping intersectands over-approximates soundly).
        flat.truncate(crate::widening::UNION_ARITY_CAP);
        match flat.len() {
            0 => Self::mixed(db),
            1 => flat.swap_remove(0),
            _ => Self::intern(
                db,
                TypeData::Intersection {
                    intersectands: flat,
                },
            ),
        }
    }

    pub fn contains_null(self, db: &'db dyn salsa::Database) -> bool {
        match self.data(db) {
            TypeData::Null => true,
            TypeData::Union { constituents } => constituents.iter().any(|part| part.is_null(db)),
            _ => false,
        }
    }

    /// The type with `null` removed; `null` alone becomes `never`.
    pub fn without_null(self, db: &'db dyn salsa::Database) -> TypeId<'db> {
        match self.data(db) {
            TypeData::Null => Self::never(db),
            TypeData::Union { constituents } => Self::union(
                db,
                constituents
                    .iter()
                    .copied()
                    .filter(|part| !part.is_null(db)),
            ),
            _ => self,
        }
    }

    /// Union constituents; any other type answers itself as a singleton.
    pub fn constituents(self, db: &'db dyn salsa::Database) -> Vec<TypeId<'db>> {
        match self.data(db) {
            TypeData::Union { constituents } => constituents.clone(),
            _ => vec![self],
        }
    }

    /// Intersection parts; any other type answers itself as a singleton.
    pub fn intersectands(self, db: &'db dyn salsa::Database) -> Vec<TypeId<'db>> {
        match self.data(db) {
            TypeData::Intersection { intersectands } => intersectands.clone(),
            _ => vec![self],
        }
    }

    pub fn array(db: &'db dyn salsa::Database, key: TypeId<'db>, value: TypeId<'db>) -> Self {
        let key = crate::widening::capped_child(db, key);
        let value = crate::widening::capped_child(db, value);
        Self::intern(
            db,
            TypeData::Array {
                key,
                value,
                is_list: false,
                non_empty: false,
            },
        )
    }

    pub fn non_empty_array(
        db: &'db dyn salsa::Database,
        key: TypeId<'db>,
        value: TypeId<'db>,
    ) -> Self {
        let key = crate::widening::capped_child(db, key);
        let value = crate::widening::capped_child(db, value);
        Self::intern(
            db,
            TypeData::Array {
                key,
                value,
                is_list: false,
                non_empty: true,
            },
        )
    }

    pub fn list(db: &'db dyn salsa::Database, value: TypeId<'db>) -> Self {
        let key = Self::int(db);
        let value = crate::widening::capped_child(db, value);
        Self::intern(
            db,
            TypeData::Array {
                key,
                value,
                is_list: true,
                non_empty: false,
            },
        )
    }

    pub fn non_empty_list(db: &'db dyn salsa::Database, value: TypeId<'db>) -> Self {
        let key = Self::int(db);
        let value = crate::widening::capped_child(db, value);
        Self::intern(
            db,
            TypeData::Array {
                key,
                value,
                is_list: true,
                non_empty: true,
            },
        )
    }

    /// A sealed shape: duplicate keys keep the last occurrence (PHP
    /// array-literal write semantics), fields sort by key.
    pub fn shape(db: &'db dyn salsa::Database, fields: Vec<ShapeField<'db>>) -> Self {
        let mut deduplicated: Vec<ShapeField<'db>> = Vec::with_capacity(fields.len());
        for mut field in fields {
            field.value = crate::widening::capped_child(db, field.value);
            if let Some(existing) = deduplicated
                .iter_mut()
                .find(|candidate| candidate.key == field.key)
            {
                *existing = field;
            } else {
                deduplicated.push(field);
            }
        }
        deduplicated.sort_by(|left, right| left.key.cmp(&right.key));
        Self::intern(
            db,
            TypeData::Shape {
                fields: deduplicated,
            },
        )
    }

    /// The general array form of a shape: the judgment's and the
    /// interrogation's shared widening.
    pub(crate) fn shape_as_array(
        db: &'db dyn salsa::Database,
        fields: &[ShapeField<'db>],
    ) -> (TypeId<'db>, TypeId<'db>, bool, bool) {
        let key = Self::union(
            db,
            fields.iter().map(|field| match &field.key {
                ShapeKey::Integer(value) => Self::int_literal(db, *value),
                ShapeKey::String(value) => Self::string_literal(db, value),
            }),
        );
        let value = Self::union(db, fields.iter().map(|field| field.value));
        let non_empty = fields.iter().any(|field| !field.optional);
        let integer_keys: Option<Vec<i64>> = fields
            .iter()
            .map(|field| match &field.key {
                ShapeKey::Integer(value) => Some(*value),
                ShapeKey::String(_) => None,
            })
            .collect();
        let consecutive = integer_keys.is_some_and(|keys| {
            keys.iter()
                .enumerate()
                .all(|(index, key)| *key == index as i64)
        });
        let optional_is_suffix = fields
            .iter()
            .position(|field| field.optional)
            .is_none_or(|first| fields.iter().skip(first).all(|field| field.optional));
        let is_list = consecutive && optional_is_suffix;
        (key, value, is_list, non_empty)
    }

    pub fn array_key(self, db: &'db dyn salsa::Database) -> Option<TypeId<'db>> {
        match self.data(db) {
            TypeData::Array { key, .. } => Some(*key),
            TypeData::Shape { fields } => Some(Self::shape_as_array(db, fields).0),
            _ => None,
        }
    }

    pub fn array_value(self, db: &'db dyn salsa::Database) -> Option<TypeId<'db>> {
        match self.data(db) {
            TypeData::Array { value, .. } => Some(*value),
            TypeData::Shape { fields } => Some(Self::shape_as_array(db, fields).1),
            _ => None,
        }
    }

    pub fn is_list(self, db: &'db dyn salsa::Database) -> bool {
        match self.data(db) {
            TypeData::Array { is_list, .. } => *is_list,
            TypeData::Shape { fields } => Self::shape_as_array(db, fields).2,
            _ => false,
        }
    }

    pub fn is_non_empty_array(self, db: &'db dyn salsa::Database) -> bool {
        match self.data(db) {
            TypeData::Array { non_empty, .. } => *non_empty,
            TypeData::Shape { fields } => Self::shape_as_array(db, fields).3,
            _ => false,
        }
    }

    pub fn shape_fields(self, db: &'db dyn salsa::Database) -> Option<Vec<ShapeField<'db>>> {
        match self.data(db) {
            TypeData::Shape { fields } => Some(fields.clone()),
            _ => None,
        }
    }

    /// A class-like type. The name strips a leading backslash, then
    /// folds, so spelling variants (fully qualified or not, any casing)
    /// intern to one type; `display` therefore renders the folded key,
    /// on purpose — canonical form must never depend on spelling. The
    /// checks layer (`celerrate_types::checks::receivers`'s
    /// `written_type_display`) recovers the originally written
    /// spelling through the symbol table when rendering diagnostics,
    /// via [`TypeId::display_with_names`].
    pub fn class(db: &'db dyn salsa::Database, name: &str, arguments: Vec<TypeId<'db>>) -> Self {
        let name = name.strip_prefix('\\').unwrap_or(name);
        let folded = folded_symbol_key(SymbolSpace::ClassLike, name);
        let arguments = arguments
            .into_iter()
            .map(|argument| crate::widening::capped_child(db, argument))
            .collect();
        Self::intern(
            db,
            TypeData::Class {
                name: folded,
                arguments,
            },
        )
    }

    /// An enum case. The enum name strips a leading backslash, then
    /// folds, the same as [`TypeId::class`]; the case name stays
    /// verbatim (PHP case names are case-sensitive identifiers, not a
    /// namespaced symbol).
    pub fn enum_case(db: &'db dyn salsa::Database, enum_name: &str, case_name: &str) -> Self {
        let enum_name = enum_name.strip_prefix('\\').unwrap_or(enum_name);
        let folded = folded_symbol_key(SymbolSpace::ClassLike, enum_name);
        Self::intern(
            db,
            TypeData::EnumCase {
                enum_name: folded,
                case_name: case_name.to_owned(),
            },
        )
    }

    pub fn class_string(db: &'db dyn salsa::Database, argument: Option<TypeId<'db>>) -> Self {
        let argument = argument.map(|argument| crate::widening::capped_child(db, argument));
        Self::intern(db, TypeData::ClassString { argument })
    }

    pub fn static_placeholder(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::StaticPlaceholder)
    }

    pub fn self_placeholder(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::SelfPlaceholder)
    }

    pub fn parent_placeholder(db: &'db dyn salsa::Database) -> Self {
        Self::intern(db, TypeData::ParentPlaceholder)
    }

    pub fn class_name(self, db: &'db dyn salsa::Database) -> Option<String> {
        match self.data(db) {
            TypeData::Class { name, .. } => Some(name.clone()),
            _ => None,
        }
    }

    pub fn class_arguments(self, db: &'db dyn salsa::Database) -> Vec<TypeId<'db>> {
        match self.data(db) {
            TypeData::Class { arguments, .. } => arguments.clone(),
            _ => Vec::new(),
        }
    }

    pub fn enum_case_parts(self, db: &'db dyn salsa::Database) -> Option<(String, String)> {
        match self.data(db) {
            TypeData::EnumCase {
                enum_name,
                case_name,
            } => Some((enum_name.clone(), case_name.clone())),
            _ => None,
        }
    }

    pub fn class_string_argument(
        self,
        db: &'db dyn salsa::Database,
    ) -> Option<Option<TypeId<'db>>> {
        match self.data(db) {
            TypeData::ClassString { argument } => Some(*argument),
            _ => None,
        }
    }

    /// `iterable<K, V>` desugared: `array<K, V>|Traversable<K, V>`
    /// (spec section 3).
    pub fn iterable(db: &'db dyn salsa::Database, key: TypeId<'db>, value: TypeId<'db>) -> Self {
        Self::union(
            db,
            [
                Self::array(db, key, value),
                Self::class(db, "Traversable", vec![key, value]),
            ],
        )
    }

    pub fn callable(
        db: &'db dyn salsa::Database,
        parameters: Vec<CallableParameter<'db>>,
        return_type: TypeId<'db>,
    ) -> Self {
        let parameters = parameters
            .into_iter()
            .map(|parameter| CallableParameter {
                parameter_type: crate::widening::capped_child(db, parameter.parameter_type),
                ..parameter
            })
            .collect();
        let return_type = crate::widening::capped_child(db, return_type);
        Self::intern(
            db,
            TypeData::Callable {
                parameters,
                return_type,
            },
        )
    }

    /// A template variable. The scope is the declaring symbol's folded
    /// key by convention (`<class key>::<member key>` or a function
    /// key); plans 3 and 4 produce it. Callers fold; this constructor
    /// stores the scope verbatim.
    pub fn template(
        db: &'db dyn salsa::Database,
        scope: &str,
        name: &str,
        bound: TypeId<'db>,
    ) -> Self {
        let bound = crate::widening::capped_child(db, bound);
        Self::intern(
            db,
            TypeData::Template {
                scope: scope.to_owned(),
                name: name.to_owned(),
                bound,
            },
        )
    }

    /// `key-of<subject>`: evaluates shapes (union of key literals) and
    /// arrays (the key type); anything else stays symbolic.
    pub fn key_of(db: &'db dyn salsa::Database, subject: TypeId<'db>) -> Self {
        match subject.data(db) {
            TypeData::Shape { fields } => Self::shape_as_array(db, fields).0,
            TypeData::Array { key, .. } => *key,
            _ => {
                let subject = crate::widening::capped_child(db, subject);
                Self::intern(db, TypeData::KeyOf { subject })
            }
        }
    }

    /// `value-of<subject>`: evaluates shapes (union of field values)
    /// and arrays (the value type); enums need member facts and stay
    /// symbolic until plan 3.
    pub fn value_of(db: &'db dyn salsa::Database, subject: TypeId<'db>) -> Self {
        match subject.data(db) {
            TypeData::Shape { fields } => Self::shape_as_array(db, fields).1,
            TypeData::Array { value, .. } => *value,
            _ => {
                let subject = crate::widening::capped_child(db, subject);
                Self::intern(db, TypeData::ValueOf { subject })
            }
        }
    }

    pub fn conditional(
        db: &'db dyn salsa::Database,
        subject: TypeId<'db>,
        matches: TypeId<'db>,
        then_branch: TypeId<'db>,
        otherwise_branch: TypeId<'db>,
        negated: bool,
    ) -> Self {
        let subject = crate::widening::capped_child(db, subject);
        let matches = crate::widening::capped_child(db, matches);
        let then_branch = crate::widening::capped_child(db, then_branch);
        let otherwise_branch = crate::widening::capped_child(db, otherwise_branch);
        Self::intern(
            db,
            TypeData::Conditional {
                subject,
                matches,
                then_branch,
                otherwise_branch,
                negated,
            },
        )
    }

    pub fn callable_return(self, db: &'db dyn salsa::Database) -> Option<TypeId<'db>> {
        match self.data(db) {
            TypeData::Callable { return_type, .. } => Some(*return_type),
            _ => None,
        }
    }

    pub fn callable_parameters(
        self,
        db: &'db dyn salsa::Database,
    ) -> Option<Vec<CallableParameter<'db>>> {
        match self.data(db) {
            TypeData::Callable { parameters, .. } => Some(parameters.clone()),
            _ => None,
        }
    }

    pub fn template_bound(self, db: &'db dyn salsa::Database) -> Option<TypeId<'db>> {
        match self.data(db) {
            TypeData::Template { bound, .. } => Some(*bound),
            _ => None,
        }
    }

    /// The deterministic, PHPStan-flavored rendering.
    pub fn display(self, db: &'db dyn salsa::Database) -> String {
        crate::display::display_type(db, self)
    }

    /// `display`, but a class or enum name tries `resolve` first,
    /// falling back to the folded key when it answers `None` — the
    /// checks layer's written-spelling recovery (decision 3), never a
    /// new public rendering surface: `resolve` is not looked up
    /// through any registry, just handed straight to the recursive
    /// walk. Its only caller today is `checks::receivers`'s own
    /// `written_type_display`, itself unread from production code
    /// until tasks 4-6 wire the walkers to call it — hence the
    /// `dead_code` allow, the same situation as `checks/mod.rs`'s
    /// `CheckContext` fields.
    #[allow(dead_code)]
    pub(crate) fn display_with_names(
        self,
        db: &'db dyn salsa::Database,
        resolve: crate::display::NameResolver<'_>,
    ) -> String {
        crate::display::display_type_resolved(db, self, Some(resolve))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use crate::{CallableParameter, ShapeField, ShapeKey, TypeId};

    #[test]
    fn atoms_intern_to_stable_identities() {
        let db = TestDatabase::default();
        assert_eq!(TypeId::mixed(&db), TypeId::mixed(&db));
        assert_eq!(TypeId::null(&db), TypeId::null(&db));
        assert_ne!(TypeId::mixed(&db), TypeId::never(&db));
        assert_ne!(TypeId::void(&db), TypeId::null(&db));
        assert_ne!(TypeId::object(&db), TypeId::resource(&db));
    }

    #[test]
    fn interrogation_answers_the_atom_kinds() {
        let db = TestDatabase::default();
        assert!(TypeId::mixed(&db).is_mixed(&db));
        assert!(TypeId::never(&db).is_never(&db));
        assert!(TypeId::null(&db).is_null(&db));
        assert!(TypeId::void(&db).is_void(&db));
        assert!(!TypeId::null(&db).is_mixed(&db));
    }

    #[test]
    fn bool_literals_are_distinct_from_general_bool() {
        let db = TestDatabase::default();
        let general = TypeId::bool(&db);
        let true_type = TypeId::bool_literal(&db, true);
        let false_type = TypeId::bool_literal(&db, false);
        assert_ne!(general, true_type);
        assert_ne!(true_type, false_type);
        assert_eq!(true_type.bool_literal_value(&db), Some(true));
        assert_eq!(general.bool_literal_value(&db), None);
    }

    #[test]
    fn integer_literals_are_singleton_ranges() {
        let db = TestDatabase::default();
        let literal = TypeId::int_literal(&db, 42);
        let singleton = TypeId::int_range(&db, Some(42), Some(42));
        assert_eq!(literal, singleton);
        assert_eq!(literal.int_literal_value(&db), Some(42));
        assert_eq!(TypeId::int(&db).int_literal_value(&db), None);
        assert_eq!(TypeId::int(&db).int_bounds(&db), Some((None, None)));
        assert_eq!(
            TypeId::int_range(&db, Some(1), None).int_bounds(&db),
            Some((Some(1), None))
        );
    }

    #[test]
    fn an_inverted_integer_range_canonicalizes_to_never() {
        let db = TestDatabase::default();
        assert_eq!(TypeId::int_range(&db, Some(5), Some(1)), TypeId::never(&db));
    }

    #[test]
    fn float_literals_intern_by_bit_pattern() {
        let db = TestDatabase::default();
        assert_eq!(
            TypeId::float_literal(&db, 3.25),
            TypeId::float_literal(&db, 3.25)
        );
        assert_ne!(TypeId::float_literal(&db, 3.25), TypeId::float(&db));
        assert_eq!(
            TypeId::float_literal(&db, 3.25).float_literal_value(&db),
            Some(3.25)
        );
        // Every NaN canonicalizes to one interned literal.
        assert_eq!(
            TypeId::float_literal(&db, f64::NAN),
            TypeId::float_literal(&db, -f64::NAN)
        );
    }

    #[test]
    fn the_string_family_is_five_distinct_types() {
        let db = TestDatabase::default();
        let all = [
            TypeId::string(&db),
            TypeId::non_empty_string(&db),
            TypeId::numeric_string(&db),
            TypeId::literal_string_type(&db),
            TypeId::string_literal(&db, "active"),
        ];
        for (index, left) in all.iter().enumerate() {
            for right in all.iter().skip(index + 1) {
                assert_ne!(left, right);
            }
        }
        assert_eq!(
            TypeId::string_literal(&db, "active").string_literal_value(&db),
            Some("active".to_owned())
        );
        assert_eq!(TypeId::string(&db).string_literal_value(&db), None);
    }

    #[test]
    fn unions_canonicalize_independently_of_construction_order() {
        let db = TestDatabase::default();
        let forward = TypeId::union(
            &db,
            [TypeId::int(&db), TypeId::string(&db), TypeId::null(&db)],
        );
        let backward = TypeId::union(
            &db,
            [TypeId::null(&db), TypeId::string(&db), TypeId::int(&db)],
        );
        assert_eq!(forward, backward);
    }

    #[test]
    fn unions_flatten_deduplicate_and_unwrap() {
        let db = TestDatabase::default();
        let nested = TypeId::union(
            &db,
            [
                TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]),
                TypeId::int(&db),
            ],
        );
        let flat = TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]);
        assert_eq!(nested, flat);
        // A singleton unwraps to its only constituent.
        assert_eq!(TypeId::union(&db, [TypeId::int(&db)]), TypeId::int(&db));
        // An empty union is never.
        assert_eq!(TypeId::union(&db, std::iter::empty()), TypeId::never(&db));
    }

    #[test]
    fn union_absorption_rules_hold() {
        let db = TestDatabase::default();
        // never disappears; mixed absorbs everything.
        assert_eq!(
            TypeId::union(&db, [TypeId::int(&db), TypeId::never(&db)]),
            TypeId::int(&db)
        );
        assert_eq!(
            TypeId::union(&db, [TypeId::int(&db), TypeId::mixed(&db)]),
            TypeId::mixed(&db)
        );
        // true|false collapses to bool.
        assert_eq!(
            TypeId::union(
                &db,
                [
                    TypeId::bool_literal(&db, true),
                    TypeId::bool_literal(&db, false)
                ]
            ),
            TypeId::bool(&db)
        );
    }

    #[test]
    fn intersections_are_the_dual() {
        let db = TestDatabase::default();
        let forward =
            TypeId::intersection(&db, [TypeId::string(&db), TypeId::non_empty_string(&db)]);
        let backward =
            TypeId::intersection(&db, [TypeId::non_empty_string(&db), TypeId::string(&db)]);
        assert_eq!(forward, backward);
        assert_eq!(
            TypeId::intersection(&db, [TypeId::int(&db), TypeId::mixed(&db)]),
            TypeId::int(&db)
        );
        assert_eq!(
            TypeId::intersection(&db, [TypeId::int(&db), TypeId::never(&db)]),
            TypeId::never(&db)
        );
        assert_eq!(
            TypeId::intersection(&db, std::iter::empty()),
            TypeId::mixed(&db)
        );
    }

    #[test]
    fn null_interrogation_walks_unions() {
        let db = TestDatabase::default();
        let nullable = TypeId::union(&db, [TypeId::int(&db), TypeId::null(&db)]);
        assert!(nullable.contains_null(&db));
        assert!(TypeId::null(&db).contains_null(&db));
        assert!(!TypeId::int(&db).contains_null(&db));
        assert_eq!(nullable.without_null(&db), TypeId::int(&db));
        assert_eq!(TypeId::null(&db).without_null(&db), TypeId::never(&db));
        assert_eq!(TypeId::int(&db).without_null(&db), TypeId::int(&db));
        assert_eq!(nullable.constituents(&db).len(), 2);
        assert_eq!(TypeId::int(&db).constituents(&db), vec![TypeId::int(&db)]);
    }

    #[test]
    fn arrays_carry_their_flags() {
        let db = TestDatabase::default();
        let general = TypeId::array(&db, TypeId::string(&db), TypeId::int(&db));
        let non_empty = TypeId::non_empty_array(&db, TypeId::string(&db), TypeId::int(&db));
        let list = TypeId::list(&db, TypeId::int(&db));
        assert_ne!(general, non_empty);
        assert_ne!(general, list);
        assert_eq!(general.array_key(&db), Some(TypeId::string(&db)));
        assert_eq!(general.array_value(&db), Some(TypeId::int(&db)));
        assert!(!general.is_list(&db));
        assert!(list.is_list(&db));
        assert_eq!(list.array_key(&db), Some(TypeId::int(&db)));
        assert!(non_empty.is_non_empty_array(&db));
        assert!(!general.is_non_empty_array(&db));
        assert!(TypeId::non_empty_list(&db, TypeId::int(&db)).is_list(&db));
    }

    #[test]
    fn shapes_sort_their_fields_and_keep_the_last_duplicate() {
        let db = TestDatabase::default();
        let forward = TypeId::shape(
            &db,
            vec![
                ShapeField {
                    key: ShapeKey::String("id".to_owned()),
                    optional: false,
                    value: TypeId::int(&db),
                },
                ShapeField {
                    key: ShapeKey::String("name".to_owned()),
                    optional: true,
                    value: TypeId::string(&db),
                },
            ],
        );
        let backward = TypeId::shape(
            &db,
            vec![
                ShapeField {
                    key: ShapeKey::String("name".to_owned()),
                    optional: true,
                    value: TypeId::string(&db),
                },
                ShapeField {
                    key: ShapeKey::String("id".to_owned()),
                    optional: false,
                    value: TypeId::int(&db),
                },
            ],
        );
        assert_eq!(forward, backward);
        let duplicated = TypeId::shape(
            &db,
            vec![
                ShapeField {
                    key: ShapeKey::String("id".to_owned()),
                    optional: false,
                    value: TypeId::string(&db),
                },
                ShapeField {
                    key: ShapeKey::String("id".to_owned()),
                    optional: false,
                    value: TypeId::int(&db),
                },
            ],
        );
        let fields = duplicated.shape_fields(&db).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields.first().unwrap().value, TypeId::int(&db));
    }

    #[test]
    fn shapes_answer_the_array_interrogation_through_their_array_form() {
        let db = TestDatabase::default();
        let shape = TypeId::shape(
            &db,
            vec![
                ShapeField {
                    key: ShapeKey::Integer(0),
                    optional: false,
                    value: TypeId::int(&db),
                },
                ShapeField {
                    key: ShapeKey::Integer(1),
                    optional: true,
                    value: TypeId::string(&db),
                },
            ],
        );
        assert_eq!(
            shape.array_value(&db),
            Some(TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)]))
        );
        assert!(shape.is_list(&db));
        assert!(shape.is_non_empty_array(&db));
        // A gap breaks the list property.
        let gapped = TypeId::shape(
            &db,
            vec![
                ShapeField {
                    key: ShapeKey::Integer(0),
                    optional: false,
                    value: TypeId::int(&db),
                },
                ShapeField {
                    key: ShapeKey::Integer(2),
                    optional: false,
                    value: TypeId::int(&db),
                },
            ],
        );
        assert!(!gapped.is_list(&db));
        // An optional field before a required one breaks it too.
        let interleaved = TypeId::shape(
            &db,
            vec![
                ShapeField {
                    key: ShapeKey::Integer(0),
                    optional: true,
                    value: TypeId::int(&db),
                },
                ShapeField {
                    key: ShapeKey::Integer(1),
                    optional: false,
                    value: TypeId::int(&db),
                },
            ],
        );
        assert!(!interleaved.is_list(&db));
    }

    #[test]
    fn an_empty_shape_is_a_possibly_empty_list() {
        let db = TestDatabase::default();
        let empty = TypeId::shape(&db, vec![]);
        assert!(empty.is_list(&db));
        assert!(!empty.is_non_empty_array(&db));
    }

    #[test]
    fn a_leading_backslash_is_stripped_before_folding() {
        let db = TestDatabase::default();
        assert_eq!(
            TypeId::class(&db, "\\App\\Entity\\User", vec![]),
            TypeId::class(&db, "App\\Entity\\User", vec![])
        );
        assert_eq!(
            TypeId::enum_case(&db, "\\App\\Status", "Active"),
            TypeId::enum_case(&db, "App\\Status", "Active")
        );
    }

    #[test]
    fn class_names_fold_at_construction() {
        let db = TestDatabase::default();
        let lower = TypeId::class(&db, "app\\entity\\user", vec![]);
        let mixed_case = TypeId::class(&db, "App\\Entity\\User", vec![]);
        assert_eq!(lower, mixed_case);
        assert_eq!(lower.class_name(&db), Some("app\\entity\\user".to_owned()));
        assert_eq!(lower.class_arguments(&db), Vec::<TypeId>::new());
    }

    #[test]
    fn generic_arguments_participate_in_identity() {
        let db = TestDatabase::default();
        let of_user = TypeId::class(&db, "Collection", vec![TypeId::class(&db, "User", vec![])]);
        let of_int = TypeId::class(&db, "Collection", vec![TypeId::int(&db)]);
        let bare = TypeId::class(&db, "Collection", vec![]);
        assert_ne!(of_user, of_int);
        assert_ne!(of_user, bare);
        assert_eq!(of_user.class_arguments(&db).len(), 1);
    }

    #[test]
    fn enum_cases_fold_the_enum_and_keep_the_case_verbatim() {
        let db = TestDatabase::default();
        assert_eq!(
            TypeId::enum_case(&db, "App\\Status", "Active"),
            TypeId::enum_case(&db, "app\\status", "Active")
        );
        assert_ne!(
            TypeId::enum_case(&db, "App\\Status", "Active"),
            TypeId::enum_case(&db, "App\\Status", "ACTIVE")
        );
        assert_eq!(
            TypeId::enum_case(&db, "App\\Status", "Active").enum_case_parts(&db),
            Some(("app\\status".to_owned(), "Active".to_owned()))
        );
    }

    #[test]
    fn class_string_carries_an_optional_argument() {
        let db = TestDatabase::default();
        let bare = TypeId::class_string(&db, None);
        let of_user = TypeId::class_string(&db, Some(TypeId::class(&db, "User", vec![])));
        assert_ne!(bare, of_user);
        assert_eq!(bare.class_string_argument(&db), Some(None));
        assert_eq!(
            of_user.class_string_argument(&db),
            Some(Some(TypeId::class(&db, "User", vec![])))
        );
    }

    #[test]
    fn the_placeholders_are_three_distinct_atoms() {
        let db = TestDatabase::default();
        assert_ne!(
            TypeId::static_placeholder(&db),
            TypeId::self_placeholder(&db)
        );
        assert_ne!(
            TypeId::self_placeholder(&db),
            TypeId::parent_placeholder(&db)
        );
    }

    #[test]
    fn iterable_desugars_to_the_spec_union() {
        let db = TestDatabase::default();
        let iterable = TypeId::iterable(&db, TypeId::string(&db), TypeId::int(&db));
        let expected = TypeId::union(
            &db,
            [
                TypeId::array(&db, TypeId::string(&db), TypeId::int(&db)),
                TypeId::class(
                    &db,
                    "Traversable",
                    vec![TypeId::string(&db), TypeId::int(&db)],
                ),
            ],
        );
        assert_eq!(iterable, expected);
    }

    #[test]
    fn callables_carry_parameters_and_return() {
        let db = TestDatabase::default();
        let callable = TypeId::callable(
            &db,
            vec![
                CallableParameter {
                    parameter_type: TypeId::int(&db),
                    optional: false,
                    variadic: false,
                    by_reference: false,
                },
                CallableParameter {
                    parameter_type: TypeId::string(&db),
                    optional: true,
                    variadic: false,
                    by_reference: false,
                },
            ],
            TypeId::bool(&db),
        );
        assert_eq!(callable.callable_return(&db), Some(TypeId::bool(&db)));
        assert_eq!(callable.callable_parameters(&db).unwrap().len(), 2);
        assert_eq!(TypeId::int(&db).callable_return(&db), None);
    }

    #[test]
    fn templates_are_scoped_lattice_citizens() {
        let db = TestDatabase::default();
        let bound = TypeId::class(&db, "FormTypeInterface", vec![]);
        let one = TypeId::template(&db, "app\\form::configure", "T", bound);
        let same = TypeId::template(&db, "app\\form::configure", "T", bound);
        let other_scope = TypeId::template(&db, "app\\other::configure", "T", bound);
        assert_eq!(one, same);
        assert_ne!(one, other_scope);
        assert_eq!(one.template_bound(&db), Some(bound));
        assert_eq!(TypeId::int(&db).template_bound(&db), None);
    }

    #[test]
    fn key_of_and_value_of_evaluate_decidable_subjects() {
        let db = TestDatabase::default();
        let shape = TypeId::shape(
            &db,
            vec![
                ShapeField {
                    key: ShapeKey::String("id".to_owned()),
                    optional: false,
                    value: TypeId::int(&db),
                },
                ShapeField {
                    key: ShapeKey::String("name".to_owned()),
                    optional: true,
                    value: TypeId::string(&db),
                },
            ],
        );
        assert_eq!(
            TypeId::key_of(&db, shape),
            TypeId::union(
                &db,
                [
                    TypeId::string_literal(&db, "id"),
                    TypeId::string_literal(&db, "name")
                ]
            )
        );
        assert_eq!(
            TypeId::value_of(&db, shape),
            TypeId::union(&db, [TypeId::int(&db), TypeId::string(&db)])
        );
        let array = TypeId::array(&db, TypeId::string(&db), TypeId::int(&db));
        assert_eq!(TypeId::key_of(&db, array), TypeId::string(&db));
        assert_eq!(TypeId::value_of(&db, array), TypeId::int(&db));
        // Undecidable subjects stay symbolic (and distinct).
        let template = TypeId::template(&db, "scope", "T", TypeId::mixed(&db));
        assert_ne!(
            TypeId::key_of(&db, template),
            TypeId::value_of(&db, template)
        );
        assert_ne!(TypeId::key_of(&db, template), template);
    }

    #[test]
    fn conditionals_stay_symbolic_and_structural() {
        let db = TestDatabase::default();
        let subject = TypeId::template(&db, "scope", "T", TypeId::mixed(&db));
        let positive = TypeId::conditional(
            &db,
            subject,
            TypeId::int(&db),
            TypeId::string(&db),
            TypeId::bool(&db),
            false,
        );
        let negated = TypeId::conditional(
            &db,
            subject,
            TypeId::int(&db),
            TypeId::string(&db),
            TypeId::bool(&db),
            true,
        );
        assert_ne!(positive, negated);
        assert_eq!(
            positive,
            TypeId::conditional(
                &db,
                subject,
                TypeId::int(&db),
                TypeId::string(&db),
                TypeId::bool(&db),
                false,
            )
        );
    }
}
