//! The per-file coercion mode: whether the file
//! declares `strict_types=1`. An own-tree read for strictly-local
//! output — the syntax-gating precedent (`syntax_gating.rs`) — so
//! nothing above the file is invalidated by the directive, and
//! nothing here survives a parse change it should not.

use celerrate_db::{SourceFile, parse};
use celerrate_syntax::ast::{self, AstNode};

/// `true` iff the file's own tree carries a top-level
/// `declare(strict_types=1)`. PHP requires the directive to be the
/// file's very first statement; a later placement is a compile error,
/// so a file that places it later can never run either way. This
/// query still recognizes it there (recorded over-acceptance): treating
/// such a file as strict only tightens the checks built on top of this
/// posture, never loosens them.
#[salsa::tracked]
pub fn file_strict_types(db: &dyn salsa::Database, file: SourceFile) -> bool {
    let root = parse(db, file).tree();
    let Some(source_file) = ast::SourceFile::cast(root) else {
        return false;
    };
    source_file.statements().any(|statement| {
        let ast::Statement::DeclareStatement(declare) = statement else {
            return false;
        };
        declare
            .declare_directives()
            .any(|directive| directive_is_strict_types(&directive))
    })
}

/// `strict_types` compared case-insensitively (PHP's directive names
/// are), value literal `1`. Text is read through the generated
/// accessors (`name_token`, `value`) and `ast::expression_text`, never
/// by re-lexing the source.
fn directive_is_strict_types(directive: &ast::DeclareDirective) -> bool {
    let name_matches = directive
        .name_token()
        .is_some_and(|name| name.text().eq_ignore_ascii_case("strict_types"));
    let value_is_one = directive
        .value()
        .is_some_and(|value| ast::expression_text(&value).trim() == "1");
    name_matches && value_is_one
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use celerrate_db::SourceFile;
    use celerrate_db::testing::TestDatabase;
    use celerrate_source::FileId;

    use super::file_strict_types;

    struct Fixture {
        db: TestDatabase,
        handles: Vec<SourceFile>,
    }

    fn fixture(sources: &[&str]) -> Fixture {
        let db = TestDatabase::default();
        let handles: Vec<SourceFile> = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                SourceFile::new(&db, FileId::new(index as u32), source.as_bytes().to_vec())
            })
            .collect();
        Fixture { db, handles }
    }

    fn handle_of(fixture: &Fixture, index: usize) -> SourceFile {
        fixture.handles[index]
    }

    #[test]
    fn the_declare_directive_is_read_from_the_top_of_the_file() {
        let cases: &[(&str, bool)] = &[
            ("<?php declare(strict_types=1);\nfunction f() {}", true),
            ("<?php declare(strict_types = 1);\nfunction f() {}", true),
            ("<?php declare(STRICT_TYPES=1);\nfunction f() {}", true),
            ("<?php declare(strict_types=0);\nfunction f() {}", false),
            ("<?php declare(ticks=1);\nfunction f() {}", false),
            ("<?php function f() {}", false),
            ("", false),
            // PHP requires the directive to be the file's very first
            // statement; a later placement is a compile error, so the
            // file cannot run either way. Accepting it as strict is
            // recorded over-acceptance (it only tightens the checks).
            ("<?php $x = 1; declare(strict_types=1);", true),
        ];
        for (source, expected) in cases {
            let fixture = fixture(&[source]);
            assert_eq!(
                file_strict_types(&fixture.db, handle_of(&fixture, 0)),
                *expected,
                "source: {source:?}",
            );
        }
    }
}
