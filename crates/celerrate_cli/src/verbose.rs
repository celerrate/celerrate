//! The verbose channel: meta-reporting about the analysis, on stderr,
//! behind the global `--verbose` flag. Two kinds of lines: one per
//! widened foreign directive (the sub-project 4 debt: a foreign
//! directive with any unmapped identifier falls back to scope-wide
//! suppression, and the user now sees it), and one run summary of
//! already-available meta-information (files analyzed, cache verdict
//! traffic).
//!
//! Stderr because this is meta-reporting about the analysis, not an
//! analysis result: the machine formats stay byte-identical with or
//! without the flag. Widening is deliberately not a diagnostic - a CEL
//! code here would recreate the false-positive storm on imported
//! codebases. The content is not a stable surface, and nothing here
//! enters the queries or the persistent cache: the widened marks are
//! derived fresh from `suppression_directives`, so the lines are
//! independent of which files the cache happened to serve this run.
//! The module follows the `cache::statistics` convention: pure render
//! functions, unit tested, plus a thin `eprintln!` emitter.

use std::sync::atomic::Ordering;

use crate::cache::statistics::CacheStatistics;
use crate::session::Session;

/// One widened foreign directive, presentation-ready. The derived
/// order (path, then line, then identifiers) is the report order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WidenedDirective {
    /// The carrying file, project-relative, as the renderer displays it.
    pub path: String,
    /// The carrying comment's first line, 1-based.
    pub line: u32,
    /// The unmapped written identifiers, in written order.
    pub identifiers: Vec<String>,
}

/// Every widened foreign directive across the reported files, sorted.
/// Derived fresh from `suppression_directives`, deliberately: the
/// answer is a pure function of the sources and the registered
/// bridge, independent of which files the cache served this run. On a
/// warm run this parses files the analysis itself skipped; that is the
/// price of asking, paid only under `--verbose`.
pub fn widened_directives(session: &Session) -> Vec<WidenedDirective> {
    let database = &session.database;
    let mut widened = Vec::new();
    for &file in session.inputs().reported.iter() {
        for directive in celerrate_semantics::suppression_directives(database, file) {
            if directive.widened_by.is_empty() {
                continue;
            }
            let file_id = file.file_id(database);
            let index = celerrate_db::line_index(database, file);
            widened.push(WidenedDirective {
                path: crate::render::display_path(session, file_id),
                line: index.line_column(directive.anchor.start()).line + 1,
                identifiers: directive.widened_by.clone(),
            });
        }
    }
    widened.sort();
    widened
}

/// One line per widened directive: the file, the directive's line, the
/// unmapped identifiers, and the consequence. Not a stable surface.
pub fn render_widened(directive: &WidenedDirective) -> String {
    let noun = if directive.identifiers.len() == 1 {
        "identifier"
    } else {
        "identifiers"
    };
    let identifiers = directive
        .identifiers
        .iter()
        .map(|identifier| format!("`{identifier}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "verbose: {}:{}: unmapped {noun} {identifiers}: the directive \
         widens to scope-wide suppression",
        directive.path, directive.line,
    )
}

/// The run summary: already-available meta-information, no format
/// commitment. Not a stable surface.
pub fn render_run_summary(statistics: &CacheStatistics, analyzed: usize) -> String {
    let load = |counter: &std::sync::atomic::AtomicU64| counter.load(Ordering::Relaxed);
    format!(
        "verbose: {analyzed} files analyzed; verdicts {} served / {} \
         discarded / {} absent from the cache",
        load(&statistics.verdicts_served),
        load(&statistics.verdicts_discarded),
        load(&statistics.verdicts_absent),
    )
}

/// Prints every verbose line to stderr. The caller gates on the flag;
/// this function only speaks (the `cache::statistics::report` model:
/// the render functions above carry the tests, this wrapper stays
/// thin).
pub fn report(session: &Session) {
    for directive in widened_directives(session) {
        eprintln!("{}", render_widened(&directive));
    }
    let analyzed = session.inputs().reported.len();
    eprintln!("{}", render_run_summary(&session.statistics, analyzed));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::session::Session;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (path, contents) in files {
            let path = root.path().join(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }
        root
    }

    #[test]
    fn an_unmapped_identifier_yields_one_widened_entry() {
        let root = project(&[(
            "a.php",
            "<?php\nnew MissingOne(); // @phpstan-ignore some.unknownIdentifier\n",
        )]);
        let session = Session::start(root.path());
        assert_eq!(
            widened_directives(&session),
            vec![WidenedDirective {
                path: "a.php".to_owned(),
                line: 2,
                identifiers: vec!["some.unknownIdentifier".to_owned()],
            }],
        );
    }

    #[test]
    fn a_fully_mapped_directive_is_not_reported() {
        let root = project(&[(
            "a.php",
            "<?php\nnew MissingOne(); // @phpstan-ignore class.notFound\n",
        )]);
        let session = Session::start(root.path());
        assert!(widened_directives(&session).is_empty());
    }

    #[test]
    fn an_explicit_scope_wide_suppression_is_not_reported() {
        // `@psalm-suppress all` is the user's own decision, not a
        // fallback the channel should second-guess.
        let root = project(&[(
            "a.php",
            "<?php\n/* @psalm-suppress all */\nnew MissingOne();\n",
        )]);
        let session = Session::start(root.path());
        assert!(widened_directives(&session).is_empty());
    }

    #[test]
    fn a_wrapped_list_reports_the_synthetic_continuation_identifier() {
        let root = project(&[(
            "a.php",
            "<?php\n/**\n * @psalm-suppress UndefinedClass,\n * UndefinedFunction\n */\nclass Service {}\n",
        )]);
        let session = Session::start(root.path());
        let widened = widened_directives(&session);
        assert_eq!(widened.len(), 1);
        assert_eq!(
            widened[0].identifiers,
            vec!["<identifier list continues on the next line>".to_owned()],
        );
        // The line is the carrying comment's first line.
        assert_eq!(widened[0].line, 2);
    }

    #[test]
    fn entries_are_sorted_by_path_then_line() {
        let root = project(&[
            (
                "b.php",
                "<?php\nnew MissingOne(); // @phpstan-ignore second.unknown\n",
            ),
            (
                "a.php",
                "<?php\nnew MissingOne(); // @phpstan-ignore first.unknown\n\nnew MissingTwo(); // @phpstan-ignore third.unknown\n",
            ),
        ]);
        let session = Session::start(root.path());
        let widened = widened_directives(&session);
        let keys: Vec<(&str, u32)> = widened
            .iter()
            .map(|entry| (entry.path.as_str(), entry.line))
            .collect();
        assert_eq!(keys, vec![("a.php", 2), ("a.php", 4), ("b.php", 2)]);
    }

    #[test]
    fn the_widened_line_names_file_line_identifier_and_consequence() {
        let directive = WidenedDirective {
            path: "src/a.php".to_owned(),
            line: 3,
            identifiers: vec!["some.unknown".to_owned()],
        };
        assert_eq!(
            render_widened(&directive),
            "verbose: src/a.php:3: unmapped identifier `some.unknown`: \
             the directive widens to scope-wide suppression",
        );
    }

    #[test]
    fn several_unmapped_identifiers_share_one_line() {
        let directive = WidenedDirective {
            path: "a.php".to_owned(),
            line: 2,
            identifiers: vec!["first.unknown".to_owned(), "second.unknown".to_owned()],
        };
        assert_eq!(
            render_widened(&directive),
            "verbose: a.php:2: unmapped identifiers `first.unknown`, \
             `second.unknown`: the directive widens to scope-wide suppression",
        );
    }

    #[test]
    fn the_run_summary_carries_the_counters() {
        use std::sync::atomic::Ordering;
        let statistics = crate::cache::statistics::CacheStatistics::default();
        statistics.verdicts_served.fetch_add(3, Ordering::Relaxed);
        statistics.verdicts_absent.fetch_add(2, Ordering::Relaxed);
        assert_eq!(
            render_run_summary(&statistics, 5),
            "verbose: 5 files analyzed; verdicts 3 served / 0 discarded / \
             2 absent from the cache",
        );
    }
}
