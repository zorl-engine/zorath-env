//! zorath-env library
//!
//! This module exposes the internal functionality for integration testing.
//! The primary interface is the CLI binary (`zenv`), but these modules
//! can be used programmatically for testing and embedding.

pub mod commands;
pub mod config;
pub mod envfile;
pub mod presets;
pub mod remote;
pub mod schema;
pub mod secrets;
pub mod suggestions;
