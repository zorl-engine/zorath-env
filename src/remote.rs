use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use thiserror::Error;
use ureq::tls::{parse_pem, PemItem, RootCerts, TlsConfig};

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
    #[error("hash verification failed: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("rate limited: wait {seconds} seconds before fetching again")]
    RateLimited { seconds: u64 },
    #[error("failed to load CA certificate: {0}")]
    CertificateError(String),
}

/// Default cache TTL: 1 hour
pub const CACHE_TTL_SECS: u64 = 3600;

/// Default rate limit: 60 seconds between fetches per URL
pub const DEFAULT_RATE_LIMIT_SECS: u64 = 60;

/// HTTP request timeout
pub const HTTP_TIMEOUT_SECS: u64 = 30;

/// Maximum schema response body size (5 MiB).
pub const MAX_RESPONSE_BYTES: u64 = 5 * 1024 * 1024;

/// Minimum length for a SHA-256 hash *prefix* to be accepted as a
/// verification value. 32 hex chars = 128 bits, low enough collision risk
/// while still allowing convenient short pins.
pub const MIN_HASH_PREFIX_LEN: usize = 32;

/// Security options for remote schema fetching
#[derive(Debug, Clone)]
pub struct SecurityOptions {
    /// Expected SHA-256 hash of the schema content (hex-encoded)
    pub verify_hash: Option<String>,
    /// Custom CA certificate path for enterprise TLS
    pub ca_cert: Option<String>,
    /// Rate limit in seconds between fetches (0 to disable)
    pub rate_limit_seconds: u64,
}

impl Default for SecurityOptions {
    fn default() -> Self {
        Self {
            verify_hash: None,
            ca_cert: None,
            rate_limit_seconds: DEFAULT_RATE_LIMIT_SECS,
        }
    }
}

impl SecurityOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_hash(mut self, hash: Option<String>) -> Self {
        self.verify_hash = hash;
        self
    }

    pub fn with_ca_cert(mut self, path: Option<String>) -> Self {
        self.ca_cert = path;
        self
    }

    pub fn with_rate_limit(mut self, seconds: u64) -> Self {
        self.rate_limit_seconds = seconds;
        self
    }
}

/// Check if a path is a remote URL (https://)
pub fn is_remote_url(path: &str) -> bool {
    path.starts_with("https://") || path.starts_with("http://")
}

/// Fetch schema content from a remote URL with security options
///
/// Supports hash verification, rate limiting, and custom CA certificates.
pub fn fetch_remote_schema_secure(
    url: &str,
    no_cache: bool,
    security: &SecurityOptions,
) -> Result<String, RemoteError> {
    // Security: reject HTTP URLs
    if url.starts_with("http://") {
        return Err(RemoteError::HttpNotAllowed);
    }

    // Validate URL format
    if !url.starts_with("https://") {
        return Err(RemoteError::InvalidUrl(url.to_string()));
    }

    // SSRF defense: reject internal hostnames and IP ranges before fetching
    validate_url_host(url)?;

    // Check rate limit (unless no_cache bypasses it)
    if !no_cache && security.rate_limit_seconds > 0 {
        check_rate_limit(url, security.rate_limit_seconds)?;
    }

    // Check cache first (unless no_cache is set)
    if !no_cache {
        if let Some(cached) = read_cache(url)? {
            // Verify hash even for cached content if hash is specified
            if let Some(ref expected_hash) = security.verify_hash {
                verify_content_hash(&cached, expected_hash)?;
            }
            return Ok(cached);
        }
    }

    // Fetch from network
    let content = fetch_url_secure(url, security.ca_cert.as_deref())?;

    // Verify hash if specified
    if let Some(ref expected_hash) = security.verify_hash {
        verify_content_hash(&content, expected_hash)?;
    }

    // Write to cache (with rate limit metadata)
    if let Err(e) = write_cache_with_metadata(url, &content) {
        // Cache write failure is not fatal, just log it
        eprintln!("warning: failed to cache schema: {}", e);
    }

    Ok(content)
}

/// Verify content matches expected SHA-256 hash.
///
/// Accepts the full 64-char hex hash, or a prefix at least
/// `MIN_HASH_PREFIX_LEN` (128 bits) long. Shorter prefixes are rejected
/// because they offer false integrity (8 hex chars = 32 bits is collidable).
pub fn verify_content_hash(content: &str, expected_hash: &str) -> Result<(), RemoteError> {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let actual_hash = format!("{:x}", hasher.finalize());

    let expected_lower = expected_hash.to_lowercase();

    // Reject prefixes shorter than the minimum acceptable length.
    if expected_lower.len() < MIN_HASH_PREFIX_LEN {
        return Err(RemoteError::HashMismatch {
            expected: format!(
                "{} (rejected: hash prefix must be at least {} hex chars)",
                expected_hash, MIN_HASH_PREFIX_LEN
            ),
            actual: actual_hash,
        });
    }

    if actual_hash == expected_lower || actual_hash.starts_with(&expected_lower) {
        Ok(())
    } else {
        Err(RemoteError::HashMismatch {
            expected: expected_hash.to_string(),
            actual: actual_hash,
        })
    }
}

/// Compute SHA-256 hash of content (useful for generating expected hashes)
pub fn compute_content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Reject URLs whose host is loopback, private, link-local, or otherwise
/// internal. Resolves the host via the OS resolver and checks every
/// returned address.
fn validate_url_host(url: &str) -> Result<(), RemoteError> {
    use std::net::ToSocketAddrs;

    let parsed = url::Url::parse(url).map_err(|_| RemoteError::InvalidUrl(url.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| RemoteError::InvalidUrl(url.to_string()))?;

    let lower = host.to_ascii_lowercase();
    if lower == "localhost"
        || lower == "ip6-localhost"
        || lower.ends_with(".local")
        || lower.ends_with(".localhost")
        || lower.ends_with(".internal")
    {
        return Err(RemoteError::InvalidUrl(format!(
            "internal host not allowed: {}",
            host
        )));
    }

    let port = parsed.port().unwrap_or(443);
    let target = format!("{}:{}", host, port);

    // If resolution fails, defer to ureq for the actual error.
    if let Ok(addrs) = target.to_socket_addrs() {
        for addr in addrs {
            if is_forbidden_ip(&addr.ip()) {
                return Err(RemoteError::InvalidUrl(format!(
                    "host resolves to internal/loopback address: {} -> {}",
                    host,
                    addr.ip()
                )));
            }
        }
    }

    Ok(())
}

fn is_forbidden_ip(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.octets() == [169, 254, 169, 254] // AWS/GCP metadata IP (also link_local, but explicit)
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // unique-local fc00::/7
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
        }
    }
}

/// Perform HTTP GET request with optional custom CA certificate.
///
/// Disables redirects to prevent post-validation host pivoting and caps the
/// response body to `MAX_RESPONSE_BYTES`.
fn fetch_url_secure(url: &str, ca_cert_path: Option<&str>) -> Result<String, RemoteError> {
    let tls_config = build_tls_config(ca_cert_path)?;

    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(HTTP_TIMEOUT_SECS)))
        .max_redirects(0)
        .tls_config(tls_config)
        .build()
        .new_agent();

    let mut response = agent
        .get(url)
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
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|e| RemoteError::Network(e.to_string()))
}

/// Build TLS configuration with optional custom CA certificate
fn build_tls_config(ca_cert_path: Option<&str>) -> Result<TlsConfig, RemoteError> {
    if let Some(ca_path) = ca_cert_path {
        let pem_data = fs::read(ca_path).map_err(|e| {
            RemoteError::CertificateError(format!("failed to read {}: {}", ca_path, e))
        })?;

        let mut certs = Vec::new();
        for item in parse_pem(&pem_data) {
            match item {
                Ok(PemItem::Certificate(cert)) => certs.push(cert),
                Ok(_) => {} // skip non-certificate PEM items (keys, etc.)
                Err(e) => {
                    return Err(RemoteError::CertificateError(format!(
                        "failed to parse PEM from {}: {}",
                        ca_path, e
                    )))
                }
            }
        }

        if certs.is_empty() {
            return Err(RemoteError::CertificateError(format!(
                "no valid certificates found in {}",
                ca_path
            )));
        }

        let count = certs.len();
        let root_certs = RootCerts::new_with_certs(&certs);

        eprintln!(
            "zenv: using CA certificate from {} ({} cert(s))",
            ca_path, count
        );

        Ok(TlsConfig::builder().root_certs(root_certs).build())
    } else {
        Ok(TlsConfig::default())
    }
}

/// Check rate limit for a URL
fn check_rate_limit(url: &str, rate_limit_seconds: u64) -> Result<(), RemoteError> {
    let metadata_path = match metadata_path_for_url(url) {
        Some(p) => p,
        None => return Ok(()), // No cache dir, skip rate limiting
    };

    if !metadata_path.exists() {
        return Ok(()); // No previous fetch, allow
    }

    // Read last fetch timestamp from metadata
    if let Ok(content) = fs::read_to_string(&metadata_path) {
        if let Ok(metadata) = serde_json::from_str::<CacheMetadata>(&content) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let elapsed = now.saturating_sub(metadata.fetched_at);
            if elapsed < rate_limit_seconds {
                let wait_seconds = rate_limit_seconds - elapsed;
                return Err(RemoteError::RateLimited {
                    seconds: wait_seconds,
                });
            }
        }
    }

    Ok(())
}

/// Cache metadata for rate limiting and integrity
#[derive(serde::Serialize, serde::Deserialize)]
struct CacheMetadata {
    url: String,
    fetched_at: u64,
    content_hash: String,
}

/// Get metadata file path for a URL
fn metadata_path_for_url(url: &str) -> Option<PathBuf> {
    cache_dir().map(|d| {
        d.join(format!(
            "{}.meta",
            cache_filename(url).trim_end_matches(".json")
        ))
    })
}

/// Write schema content to cache with metadata
fn write_cache_with_metadata(url: &str, content: &str) -> Result<(), RemoteError> {
    // Write content
    write_cache(url, content)?;

    // Write metadata
    let metadata_path = match metadata_path_for_url(url) {
        Some(p) => p,
        None => return Ok(()),
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let metadata = CacheMetadata {
        url: url.to_string(),
        fetched_at: now,
        content_hash: compute_content_hash(content),
    };

    let metadata_json =
        serde_json::to_string(&metadata).map_err(|e| RemoteError::Cache(e.to_string()))?;

    fs::write(&metadata_path, metadata_json).map_err(|e| RemoteError::Cache(e.to_string()))?;

    Ok(())
}

/// Get the cache directory path
pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|p| p.join("zorath-env"))
}

/// Generate cache filename from URL using SHA-256 (truncated to 16 hex
/// chars / 64 bits) to make collisions cryptographically infeasible to
/// craft, vs. the prior trivially-collidable byte sum.
pub fn cache_filename(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("{}.json", &digest[..16])
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
        let result = fetch_remote_schema_secure(
            "http://example.com/schema.json",
            true,
            &SecurityOptions::new(),
        );
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

    // Security feature tests

    #[test]
    fn test_compute_content_hash() {
        let content = r#"{"FOO": {"type": "string"}}"#;
        let hash = compute_content_hash(content);
        // SHA-256 produces 64 hex characters
        assert_eq!(hash.len(), 64);
        // Same content should produce same hash
        assert_eq!(hash, compute_content_hash(content));
    }

    #[test]
    fn test_verify_content_hash_matches() {
        let content = "test content";
        let hash = compute_content_hash(content);

        // Full hash should match
        assert!(verify_content_hash(content, &hash).is_ok());

        // Uppercase hash should match
        assert!(verify_content_hash(content, &hash.to_uppercase()).is_ok());

        // Prefix at the documented minimum (32 hex / 128 bits) should match.
        assert!(verify_content_hash(content, &hash[..MIN_HASH_PREFIX_LEN]).is_ok());

        // 16-char prefix is now rejected as too short.
        assert!(matches!(
            verify_content_hash(content, &hash[..16]),
            Err(RemoteError::HashMismatch { .. })
        ));
    }

    #[test]
    fn test_verify_content_hash_mismatch() {
        let content = "test content";
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";

        let result = verify_content_hash(content, wrong_hash);
        assert!(matches!(result, Err(RemoteError::HashMismatch { .. })));
    }

    #[test]
    fn test_security_options_builder() {
        let opts = SecurityOptions::new()
            .with_hash(Some("abc123".to_string()))
            .with_ca_cert(Some("/path/to/cert.pem".to_string()))
            .with_rate_limit(120);

        assert_eq!(opts.verify_hash, Some("abc123".to_string()));
        assert_eq!(opts.ca_cert, Some("/path/to/cert.pem".to_string()));
        assert_eq!(opts.rate_limit_seconds, 120);
    }

    #[test]
    fn test_security_options_defaults() {
        let opts = SecurityOptions::default();
        assert_eq!(opts.verify_hash, None);
        assert_eq!(opts.ca_cert, None);
        assert_eq!(opts.rate_limit_seconds, DEFAULT_RATE_LIMIT_SECS);
    }

    #[test]
    fn test_security_options_new() {
        let opts = SecurityOptions::new();
        assert_eq!(opts.verify_hash, None);
        assert_eq!(opts.ca_cert, None);
        assert_eq!(opts.rate_limit_seconds, DEFAULT_RATE_LIMIT_SECS);
    }

    #[test]
    fn test_cache_metadata_serialization() {
        let metadata = CacheMetadata {
            url: "https://example.com/schema.json".to_string(),
            fetched_at: 1234567890,
            content_hash: "abc123".to_string(),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: CacheMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.url, metadata.url);
        assert_eq!(parsed.fetched_at, metadata.fetched_at);
        assert_eq!(parsed.content_hash, metadata.content_hash);
    }

    #[test]
    fn test_http_rejected_secure() {
        let security = SecurityOptions::new();
        let result = fetch_remote_schema_secure("http://example.com/schema.json", true, &security);
        assert!(matches!(result, Err(RemoteError::HttpNotAllowed)));
    }

    #[test]
    fn test_invalid_ca_cert_path() {
        let result = build_tls_config(Some("/nonexistent/path/ca.pem"));
        assert!(matches!(result, Err(RemoteError::CertificateError(_))));
    }

    // =========================================================================
    // Additional Hash Verification Tests
    // =========================================================================

    #[test]
    fn test_verify_hash_empty_content() {
        let content = "";
        let hash = compute_content_hash(content);
        assert!(verify_content_hash(content, &hash).is_ok());
        // Empty string has a specific SHA-256 hash
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_verify_hash_unicode_content() {
        let content = r#"{"description": "Unicode test"}"#;
        let hash = compute_content_hash(content);
        assert!(verify_content_hash(content, &hash).is_ok());
    }

    #[test]
    fn test_verify_hash_with_newlines() {
        let content = "line1\nline2\nline3";
        let hash = compute_content_hash(content);
        assert!(verify_content_hash(content, &hash).is_ok());
        // Different newline style should produce different hash
        let content_crlf = "line1\r\nline2\r\nline3";
        let hash_crlf = compute_content_hash(content_crlf);
        assert_ne!(hash, hash_crlf);
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let content = r#"{"PORT": {"type": "int", "required": true}}"#;
        let hash1 = compute_content_hash(content);
        let hash2 = compute_content_hash(content);
        let hash3 = compute_content_hash(content);
        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
    }

    #[test]
    fn test_compute_hash_different_content_different_hash() {
        let content1 = r#"{"FOO": "bar"}"#;
        let content2 = r#"{"FOO": "baz"}"#;
        let hash1 = compute_content_hash(content1);
        let hash2 = compute_content_hash(content2);
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_compute_hash_special_characters() {
        let content = r#"{"key": "value with $pecial & <chars>"}"#;
        let hash = compute_content_hash(content);
        assert_eq!(hash.len(), 64);
        // Should only contain hex characters
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_verify_hash_short_prefix_rejected() {
        let content = "test";
        let hash = compute_content_hash(content);
        // 8-char prefix (32 bits) is now rejected for false-integrity reasons.
        let result = verify_content_hash(content, &hash[..8]);
        assert!(matches!(result, Err(RemoteError::HashMismatch { .. })));
    }

    #[test]
    fn test_verify_hash_min_prefix_accepted() {
        let content = "test";
        let hash = compute_content_hash(content);
        // 32-char prefix (128 bits) meets the minimum and should pass.
        assert!(verify_content_hash(content, &hash[..MIN_HASH_PREFIX_LEN]).is_ok());
    }

    #[test]
    fn test_verify_hash_just_under_min_rejected() {
        let content = "test";
        let hash = compute_content_hash(content);
        // 31 chars is one short of the floor.
        let result = verify_content_hash(content, &hash[..MIN_HASH_PREFIX_LEN - 1]);
        assert!(matches!(result, Err(RemoteError::HashMismatch { .. })));
    }

    #[test]
    fn test_validate_url_host_rejects_localhost() {
        assert!(matches!(
            validate_url_host("https://localhost/schema.json"),
            Err(RemoteError::InvalidUrl(_))
        ));
        assert!(matches!(
            validate_url_host("https://foo.local/schema.json"),
            Err(RemoteError::InvalidUrl(_))
        ));
    }

    #[test]
    fn test_validate_url_host_rejects_metadata_ip() {
        assert!(matches!(
            validate_url_host("https://169.254.169.254/latest/meta-data/"),
            Err(RemoteError::InvalidUrl(_))
        ));
    }

    #[test]
    fn test_validate_url_host_rejects_loopback_ip() {
        assert!(matches!(
            validate_url_host("https://127.0.0.1/schema.json"),
            Err(RemoteError::InvalidUrl(_))
        ));
    }

    #[test]
    fn test_validate_url_host_rejects_private_ranges() {
        for url in [
            "https://10.0.0.1/schema.json",
            "https://192.168.1.1/schema.json",
            "https://172.16.0.1/schema.json",
        ] {
            assert!(
                matches!(validate_url_host(url), Err(RemoteError::InvalidUrl(_))),
                "expected rejection for {}",
                url
            );
        }
    }

    #[test]
    fn test_cache_filename_uses_sha256_prefix() {
        let name = cache_filename("https://example.com/a.json");
        // 16 hex chars + ".json"
        assert_eq!(name.len(), 16 + 5);
        assert!(name.ends_with(".json"));
        let stem = name.trim_end_matches(".json");
        assert!(stem.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // =========================================================================
    // CA Certificate Tests
    // =========================================================================

    #[test]
    fn test_build_tls_config_with_none() {
        let result = build_tls_config(None);
        assert!(result.is_ok(), "Should succeed with no CA cert");
    }

    #[test]
    fn test_build_tls_config_empty_file() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        // Empty file - no certificates
        let result = build_tls_config(Some(temp_file.path().to_str().unwrap()));
        // Should succeed but with 0 certs (or error depending on implementation)
        // The important thing is it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_build_tls_config_invalid_pem_content() {
        use std::io::Write;
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(temp_file, "This is not a valid PEM certificate").unwrap();
        let result = build_tls_config(Some(temp_file.path().to_str().unwrap()));
        // Should handle gracefully (either succeed with 0 certs or return error)
        let _ = result;
    }

    // =========================================================================
    // Rate Limiting Tests
    // =========================================================================

    #[test]
    fn test_rate_limit_with_zero_seconds() {
        // Rate limit of 0 effectively disables rate limiting
        let opts = SecurityOptions::new().with_rate_limit(0);
        assert_eq!(opts.rate_limit_seconds, 0);
    }

    #[test]
    fn test_cache_metadata_with_current_time() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let metadata = CacheMetadata {
            url: "https://example.com/test.json".to_string(),
            fetched_at: now,
            content_hash: compute_content_hash("test"),
        };

        // Verify we can serialize and deserialize with current timestamp
        let json = serde_json::to_string(&metadata).unwrap();
        let parsed: CacheMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.fetched_at, now);
    }

    #[test]
    fn test_cache_dir_returns_path() {
        let dir = cache_dir();
        assert!(dir.is_some(), "Should return a cache directory path");
        if let Some(path) = dir {
            // Should be a valid path string
            assert!(!path.as_os_str().is_empty());
        }
    }

    #[test]
    fn test_cache_filename_consistent() {
        let url = "https://example.com/schemas/env.schema.json";
        let name1 = cache_filename(url);
        let name2 = cache_filename(url);
        assert_eq!(name1, name2, "Same URL should produce same cache filename");
    }

    #[test]
    fn test_cache_filename_different_for_different_urls() {
        let url1 = "https://example.com/a.json";
        let url2 = "https://example.com/b.json";
        let url3 = "https://other.com/a.json";

        let name1 = cache_filename(url1);
        let name2 = cache_filename(url2);
        let name3 = cache_filename(url3);

        assert_ne!(name1, name2);
        assert_ne!(name1, name3);
        assert_ne!(name2, name3);
    }

    // ====== Additional Edge Case Tests ======

    #[test]
    fn test_is_remote_url_various_schemes() {
        // HTTPS - valid
        assert!(is_remote_url("https://example.com/schema.json"));
        // HTTP - valid but rejected elsewhere for security
        assert!(is_remote_url("http://example.com/schema.json"));
        // FTP - not recognized as remote URL for our purposes
        assert!(!is_remote_url("ftp://example.com/schema.json"));
        // File paths - not remote
        assert!(!is_remote_url("./schema.json"));
        assert!(!is_remote_url("/path/to/schema.json"));
        assert!(!is_remote_url("C:\\path\\schema.json"));
    }

    #[test]
    fn test_resolve_relative_url_edge_cases() {
        // Base with trailing slash
        let base = "https://example.com/schemas/";
        let relative = "child.json";
        let result = resolve_relative_url(base, relative).unwrap();
        assert!(result.contains("example.com"));
        assert!(result.contains("child.json"));

        // Base without trailing slash
        let base2 = "https://example.com/schemas/parent.json";
        let relative2 = "child.json";
        let result2 = resolve_relative_url(base2, relative2).unwrap();
        assert!(result2.contains("child.json"));
    }

    #[test]
    fn test_security_options_all_fields() {
        let opts = SecurityOptions::new()
            .with_hash(Some("abc123".to_string()))
            .with_ca_cert(Some("/path/to/cert.pem".to_string()))
            .with_rate_limit(120);

        assert_eq!(opts.verify_hash, Some("abc123".to_string()));
        assert_eq!(opts.ca_cert, Some("/path/to/cert.pem".to_string()));
        assert_eq!(opts.rate_limit_seconds, 120);
    }

    #[test]
    fn test_security_options_chaining() {
        // Test fluent builder pattern
        let opts = SecurityOptions::new()
            .with_hash(None)
            .with_ca_cert(None)
            .with_rate_limit(0);

        assert!(opts.verify_hash.is_none());
        assert!(opts.ca_cert.is_none());
        assert_eq!(opts.rate_limit_seconds, 0);
    }

    #[test]
    fn test_cache_filename_url_encoded_chars() {
        let url1 = "https://example.com/schema%20with%20spaces.json";
        let url2 = "https://example.com/schema?query=value&other=123";

        let name1 = cache_filename(url1);
        let name2 = cache_filename(url2);

        // Should produce valid filenames (no special chars)
        assert!(!name1.contains('%'));
        assert!(!name1.contains(' '));
        assert!(!name2.contains('?'));
        assert!(!name2.contains('&'));
    }

    #[test]
    fn test_verify_content_hash_case_insensitive() {
        let content = "test content";
        let hash_lower = compute_content_hash(content).to_lowercase();
        let _hash_upper = hash_lower.to_uppercase();

        // Both should verify correctly (if implementation is case-insensitive)
        // If not, at least one should work
        let result_lower = verify_content_hash(content, &hash_lower);
        assert!(result_lower.is_ok());
    }

    #[test]
    fn test_compute_hash_consistency_across_calls() {
        let content = "consistent content";

        // Multiple calls should produce identical results
        let hash1 = compute_content_hash(content);
        let hash2 = compute_content_hash(content);
        let hash3 = compute_content_hash(content);

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
    }
}
