//! Tag classification across the two semantic dialects — the
//! inter-dialect precedence lives here because the dialects coexist
//! on one docblock in real code (design section 5).
//!
//! # The conflict table (decision 8; published by plan 9c)
//!
//! For one slot, the tiers resolve as:
//!
//! | slot | wins | over | over |
//! |---|---|---|---|
//! | return | `@phpstan-return` | `@psalm-return` | `@return` |
//! | param (per name) | `@phpstan-param` | `@psalm-param` | `@param` |
//! | var | `@phpstan-var` | `@psalm-var` | `@var` |
//! | property / method | `@phpstan-` form | `@psalm-` form | bare form |
//!
//! Within one tier the first *parseable* tag wins; an unparseable tag
//! never consumes a slot (the 4a rule, preserved). `@throws`
//! accumulates across tiers instead of resolving. The enumerated
//! ignored-divergent bucket (purity, taint, Psalm-specific `this`
//! refinements) classifies as `Ignored`: recognized, contributing
//! nothing, disturbing nothing — traced as debt toward a later
//! complement.

pub(crate) mod phpstan;
pub(crate) mod psalm;

/// Precedence tiers, strongest first: `Ord` derives so a lower
/// variant wins a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TagTier {
    PhpstanPrefixed,
    PsalmPrefixed,
    Bare,
}

/// What a recognized tag feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TagRole {
    Param,
    Return,
    Var,
    Throws,
    Property,
    Method,
    /// `@template`, `@template-covariant`, `@template-contravariant`:
    /// the variance marker is recognized and dropped (decision 6).
    Template,
    /// The enumerated divergent bucket: parsed, ignored without error.
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClassifiedTag {
    pub(crate) role: TagRole,
    pub(crate) tier: TagTier,
}

/// PHPStan's vocabulary is consulted first, Psalm's second — an
/// arbitrary-looking order that is in fact inert: the two `classify`
/// functions match disjoint tag names.
pub(crate) fn classify(name: &str) -> Option<ClassifiedTag> {
    phpstan::classify(name).or_else(|| psalm::classify(name))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn classification_covers_both_dialects_and_the_tiers_order() {
        assert!(TagTier::PhpstanPrefixed < TagTier::PsalmPrefixed);
        assert!(TagTier::PsalmPrefixed < TagTier::Bare);
        let param = classify("param").unwrap();
        assert_eq!((param.role, param.tier), (TagRole::Param, TagTier::Bare));
        let phpstan = classify("phpstan-return").unwrap();
        assert_eq!(
            (phpstan.role, phpstan.tier),
            (TagRole::Return, TagTier::PhpstanPrefixed),
        );
        let psalm = classify("psalm-var").unwrap();
        assert_eq!(
            (psalm.role, psalm.tier),
            (TagRole::Var, TagTier::PsalmPrefixed)
        );
        assert_eq!(classify("psalm-pure").unwrap().role, TagRole::Ignored);
        assert_eq!(classify("author"), None);
    }
}
