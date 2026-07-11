/// A stable, documented diagnostic identifier, `CEL0001`-style.
///
/// Identifiers are permanent once published: users script against them
/// and suppress by them, so renumbering is a breaking change. Each
/// producing crate owns the identifiers of its own diagnostic kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiagnosticId(&'static str);

impl DiagnosticId {
    pub const fn new(identifier: &'static str) -> Self {
        Self(identifier)
    }

    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}
