//! The argument family: assignability of each call argument against its
//! parameter, arity (too few, too many), and unknown named arguments
//! (design section 8). A later task fills this walker in; today it
//! finds nothing.

use super::{CheckContext, TypedVerdict};

pub(crate) fn check(_context: &CheckContext<'_, '_>, _verdicts: &mut Vec<TypedVerdict>) {}
