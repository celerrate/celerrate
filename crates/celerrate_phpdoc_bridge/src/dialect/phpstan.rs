//! The PHPStan dialect's tag vocabulary: the bare inherited tags and
//! their `@phpstan-` prefixed forms.

use super::{ClassifiedTag, TagRole, TagTier};

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
        // Purity is out of this sub-project's scope end to end
        // (design section 1): ignored without error.
        "pure" | "impure" => TagRole::Ignored,
        _ => return None,
    };
    Some(ClassifiedTag { role, tier })
}
