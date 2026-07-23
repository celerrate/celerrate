//! The shared diagnostic data model.
//!
//! Every layer that reports, from the parser up, projects its structured
//! findings into this model: a stable identifier, a severity, and a
//! primary span. The rich anatomy (annotated spans, notes, structured
//! suggestions) arrives with the diagnostics-and-fixes sub-project;
//! rendering is always an upper layer's business.

mod diagnostic;
mod explain;
mod identifier;
mod label;
mod pages;
mod registry;
mod severity;
mod suggestion;

pub use diagnostic::{Anchor, Diagnostic};
pub use explain::{EXECUTABLE_EXAMPLE_EXEMPTIONS, ExampleExemption, ExplainPage};
pub use identifier::DiagnosticId;
pub use label::{Label, LabelTarget};
pub use registry::{REGISTRY, RegisteredDiagnostic, find_identifier, find_page};
pub use severity::Severity;
pub use suggestion::{Confidence, Suggestion};
