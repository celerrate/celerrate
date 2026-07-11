//! Composer project discovery.
//!
//! Zero-configuration detection: `composer.json` is located and read
//! tolerantly (a corrupted or missing file produces a notice and
//! defaults, never a failure), the autoload rules it and
//! `vendor/composer/installed.json` declare drive the disk walk and
//! classify every file as project or vendor, and the PHP version range
//! follows the parent spec's detection precedence. This crate is pure:
//! the configuration becomes a salsa input at the composition root.

mod version;

pub use version::{
    LATEST_STABLE_VERSION, OLDEST_SUPPORTED_VERSION, PhpVersion, PhpVersionRange,
    SUPPORTED_VERSIONS,
};
