//! The unknown-member family: unknown method, unknown property, unknown
//! class constant, unknown enum case (design section 8). Task 4 fills
//! this walker in; today it finds nothing.

use super::{CheckContext, TypedVerdict};

pub(crate) fn check(_context: &CheckContext<'_, '_>, _verdicts: &mut Vec<TypedVerdict>) {}
