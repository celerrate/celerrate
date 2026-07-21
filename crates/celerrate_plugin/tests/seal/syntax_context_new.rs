//! The syntax context is sealed: `new` is crate-private in
//! `celerrate_rules`, so the facade side cannot construct one.
fn main() {
    let _ = celerrate_plugin::SyntaxContext::new;
}
