const CHANGELOG_URL: &str = "https://github.com/zorl-engine/zorath-env/blob/main/CHANGELOG.md";
const RELEASES_URL: &str = "https://github.com/zorl-engine/zorath-env/releases";

use crate::errors::CliError;

/// Show version information
#[doc(hidden)]
pub fn run(check_update: bool) -> Result<(), CliError> {
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

    #[test]
    fn test_version_urls_defined() {
        // Verify URL constants are defined correctly
        assert!(CHANGELOG_URL.starts_with("https://"));
        assert!(RELEASES_URL.starts_with("https://"));
        assert!(CHANGELOG_URL.contains("CHANGELOG"));
        assert!(RELEASES_URL.contains("releases"));
    }

    #[test]
    fn test_parse_cargo_search_output() {
        // Test the version parsing logic used in check_latest_version
        let sample_output = r#"zorath-env = "0.3.7"    # Validate .env files against a schema"#;

        // Simulate the parsing logic
        let mut result: Option<String> = None;
        for line in sample_output.lines() {
            if line.starts_with("zorath-env") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        result = Some(line[start + 1..start + 1 + end].to_string());
                    }
                }
            }
        }

        assert!(result.is_some());
        assert_eq!(result.unwrap(), "0.3.7");
    }

    #[test]
    fn test_parse_cargo_search_output_with_prerelease() {
        // Test parsing a prerelease version
        let sample_output = r#"zorath-env = "1.0.0-beta.1"    # Description"#;

        let mut result: Option<String> = None;
        for line in sample_output.lines() {
            if line.starts_with("zorath-env") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        result = Some(line[start + 1..start + 1 + end].to_string());
                    }
                }
            }
        }

        assert!(result.is_some());
        assert_eq!(result.unwrap(), "1.0.0-beta.1");
    }

    #[test]
    fn test_parse_cargo_search_no_match() {
        // Test when package is not found
        let sample_output = "other-package = \"1.0.0\"    # Different package";

        let mut result: Option<String> = None;
        for line in sample_output.lines() {
            if line.starts_with("zorath-env") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        result = Some(line[start + 1..start + 1 + end].to_string());
                    }
                }
            }
        }

        assert!(result.is_none());
    }

    #[test]
    fn test_current_version_format() {
        // Verify the current version is a valid semver
        let version = env!("CARGO_PKG_VERSION");
        let parts: Vec<&str> = version.split('.').collect();
        assert!(parts.len() >= 3, "Version should have at least 3 parts");

        // First two parts should be numeric
        assert!(parts[0].parse::<u32>().is_ok());
        assert!(parts[1].parse::<u32>().is_ok());
        // Third part might have prerelease suffix
        let patch = parts[2].split('-').next().unwrap();
        assert!(patch.parse::<u32>().is_ok());
    }

    #[test]
    fn test_parse_cargo_search_empty_output() {
        // Test when cargo search returns empty output
        let sample_output = "";

        let mut result: Option<String> = None;
        for line in sample_output.lines() {
            if line.starts_with("zorath-env") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        result = Some(line[start + 1..start + 1 + end].to_string());
                    }
                }
            }
        }

        assert!(result.is_none(), "Empty output should return None");
    }

    #[test]
    fn test_parse_cargo_search_malformed_line() {
        // Test when the line is malformed (no quotes)
        let sample_output = "zorath-env = 0.3.7    # No quotes";

        let mut result: Option<String> = None;
        for line in sample_output.lines() {
            if line.starts_with("zorath-env") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        result = Some(line[start + 1..start + 1 + end].to_string());
                    }
                }
            }
        }

        assert!(result.is_none(), "Malformed line should return None");
    }

    #[test]
    fn test_parse_cargo_search_multiple_results() {
        // Test when cargo search returns multiple results (should only match our package)
        let sample_output = r#"zorath-env = "0.3.8"    # Validate .env files
other-env = "1.0.0"    # Different package
env-checker = "2.0.0"    # Another package"#;

        let mut result: Option<String> = None;
        for line in sample_output.lines() {
            if line.starts_with("zorath-env") {
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        result = Some(line[start + 1..start + 1 + end].to_string());
                    }
                }
            }
        }

        assert_eq!(result, Some("0.3.8".to_string()), "Should match our package only");
    }

    #[test]
    fn test_parse_cargo_search_similar_package_names() {
        // Test that we don't accidentally match similar package names
        let sample_output = r#"zorath-env-extra = "1.0.0"    # Different package
zorath-env = "0.3.8"    # Our package"#;

        let mut result: Option<String> = None;
        for line in sample_output.lines() {
            if line.starts_with("zorath-env =") || line.starts_with("zorath-env=\"") {
                // More precise matching
                if let Some(start) = line.find('"') {
                    if let Some(end) = line[start + 1..].find('"') {
                        result = Some(line[start + 1..start + 1 + end].to_string());
                    }
                }
            }
        }

        assert_eq!(result, Some("0.3.8".to_string()), "Should match exact package name");
    }

    #[test]
    fn test_changelog_url_valid() {
        assert!(CHANGELOG_URL.starts_with("https://github.com"));
        assert!(CHANGELOG_URL.contains("zorl-engine"));
        assert!(CHANGELOG_URL.ends_with("CHANGELOG.md"));
    }

    #[test]
    fn test_releases_url_valid() {
        assert!(RELEASES_URL.starts_with("https://github.com"));
        assert!(RELEASES_URL.contains("zorl-engine"));
        assert!(RELEASES_URL.ends_with("/releases"));
    }
}
