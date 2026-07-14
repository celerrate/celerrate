//! Constructors and interrogation methods: the only way in and out of
//! the lattice. Every constructor canonicalizes before interning.

use crate::representation::{FloatBits, StringConstraint, TypeData, TypeId};

impl<'db> TypeId<'db> {
    fn intern(db: &'db dyn salsa::Database, data: TypeData) -> Self {
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
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;

    use crate::TypeId;

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
}
