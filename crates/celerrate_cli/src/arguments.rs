//! The command line. `clap` earns its weight here: real `--help`,
//! `--version`, subcommand structure, and correct errors on bad flags,
//! and sub-project 5 grows this surface substantially (baseline, output
//! formats, `migrate --from-phpstan`).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// A complete PHP toolchain, written in Rust.
#[derive(Debug, Parser)]
#[command(name = "celerrate", version, about)]
pub struct Arguments {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Analyze a project and report its diagnostics.
    Check {
        /// The project root. Defaults to the current directory.
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Re-analyze on every change, and keep reporting.
        #[arg(long)]
        watch: bool,

        /// Apply the safe suggestions and rewrite the files.
        #[arg(long, conflicts_with = "watch")]
        fix: bool,

        /// Apply safe and needs-review suggestions alike.
        #[arg(long, conflicts_with = "watch")]
        fix_suggestions: bool,
    },

    /// Explain a diagnostic identifier: why it fires, a failing and a
    /// fixed example, and its configuration notes.
    Explain {
        /// The identifier to explain, for example CEL0030.
        identifier: String,
    },

    /// Internal: the annotation ground-truth records (design section
    /// 10, harness 1). Consumed by `cargo xtask ground-truth`; hidden
    /// from help — the product surface is plan 9c's.
    #[command(hide = true)]
    GroundTruth { path: PathBuf },

    /// Internal: the residual mixed-rate counters over a project
    /// (design sections 7 and 9). Plan 9b publishes the number; this
    /// stays hidden until then.
    #[command(hide = true)]
    MixedRate { path: PathBuf },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use clap::Parser as _;

    use super::{Arguments, Command};

    #[test]
    fn check_defaults_to_the_current_directory_and_a_single_pass() {
        let arguments = Arguments::try_parse_from(["celerrate", "check"]).unwrap();
        let Command::Check { path, watch, .. } = arguments.command else {
            panic!("expected Command::Check");
        };
        assert_eq!(path.to_str(), Some("."));
        assert!(!watch);
    }

    #[test]
    fn check_takes_a_path_and_a_watch_flag() {
        let arguments =
            Arguments::try_parse_from(["celerrate", "check", "src", "--watch"]).unwrap();
        let Command::Check { path, watch, .. } = arguments.command else {
            panic!("expected Command::Check");
        };
        assert_eq!(path.to_str(), Some("src"));
        assert!(watch);
    }

    #[test]
    fn the_two_fix_flags_parse_and_default_off() {
        let arguments = Arguments::try_parse_from(["celerrate", "check", "src", "--fix"]).unwrap();
        let Command::Check {
            fix,
            fix_suggestions,
            ..
        } = arguments.command
        else {
            panic!("expected Command::Check");
        };
        assert!(fix);
        assert!(!fix_suggestions);
        let arguments =
            Arguments::try_parse_from(["celerrate", "check", "--fix-suggestions"]).unwrap();
        let Command::Check {
            fix,
            fix_suggestions,
            ..
        } = arguments.command
        else {
            panic!("expected Command::Check");
        };
        assert!(!fix);
        assert!(fix_suggestions);
    }

    /// Either fix flag combined with `--watch` is a usage error
    /// (design section 7): applying edits from inside a watch loop
    /// would race the watcher against its own writes.
    #[test]
    fn a_fix_flag_with_watch_is_a_usage_error() {
        assert!(Arguments::try_parse_from(["celerrate", "check", "--fix", "--watch"]).is_err());
        assert!(
            Arguments::try_parse_from(["celerrate", "check", "--fix-suggestions", "--watch"])
                .is_err()
        );
    }

    #[test]
    fn a_bad_flag_is_a_usage_error_not_a_panic() {
        assert!(Arguments::try_parse_from(["celerrate", "check", "--nope"]).is_err());
    }

    #[test]
    fn explain_takes_an_identifier() {
        let arguments = Arguments::try_parse_from(["celerrate", "explain", "CEL0030"]).unwrap();
        match arguments.command {
            Command::Explain { identifier } => assert_eq!(identifier, "CEL0030"),
            other => panic!("expected explain, parsed {other:?}"),
        }
    }

    #[test]
    fn ground_truth_takes_a_path() {
        let arguments = Arguments::try_parse_from(["celerrate", "ground-truth", "src"]).unwrap();
        let Command::GroundTruth { path } = arguments.command else {
            panic!("expected Command::GroundTruth");
        };
        assert_eq!(path.to_str(), Some("src"));
    }

    #[test]
    fn mixed_rate_takes_a_path() {
        let arguments = Arguments::try_parse_from(["celerrate", "mixed-rate", "src"]).unwrap();
        let Command::MixedRate { path } = arguments.command else {
            panic!("expected Command::MixedRate");
        };
        assert_eq!(path.to_str(), Some("src"));
    }

    /// The hidden variants must actually stay hidden: `--help` is the
    /// product surface plan 9c owns, and a subcommand appearing there
    /// early would ship an undocumented, unstable channel as if it
    /// were supported.
    #[test]
    fn mixed_rate_is_hidden_from_help() {
        let mut output = Vec::new();
        let outcome = crate::run(
            vec!["celerrate".into(), "--help".into()],
            &mut output,
            crate::ColorMode::Plain,
        );
        assert_eq!(outcome, crate::Outcome::Clean);
        assert!(
            !String::from_utf8(output).unwrap().contains("mixed-rate"),
            "mixed-rate must not appear in --help",
        );
    }
}
