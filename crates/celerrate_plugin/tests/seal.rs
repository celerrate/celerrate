//! The negative proof of the plugin-boundary seal (issues #61, #67):
//! from the facade side, the sealed constructors are not nameable, the
//! database is not reachable, and `salsa` is not re-exported. Each case
//! is one file whose compiler error is pinned; an edit that reopens the
//! boundary makes a case compile, which fails this suite.

#[test]
fn the_seal_holds() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/seal/*.rs");
}
