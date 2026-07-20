//! The database escape hatch is engine-internal: a facade consumer
//! cannot reach a salsa handle through an annotation site.
fn misuse<'db, 'site>(site: &celerrate_plugin::AnnotationSite<'db, 'site>) {
    let _ = site.database();
}

fn main() {}
