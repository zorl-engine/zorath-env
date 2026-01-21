//! Build script for zorath-env
//!
//! Provides version-aware cache invalidation to prevent stale build issues.
//! Warns when cached version differs from Cargo.toml version.

use std::fs;
use std::path::Path;

fn main() {
    // Force rebuild when key files change
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/main.rs");

    // Get current version from Cargo
    let version = env!("CARGO_PKG_VERSION");

    // Check for version stamp in target directory
    let out_dir = std::env::var("OUT_DIR").unwrap_or_default();
    let target_dir = Path::new(&out_dir)
        .ancestors()
        .nth(3) // Go up from OUT_DIR to target/
        .unwrap_or(Path::new("target"));

    let stamp_path = target_dir.join(".zenv_version");

    // Compare cached version with current
    if let Ok(cached_version) = fs::read_to_string(&stamp_path) {
        let cached = cached_version.trim();
        if cached != version {
            println!(
                "cargo:warning=zenv version changed: {} -> {}. If you see stale build errors, run: cargo clean",
                cached, version
            );
        }
    }

    // Write current version stamp
    if let Err(e) = fs::write(&stamp_path, version) {
        // Non-fatal - just means we won't detect version changes next time
        println!("cargo:warning=Could not write version stamp: {}", e);
    }
}
