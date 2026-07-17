//! The nullability family: dereferencing a possibly-null receiver
//! (design section 8). A later task fills this walker in; today it
//! finds nothing.

use super::{CheckContext, TypedVerdict};

pub(crate) fn check(_context: &CheckContext<'_, '_>, _verdicts: &mut Vec<TypedVerdict>) {}
