//! The shared diagnostic data model.
//!
//! Every layer that reports, from the parser up, projects its structured
//! findings into this model: a stable identifier, a severity, and a
//! primary span. The rich anatomy (annotated spans, notes, structured
//! suggestions) arrives with the diagnostics-and-fixes sub-project;
//! rendering is always an upper layer's business.

mod diagnostic;
mod identifier;
mod severity;

pub use diagnostic::Diagnostic;
pub use identifier::DiagnosticId;
pub use severity::Severity;
