const CHANGELOG_URL: &str = "https://github.com/zorl-engine/zorath-env/blob/main/CHANGELOG.md";
const RELEASES_URL: &str = "https://github.com/zorl-engine/zorath-env/releases";

/// Show version information
pub fn run(check_update: bool) -> Result<(), String> {
    let version = env!("CARGO_PKG_VERSION");
    println!("zenv v{version}");

    if check_update {
        match check_latest_version() {
            Ok(Some(latest)) if latest != version => {
                println!("Latest: v{latest} (update available)");
                println!("Run: cargo install zorath-env --force");
                println!();
                println!("Changelog: {}", CHANGELOG_URL);
                println!("Releases:  {}", RELEASES_URL);
            }
            Ok(Some(_)) => {
                println!("You are on the latest version.");
            }
            Ok(None) => {
                println!("Could not determine latest version.");
            }
            Err(e) => {
                println!("Failed to check for updates: {e}");
            }
        }
    }

    Ok(())
}

/// Query crates.io API for the latest version
fn check_latest_version() -> Result<Option<String>, String> {
    // Use cargo search output parsing (simpler than HTTP client)
    let output = std::process::Command::new("cargo")
        .args(["search", "zorath-env", "--limit", "1"])
        .output()
        .map_err(|e| format!("failed to run cargo search: {e}"))?;

    if !output.status.success() {
        return Err("cargo search failed".into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Parse: zorath-env = "0.2.1"    # description...
    for line in stdout.lines() {
        if line.starts_with("zorath-env") {
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start + 1..].find('"') {
                    return Ok(Some(line[start + 1..start + 1 + end].to_string()));
                }
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_output() {
        // Just verify run doesn't panic without --check-update
        let result = run(false);
        assert!(result.is_ok());
    }
}
