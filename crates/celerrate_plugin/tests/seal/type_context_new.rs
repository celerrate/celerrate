//! The type facade is entered through `AnnotationSite::types()` or
//! `InvocationSite::types()`, never constructed directly.
fn main() {
    let _ = celerrate_plugin::TypeContext::new;
}
