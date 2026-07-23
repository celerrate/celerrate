//! The page for the source-loading environment class: a file too large
//! for the analysis engine to hold (CEL0001).

use crate::explain::ExplainPage;

pub(crate) const CEL0001: ExplainPage = ExplainPage {
    why: "\
The engine decodes a file's bytes into `SourceText` before anything
else can run over it, and that decode step refuses a file whose
decoded size exceeds 4 GiB: no lexer, parser, or rule ever sees the
content. Rather than analyze a truncated or partial view of a file
this large, the file is skipped outright and this diagnostic is
reported in its place, at the very start of the file.",
    failing_example: "\
This page's example is authored, not executed by the explain-page
harness: it fires only on a file whose decoded size exceeds 4 GiB,
which cannot be committed as a fixture. Picture a generated PHP file
that embeds a multi-gigabyte data blob directly in source, for example
a single string literal holding a base64-encoded asset checked in by
mistake instead of loaded at runtime:

<?php
namespace App;

// A code generator emitted this file with a single string literal
// whose decoded content is several gigabytes long (an embedded asset
// that belongs in a separate binary file, not in PHP source).
const EMBEDDED_ASSET = '...several gigabytes of encoded data...';
",
    fixed_example: "\
Loading the asset at runtime instead of embedding it in source keeps
the file well under the 4 GiB cap, so decoding, parsing, and analysis
proceed normally:

<?php
namespace App;

function embedded_asset(): string {
    return file_get_contents(__DIR__ . '/asset.bin');
}
",
    configuration: "\
Reported by `celerrate_db`'s source-loading step, not a rule: it is
neither disableable nor configurable. The file is skipped entirely;
every other file in the project is still analyzed.",
};
