//! Constructors and interrogation methods: the only way in and out of
//! the lattice. Every constructor canonicalizes before interning.

use crate::ordering::structural_order;
use crate::representation::{FloatBits, StringConstraint, TypeData, TypeId};

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
        // cap point: Task 6 collapses beyond UNION_ARITY_CAP here.
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
        // cap point: Task 6 truncates beyond UNION_ARITY_CAP here (sorted,
        // so deterministic; a sound over-approximation).
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
}
