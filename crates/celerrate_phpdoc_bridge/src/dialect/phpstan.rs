//! The PHPStan dialect's tag vocabulary: the bare inherited tags and
//! their `@phpstan-` prefixed forms.

use super::{ClassifiedTag, TagRole, TagTier};
use celerrate_plugin::AssertionPolarity;

pub(crate) fn classify(name: &str) -> Option<ClassifiedTag> {
    let (tier, bare) = match name.strip_prefix("phpstan-") {
        Some(rest) => (TagTier::PhpstanPrefixed, rest),
        None => (TagTier::Bare, name),
    };
    let role = match bare {
        "param" => TagRole::Param,
        "return" => TagRole::Return,
        "var" => TagRole::Var,
        "throws" => TagRole::Throws,
        "property" | "property-read" | "property-write" => TagRole::Property,
        "method" => TagRole::Method,
        "extends" | "template-extends" => TagRole::Extends,
        "implements" | "template-implements" => TagRole::Implements,
        "use" | "template-use" => TagRole::UseTrait,
        "template" | "template-covariant" | "template-contravariant" => TagRole::Template,
        "assert" if tier == TagTier::PhpstanPrefixed => TagRole::Assert(AssertionPolarity::Always),
        "assert-if-true" if tier == TagTier::PhpstanPrefixed => {
            TagRole::Assert(AssertionPolarity::IfTrue)
        }
        "assert-if-false" if tier == TagTier::PhpstanPrefixed => {
            TagRole::Assert(AssertionPolarity::IfFalse)
        }
        // Purity is out of this sub-project's scope end to end
        // (design section 1): ignored without error.
        "pure" | "impure" => TagRole::Ignored,
        _ => return None,
    };
    Some(ClassifiedTag { role, tier })
}
