//! Sites are constructed by the engine at dispatch, never by a plugin.
fn main() {
    let _ = celerrate_plugin::AnnotationSite::new;
}
