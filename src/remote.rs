use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RemoteError {
    #[error("HTTP request failed: {0}")]
    Network(String),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("HTTP error: {0}")]
    HttpStatus(String),
    #[error("cache error: {0}")]
    Cache(String),
    #[error("only HTTPS URLs are allowed for security")]
    HttpNotAllowed,
}

/// Default cache TTL: 1 hour
const CACHE_TTL_SECS: u64 = 3600;

/// Check if a path is a remote URL (https://)
pub fn is_remote_url(path: &str) -> bool {
    path.starts_with("https://") || path.starts_with("http://")
}

/// Fetch schema content from a remote URL
///
/// If `no_cache` is true, always fetches fresh content.
/// Otherwise, uses cached content if available and not expired.
pub fn fetch_remote_schema(url: &str, no_cache: bool) -> Result<String, RemoteError> {
    // Security: reject HTTP URLs
    if url.starts_with("http://") {
        return Err(RemoteError::HttpNotAllowed);
    }

    // Validate URL format
    if !url.starts_with("https://") {
        return Err(RemoteError::InvalidUrl(url.to_string()));
    }

    // Check cache first (unless no_cache is set)
    if !no_cache {
        if let Some(cached) = read_cache(url)? {
            return Ok(cached);
        }
    }

    // Fetch from network
    let content = fetch_url(url)?;

    // Write to cache
    if let Err(e) = write_cache(url, &content) {
        // Cache write failure is not fatal, just log it
        eprintln!("warning: failed to cache schema: {}", e);
    }

    Ok(content)
}

/// Perform HTTP GET request
fn fetch_url(url: &str) -> Result<String, RemoteError> {
    let response = ureq::get(url)
        .timeout(Duration::from_secs(30))
        .call()
        .map_err(|e| RemoteError::Network(e.to_string()))?;

    if response.status() != 200 {
        return Err(RemoteError::HttpStatus(format!(
            "status {} for {}",
            response.status(),
            url
        )));
    }

    response
        .into_string()
        .map_err(|e| RemoteError::Network(e.to_string()))
}

/// Get the cache directory path
fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|p| p.join("zorath-env"))
}

/// Generate cache filename from URL (simple hash)
fn cache_filename(url: &str) -> String {
    // Simple hash: sum of bytes mod large prime, hex encoded
    let hash: u64 = url.bytes().enumerate().fold(0u64, |acc, (i, b)| {
        acc.wrapping_add((b as u64).wrapping_mul((i as u64).wrapping_add(1)))
    });
    format!("{:016x}.json", hash)
}

/// Read cached schema if available and not expired
fn read_cache(url: &str) -> Result<Option<String>, RemoteError> {
    let cache_dir = match cache_dir() {
        Some(dir) => dir,
        None => return Ok(None),
    };

    let cache_path = cache_dir.join(cache_filename(url));

    if !cache_path.exists() {
        return Ok(None);
    }

    // Check if cache is expired
    let metadata = fs::metadata(&cache_path).map_err(|e| RemoteError::Cache(e.to_string()))?;

    let modified = metadata
        .modified()
        .map_err(|e| RemoteError::Cache(e.to_string()))?;

    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::MAX);

    if age.as_secs() > CACHE_TTL_SECS {
        // Cache expired
        return Ok(None);
    }

    // Read cached content
    let content = fs::read_to_string(&cache_path).map_err(|e| RemoteError::Cache(e.to_string()))?;

    Ok(Some(content))
}

/// Write schema content to cache
fn write_cache(url: &str, content: &str) -> Result<(), RemoteError> {
    let cache_dir = match cache_dir() {
        Some(dir) => dir,
        None => return Ok(()), // No cache dir available, skip caching
    };

    // Create cache directory if it doesn't exist
    fs::create_dir_all(&cache_dir).map_err(|e| RemoteError::Cache(e.to_string()))?;

    let cache_path = cache_dir.join(cache_filename(url));

    let mut file = fs::File::create(&cache_path).map_err(|e| RemoteError::Cache(e.to_string()))?;

    file.write_all(content.as_bytes())
        .map_err(|e| RemoteError::Cache(e.to_string()))?;

    Ok(())
}

/// Resolve a relative URL against a base URL
pub fn resolve_relative_url(base_url: &str, relative_path: &str) -> Result<String, RemoteError> {
    // If relative_path is already absolute, return it
    if relative_path.starts_with("https://") || relative_path.starts_with("http://") {
        return Ok(relative_path.to_string());
    }

    // Parse base URL and resolve relative path
    let base = url::Url::parse(base_url).map_err(|e| RemoteError::InvalidUrl(e.to_string()))?;

    let resolved = base
        .join(relative_path)
        .map_err(|e| RemoteError::InvalidUrl(e.to_string()))?;

    Ok(resolved.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_remote_url() {
        assert!(is_remote_url("https://example.com/schema.json"));
        assert!(is_remote_url("http://example.com/schema.json"));
        assert!(!is_remote_url("env.schema.json"));
        assert!(!is_remote_url("./schemas/env.schema.json"));
        assert!(!is_remote_url("/absolute/path/schema.json"));
    }

    #[test]
    fn test_http_rejected() {
        let result = fetch_remote_schema("http://example.com/schema.json", true);
        assert!(matches!(result, Err(RemoteError::HttpNotAllowed)));
    }

    #[test]
    fn test_cache_filename() {
        let name1 = cache_filename("https://example.com/a.json");
        let name2 = cache_filename("https://example.com/b.json");
        assert_ne!(name1, name2);
        assert!(name1.ends_with(".json"));
    }

    #[test]
    fn test_resolve_relative_url() {
        let base = "https://example.com/schemas/prod.json";

        // Relative sibling
        let resolved = resolve_relative_url(base, "base.json").unwrap();
        assert_eq!(resolved, "https://example.com/schemas/base.json");

        // Parent directory
        let resolved = resolve_relative_url(base, "../common.json").unwrap();
        assert_eq!(resolved, "https://example.com/common.json");

        // Absolute URL passthrough
        let resolved = resolve_relative_url(base, "https://other.com/schema.json").unwrap();
        assert_eq!(resolved, "https://other.com/schema.json");
    }
}
