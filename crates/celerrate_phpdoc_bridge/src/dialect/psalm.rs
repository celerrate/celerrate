//! The Psalm dialect: tags with PHPStan-coincident semantics are
//! synonyms, fully honored; the genuinely divergent behaviors are the
//! enumerated ignored bucket: parsed, ignored
//! without error, traced as debt toward a later complement.

use super::{ClassifiedTag, TagRole, TagTier};
use celerrate_plugin::AssertionPolarity;

pub(crate) fn classify(name: &str) -> Option<ClassifiedTag> {
    let bare = name.strip_prefix("psalm-")?;
    let role = match bare {
        "param" => TagRole::Param,
        "return" => TagRole::Return,
        "var" => TagRole::Var,
        "property" | "property-read" | "property-write" => TagRole::Property,
        "method" => TagRole::Method,
        "extends" | "template-extends" => TagRole::Extends,
        "implements" | "template-implements" => TagRole::Implements,
        "use" | "template-use" => TagRole::UseTrait,
        "template" | "template-covariant" | "template-contravariant" => TagRole::Template,
        "assert" => TagRole::Assert(AssertionPolarity::Always),
        "assert-if-true" => TagRole::Assert(AssertionPolarity::IfTrue),
        "assert-if-false" => TagRole::Assert(AssertionPolarity::IfFalse),
        // The enumerated ignored-divergent bucket: purity, taint, and
        // the Psalm-specific `this` refinements.
        "pure"
        | "mutation-free"
        | "immutable"
        | "external-mutation-free"
        | "taint-source"
        | "taint-sink"
        | "taint-escape"
        | "taint-unescape"
        | "taint-specialize"
        | "flow"
        | "if-this-is"
        | "this-out"
        | "self-out" => TagRole::Ignored,
        _ => return None,
    };
    Some(ClassifiedTag {
        role,
        tier: TagTier::PsalmPrefixed,
    })
}
