//! The string family. Every handler is a pure projection over the
//! invocation's argument types; `None` falls through to the
//! declared tier (conservative silence).

use celerrate_plugin::{TypeContext, TypeId};

/// `explode(separator, subject, limit?)`: PHP always answers at
/// least `[$subject]`, so a missing limit or a positive integer
/// literal limit stays `non-empty-list<string>`; a negative literal
/// limit can empty the result (`list<string>`); a non-literal limit
/// is unknown, so the plain list is the sound answer. The answer
/// never depends on the separator's value.
pub(crate) fn explode<'db>(
    context: TypeContext<'db>,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    if arguments.len() < 2 {
        return None;
    }
    let non_empty = match arguments.get(2) {
        None => true,
        Some(limit) => match context.int_literal_value(*limit) {
            Some(value) => value >= 1,
            None => false,
        },
    };
    Some(if non_empty {
        context.non_empty_list(context.string())
    } else {
        context.list(context.string())
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use celerrate_db::testing::TestDatabase;
    use celerrate_plugin::TypeId;
    use celerrate_types::testing_type_context;

    #[test]
    fn explode_without_a_limit_answers_a_non_empty_list() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        assert_eq!(
            super::explode(context, &[TypeId::string(&db), TypeId::string(&db)]).unwrap(),
            TypeId::non_empty_list(&db, TypeId::string(&db)),
        );
    }

    #[test]
    fn explode_with_a_positive_literal_limit_stays_non_empty() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        assert_eq!(
            super::explode(
                context,
                &[
                    TypeId::string(&db),
                    TypeId::string(&db),
                    TypeId::int_literal(&db, 3),
                ],
            )
            .unwrap(),
            TypeId::non_empty_list(&db, TypeId::string(&db)),
        );
    }

    #[test]
    fn explode_with_a_negative_or_unknown_limit_answers_a_plain_list() {
        let db = TestDatabase::default();
        let context = testing_type_context(&db);
        for limit in [TypeId::int_literal(&db, -1), TypeId::int(&db)] {
            assert_eq!(
                super::explode(context, &[TypeId::string(&db), TypeId::string(&db), limit])
                    .unwrap(),
                TypeId::list(&db, TypeId::string(&db)),
            );
        }
    }
}
