//! The facade re-exports vocabulary, never the database crate.
use celerrate_plugin::salsa;

fn main() {
    let _ = salsa;
}
