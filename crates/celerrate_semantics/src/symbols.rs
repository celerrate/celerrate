//! The three PHP symbol spaces and the case-folded lookup keys of the
//! global symbol index. PHP resolves class and function names
//! case-insensitively and constant names case-sensitively; namespaces
//! are case-insensitive everywhere. Folding is ASCII-only, matching
//! the engine's own folding.

use crate::items::DeclarationKind;
use celerrate_stubs::StubSymbolKind;

/// The symbol space a name resolves in. Classes, interfaces, traits,
/// and enums share one space; functions and constants each have their
/// own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SymbolSpace {
    ClassLike,
    Function,
    Constant,
}

impl SymbolSpace {
    /// The space a declared symbol occupies.
    pub fn of_declaration(kind: DeclarationKind) -> Self {
        match kind {
            DeclarationKind::Class
            | DeclarationKind::Interface
            | DeclarationKind::Trait
            | DeclarationKind::Enum => Self::ClassLike,
            DeclarationKind::Function => Self::Function,
            DeclarationKind::Constant => Self::Constant,
        }
    }

    /// The space a compiled stub symbol occupies.
    pub fn of_stub_kind(kind: StubSymbolKind) -> Self {
        match kind {
            StubSymbolKind::Class
            | StubSymbolKind::Interface
            | StubSymbolKind::Trait
            | StubSymbolKind::Enum => Self::ClassLike,
            StubSymbolKind::Function => Self::Function,
            StubSymbolKind::Constant => Self::Constant,
        }
    }
}

/// Joins a namespace (`""` is global) and a name into the fully
/// qualified spelling, without a leading backslash. Both arguments
/// must be clean segments, without leading or trailing backslashes.
pub fn fully_qualified_name(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace}\\{name}")
    }
}

/// The case-folded lookup key of one fully qualified name: the whole
/// name for class-likes and functions, the namespace segments only for
/// constants (their terminal segment is case-sensitive).
pub fn folded_symbol_key(space: SymbolSpace, fully_qualified: &str) -> String {
    match space {
        SymbolSpace::ClassLike | SymbolSpace::Function => fully_qualified.to_ascii_lowercase(),
        SymbolSpace::Constant => match fully_qualified.rsplit_once('\\') {
            Some((namespace, name)) => format!("{}\\{name}", namespace.to_ascii_lowercase()),
            None => fully_qualified.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{SymbolSpace, folded_symbol_key, fully_qualified_name};
    use crate::items::DeclarationKind;
    use celerrate_stubs::StubSymbolKind;

    #[test]
    fn every_declaration_kind_maps_to_its_space() {
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Class),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Interface),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Trait),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Enum),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Function),
            SymbolSpace::Function,
        );
        assert_eq!(
            SymbolSpace::of_declaration(DeclarationKind::Constant),
            SymbolSpace::Constant,
        );
    }

    #[test]
    fn a_fully_qualified_name_joins_namespace_and_name() {
        assert_eq!(fully_qualified_name("", "Service"), "Service");
        assert_eq!(
            fully_qualified_name("App\\Domain", "Service"),
            "App\\Domain\\Service",
        );
    }

    #[test]
    fn class_and_function_keys_fold_the_whole_name() {
        assert_eq!(
            folded_symbol_key(SymbolSpace::ClassLike, "App\\Service"),
            "app\\service",
        );
        assert_eq!(
            folded_symbol_key(SymbolSpace::Function, "App\\Greet"),
            "app\\greet",
        );
    }

    #[test]
    fn constant_keys_keep_their_terminal_segment() {
        // Namespaces are case-insensitive even for constants; only the
        // constant's own name keeps its case.
        assert_eq!(
            folded_symbol_key(SymbolSpace::Constant, "App\\Sub\\Limit"),
            "app\\sub\\Limit",
        );
        assert_eq!(folded_symbol_key(SymbolSpace::Constant, "E_ALL"), "E_ALL");
    }

    #[test]
    fn folding_is_ascii_only() {
        // PHP's engine folds ASCII only; multibyte spellings stay
        // distinct.
        assert_eq!(
            folded_symbol_key(SymbolSpace::ClassLike, "App\\Éxception"),
            "app\\Éxception",
        );
    }

    #[test]
    fn every_stub_kind_maps_to_its_space() {
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Class),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Interface),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Trait),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Enum),
            SymbolSpace::ClassLike,
        );
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Function),
            SymbolSpace::Function,
        );
        assert_eq!(
            SymbolSpace::of_stub_kind(StubSymbolKind::Constant),
            SymbolSpace::Constant,
        );
    }
}
