//! The array family. Every handler is a pure projection over the
//! invocation's argument types; `None` falls through to the
//! declared tier (conservative silence).

use celerrate_plugin::{TypeId, salsa};

/// `current`/`reset`/`end`: the value projection with the `false`
/// miss. Arrays and lists answer their value type; shapes union
/// their field values; anything else is `None`.
pub(crate) fn pointer_value<'db>(
    db: &'db dyn salsa::Database,
    arguments: &[TypeId<'db>],
) -> Option<TypeId<'db>> {
    let subject = arguments.first()?;
    let value = array_value_of(db, *subject)?;
    Some(TypeId::union(db, [value, TypeId::bool_literal(db, false)]))
}

/// The value type of an array-like subject, `None` when unknown.
///
/// Adjudicated resolution (tasks 6/7 defect, closed): the lattice's
/// `array_value` already answers for `TypeData::Shape` through
/// `shape_as_array` (`construction.rs`'s `array_value`/`array_key`,
/// backed by `shape_as_array`), which unions the field values
/// exactly as a hand-rolled shape projection would. A second,
/// duplicated projection over `shape_fields` here would therefore
/// be unreachable dead code, so this helper is reduced to the
/// single lattice call. An empty shape (`[]`) is not "unknown": its
/// value union is `never` (the union of zero field values), and the
/// caller's `false`-miss union collapses `never|false` to the
/// concrete `false` literal, which matches real PHP semantics for
/// `current([])`. No explicit empty-shape guard is added: the
/// natural `false` answer is the intended, correct one, not a
/// symptom worth suppressing back into `None`.
pub(crate) fn array_value_of<'db>(
    db: &'db dyn salsa::Database,
    subject: TypeId<'db>,
) -> Option<TypeId<'db>> {
    subject.array_value(db)
}
