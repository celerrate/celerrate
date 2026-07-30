//! `celerrate migrate --from-phpstan`: convert a PHPStan project to
//! Celerrate in one command. Parse `phpstan.neon` (a minimal NEON
//! subset), generate `celerrate.toml`, report what does not carry
//! over, and record the baseline so only new problems fail.
// The command wires into the CLI in a later change; until then the
// module is library-only.
#![allow(dead_code)]

pub(crate) mod convert;
pub(crate) mod neon;
pub(crate) mod settings;
