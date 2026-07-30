//! The virtual-symbol extension point: members declared by annotation
//! rather than written in code (`@property`, `@method`).
//!
//! Owned by this crate: the registry input lives with
//! the consuming layer, implementations are registered at the
//! composition root, `celerrate_plugin` re-exports the vocabulary.
//! Type expressions travel as **unresolved text** — this layer sits
//! below `celerrate_types` and cannot name `TypeId`; the text
//! resolves downstream through the type-syntax extension point
//! exactly like a real member's signature text.

use std::sync::Arc;

use crate::plugin::PluginIdentity;

/// The member kinds an annotation can declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VirtualMemberKind {
    Method,
    Property,
}

/// One parameter of a virtual method (`@method`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct VirtualParameter {
    pub name: String,
    /// The written type expression, unresolved.
    pub type_text: Option<String>,
    /// True when the annotation gives a default value.
    pub optional: bool,
    pub variadic: bool,
}

impl VirtualParameter {
    /// Constructor for cross-crate construction: literal construction
    /// is closed by `#[non_exhaustive]`. Remaining fields default to
    /// their unset value; set them by mutation.
    pub fn new(name: String) -> Self {
        Self {
            name,
            type_text: None,
            optional: false,
            variadic: false,
        }
    }
}

/// One member declared by a class-like's docblock.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct VirtualMember {
    pub kind: VirtualMemberKind,
    /// Original spelling (property names without the `$`).
    pub name: String,
    pub is_static: bool,
    /// The value or return type expression, unresolved.
    pub type_text: Option<String>,
    /// Parameters, for virtual methods; empty for properties.
    pub parameters: Vec<VirtualParameter>,
}

impl VirtualMember {
    /// Constructor for cross-crate construction: literal construction
    /// is closed by `#[non_exhaustive]`. Remaining fields default to
    /// their unset value; set them by mutation.
    pub fn new(kind: VirtualMemberKind, name: String) -> Self {
        Self {
            kind,
            name,
            is_static: false,
            type_text: None,
            parameters: Vec::new(),
        }
    }
}

/// A provider contributes the members a class-like docblock declares.
/// Implementations must be deterministic pure functions of the
/// docblock text: no interior state, no environment reads (the
/// byte-identical harness is the mechanical detector).
pub trait VirtualSymbolProvider: Send + Sync {
    fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember>;
}

/// One registration: the implementation travels with its identity.
#[derive(Clone)]
pub struct VirtualSymbolRegistration {
    pub identity: PluginIdentity,
    pub provider: Arc<dyn VirtualSymbolProvider>,
}

impl std::fmt::Debug for VirtualSymbolRegistration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VirtualSymbolRegistration")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// The registry: set once per process at the composition root, in the
/// high-durability tier, and never mutated — reading it therefore
/// never invalidates. Databases that register nothing (every test
/// database by default) take the no-plugin path. Providers are
/// consulted in registered order; contributions concatenate in that
/// order, so the result is independent of thread timing.
#[salsa::input(singleton)]
pub struct VirtualSymbolRegistry {
    #[returns(ref)]
    pub registrations: Vec<VirtualSymbolRegistration>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use celerrate_db::testing::TestDatabase;

    #[derive(Debug)]
    struct FakeProvider {
        members: Vec<VirtualMember>,
    }

    impl VirtualSymbolProvider for FakeProvider {
        fn virtual_members(&self, class_docblock: &str) -> Vec<VirtualMember> {
            if class_docblock.contains("@fake") {
                self.members.clone()
            } else {
                Vec::new()
            }
        }
    }

    fn identity(name: &str) -> PluginIdentity {
        PluginIdentity {
            name: name.to_owned(),
            version: "0.0.0".to_owned(),
            configuration: String::new(),
        }
    }

    #[test]
    fn an_unset_registry_is_the_no_plugin_path() {
        let db = TestDatabase::default();
        assert!(VirtualSymbolRegistry::try_get(&db).is_none());
    }

    #[test]
    fn a_registered_provider_answers_through_the_registry() {
        let db = TestDatabase::default();
        let member = VirtualMember {
            kind: VirtualMemberKind::Property,
            name: "title".to_owned(),
            is_static: false,
            type_text: Some("string".to_owned()),
            parameters: Vec::new(),
        };
        let _ = VirtualSymbolRegistry::builder(vec![VirtualSymbolRegistration {
            identity: identity("fake"),
            provider: std::sync::Arc::new(FakeProvider {
                members: vec![member.clone()],
            }),
        }])
        .durability(salsa::Durability::HIGH)
        .new(&db);

        let registry = VirtualSymbolRegistry::try_get(&db).unwrap();
        let registrations = registry.registrations(&db);
        assert_eq!(registrations.len(), 1);
        assert_eq!(registrations[0].identity.name, "fake");
        assert_eq!(
            registrations[0].provider.virtual_members("/** @fake */"),
            vec![member],
        );
        assert!(
            registrations[0]
                .provider
                .virtual_members("/** plain prose */")
                .is_empty(),
        );
    }
}
