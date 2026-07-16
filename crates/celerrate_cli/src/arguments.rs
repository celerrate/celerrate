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
    },

    /// Internal: the annotation ground-truth records (design section
    /// 10, harness 1). Consumed by `cargo xtask ground-truth`; hidden
    /// from help — the product surface is plan 9c's.
    #[command(hide = true)]
    GroundTruth { path: PathBuf },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use clap::Parser as _;

    use super::{Arguments, Command};

    #[test]
    fn check_defaults_to_the_current_directory_and_a_single_pass() {
        let arguments = Arguments::try_parse_from(["celerrate", "check"]).unwrap();
        let Command::Check { path, watch } = arguments.command else {
            panic!("expected Command::Check");
        };
        assert_eq!(path.to_str(), Some("."));
        assert!(!watch);
    }

    #[test]
    fn check_takes_a_path_and_a_watch_flag() {
        let arguments =
            Arguments::try_parse_from(["celerrate", "check", "src", "--watch"]).unwrap();
        let Command::Check { path, watch } = arguments.command else {
            panic!("expected Command::Check");
        };
        assert_eq!(path.to_str(), Some("src"));
        assert!(watch);
    }

    #[test]
    fn a_bad_flag_is_a_usage_error_not_a_panic() {
        assert!(Arguments::try_parse_from(["celerrate", "check", "--nope"]).is_err());
    }

    #[test]
    fn ground_truth_takes_a_path() {
        let arguments = Arguments::try_parse_from(["celerrate", "ground-truth", "src"]).unwrap();
        let Command::GroundTruth { path } = arguments.command else {
            panic!("expected Command::GroundTruth");
        };
        assert_eq!(path.to_str(), Some("src"));
    }
}
