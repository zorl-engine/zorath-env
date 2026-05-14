const CHANGELOG_URL: &str = "https://github.com/zorl-engine/zorath-env/blob/main/CHANGELOG.md";
const RELEASES_URL: &str = "https://github.com/zorl-engine/zorath-env/releases";
const CRATES_IO_API: &str = "https://crates.io/api/v1/crates/zorath-env";

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

/// Query crates.io API for the latest version via the hardened remote
/// pipeline. Routing through `remote::fetch_metadata` keeps version checks
/// behind the same gates as schema fetches (HTTPS-only, SSRF allowlist,
/// zero redirects, bounded response body) instead of shelling out to
/// `cargo search`, which would bypass every one of those.
fn check_latest_version() -> Result<Option<String>, String> {
    let body = crate::remote::fetch_metadata(CRATES_IO_API)
        .map_err(|e| format!("failed to query crates.io: {e}"))?;
    parse_newest_version(&body)
}

/// Parse the `crate.newest_version` field out of the crates.io JSON
/// response. Returns `Ok(None)` only when the field is missing entirely;
/// a malformed body is an error so the user sees something is wrong instead
/// of a misleading "Could not determine latest version".
fn parse_newest_version(body: &str) -> Result<Option<String>, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid JSON from crates.io: {e}"))?;
    Ok(value
        .get("crate")
        .and_then(|c| c.get("newest_version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
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
        assert!(CHANGELOG_URL.starts_with("https://"));
        assert!(RELEASES_URL.starts_with("https://"));
        assert!(CRATES_IO_API.starts_with("https://"));
        assert!(CHANGELOG_URL.contains("CHANGELOG"));
        assert!(RELEASES_URL.contains("releases"));
        assert!(CRATES_IO_API.contains("crates.io/api/v1/crates/zorath-env"));
    }

    #[test]
    fn test_parse_newest_version_typical() {
        let body = r#"{"crate":{"id":"zorath-env","name":"zorath-env","newest_version":"0.3.9"}}"#;
        assert_eq!(
            parse_newest_version(body).unwrap(),
            Some("0.3.9".to_string())
        );
    }

    #[test]
    fn test_parse_newest_version_prerelease() {
        let body = r#"{"crate":{"newest_version":"1.0.0-beta.1"}}"#;
        assert_eq!(
            parse_newest_version(body).unwrap(),
            Some("1.0.0-beta.1".to_string())
        );
    }

    #[test]
    fn test_parse_newest_version_missing_field() {
        let body = r#"{"crate":{"id":"zorath-env"}}"#;
        assert_eq!(parse_newest_version(body).unwrap(), None);
    }

    #[test]
    fn test_parse_newest_version_missing_crate_object() {
        let body = r#"{"unrelated":42}"#;
        assert_eq!(parse_newest_version(body).unwrap(), None);
    }

    #[test]
    fn test_parse_newest_version_invalid_json() {
        let body = "not json at all";
        let result = parse_newest_version(body);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid JSON"));
    }

    #[test]
    fn test_parse_newest_version_wrong_type() {
        // `newest_version` is not a string -- treat as missing rather than
        // exploding, since the field exists but isn't useful.
        let body = r#"{"crate":{"newest_version":42}}"#;
        assert_eq!(parse_newest_version(body).unwrap(), None);
    }

    #[test]
    fn test_current_version_format() {
        let version = env!("CARGO_PKG_VERSION");
        let parts: Vec<&str> = version.split('.').collect();
        assert!(parts.len() >= 3, "Version should have at least 3 parts");

        assert!(parts[0].parse::<u32>().is_ok());
        assert!(parts[1].parse::<u32>().is_ok());
        let patch = parts[2].split('-').next().unwrap();
        assert!(patch.parse::<u32>().is_ok());
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
