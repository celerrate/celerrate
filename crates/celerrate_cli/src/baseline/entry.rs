//! The baseline entry: the structural fingerprint of a known finding.
//!
//! The key is `(relative path, CEL identifier, enclosing symbol path,
//! message, count)`. No line number anywhere: the symbol path provides the
//! locality a line number used to provide, without its fragility.

/// One recorded finding. The derived ordering (path, then identifier, then
/// symbol, then message, then count) is the deterministic file order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaselineEntry {
    /// Project-relative path with `/` separators on every platform.
    pub path: String,
    /// The diagnostic identifier, e.g. `CEL0034`.
    pub identifier: String,
    /// The enclosing symbol path (`App\Service\Checkout::finalize`), or
    /// `(top level)` for code outside declarations.
    pub symbol: String,
    /// The full rendered message. Two diagnostics with the same identifier
    /// in the same scope are distinguished by their messages.
    pub message: String,
    /// True duplicates absorbed: matching consumes at most this many
    /// occurrences; occurrence `count + 1` is reported as new.
    pub count: u32,
}

impl BaselineEntry {
    pub fn key(&self) -> BaselineKey {
        BaselineKey {
            path: self.path.clone(),
            identifier: self.identifier.clone(),
            symbol: self.symbol.clone(),
            message: self.message.clone(),
        }
    }
}

/// The matching key: every field of the entry except the count.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaselineKey {
    pub path: String,
    pub identifier: String,
    pub symbol: String,
    pub message: String,
}
