//! Colorless rendering snapshots (design section 11): the rich block
//! shapes, pinned byte for byte in `ColorMode::Plain`.

#![cfg(feature = "render")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use celerrate_diagnostics::{Diagnostic, Severity, find_identifier};
use celerrate_rules::render::{
    ColorMode, DegradeEverything, FaultInjection, SourceAccess, render_report,
};
use celerrate_source::{FileId, TextRange, TextSize};

struct FixtureSources(Vec<(FileId, &'static str, &'static str)>);

impl SourceAccess for FixtureSources {
    fn display_path(&self, file: FileId) -> Option<String> {
        self.0
            .iter()
            .find(|(id, _, _)| *id == file)
            .map(|(_, path, _)| (*path).to_owned())
    }

    fn text(&self, file: FileId) -> Option<&str> {
        self.0
            .iter()
            .find(|(id, _, _)| *id == file)
            .map(|(_, _, text)| *text)
    }
}

fn range(start: u32, end: u32) -> TextRange {
    TextRange::new(TextSize::from(start), TextSize::from(end))
}

const KERNEL: &str = "<?php\nnamespace App;\n\nclass Kernel extends Missing\n{\n}\n";

fn kernel_diagnostic() -> Diagnostic {
    Diagnostic::spanned(
        find_identifier("CEL0018").unwrap(),
        Severity::Error,
        FileId::new(0),
        range(43, 50),
        "unknown class `Missing`".to_owned(),
    )
}

fn sources() -> FixtureSources {
    FixtureSources(vec![(FileId::new(0), "src/Kernel.php", KERNEL)])
}

#[test]
fn a_span_diagnostic_renders_a_rustc_style_block() {
    let report = render_report(
        &[kernel_diagnostic()],
        &sources(),
        &DegradeEverything,
        ColorMode::Plain,
        &FaultInjection::None,
    );
    assert!(report.failures.is_empty());
    insta::assert_snapshot!("single_span", report.blocks.join("\n\n"));
}
