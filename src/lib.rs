//! zorath-env -- .env validation, documentation, and export library.
//!
//! # Public API (stable)
//!
//! - [`envfile::parse_env_file`], [`envfile::parse_env_str`] -- Parse .env files
//! - [`envfile::interpolate_env`] -- Resolve variable references
//! - [`schema::load_schema_with_options`] -- Load JSON/YAML schemas (local or remote)
//! - [`commands::check::validate`], [`commands::check::validate_files`] -- Validation
//! - [`commands::export::export_to_string`] -- Export env to shell/docker/k8s/json/systemd
//! - [`commands::docs::generate`], [`commands::docs::generate_markdown`], [`commands::docs::generate_json`] -- Generate docs
//! - [`commands::example::generate`] -- Generate .env.example content
//!
//! # Internal modules
//!
//! Command `run()` functions are CLI entry points that perform I/O and print
//! to stdout/stderr. They are not part of the stable library API and are
//! marked `#[doc(hidden)]`.

pub mod commands;
pub mod config;
pub mod envfile;
pub mod errors;
pub mod presets;
pub mod remote;
pub mod schema;
pub mod secrets;
pub mod suggestions;
