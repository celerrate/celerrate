//! The embedded explain pages, one module per producing area. Each
//! page is a `const` the registry references; the executable-page
//! harness at the composition root (`celerrate_cli/tests/
//! explain_pages.rs`) keeps every non-exempt example honest.

pub(crate) mod semantic;
