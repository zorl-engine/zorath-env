use serde::Deserialize;
use std::env;
use std::fs;
use std::path::PathBuf;

const CONFIG_FILENAME: &str = ".zenvrc";

/// Configuration loaded from .zenvrc file
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    /// Path to schema file
    #[serde(default)]
    pub schema: Option<String>,

    /// Path to env file
    #[serde(default)]
    pub env: Option<String>,

    /// Allow missing .env file
    #[serde(default)]
    pub allow_missing_env: Option<bool>,

    /// Enable secret detection
    #[serde(default)]
    pub detect_secrets: Option<bool>,

    /// Skip remote schema cache
    #[serde(default)]
    pub no_cache: Option<bool>,

    /// Disable colored output (also respects NO_COLOR env var)
    #[serde(default)]
    pub no_color: Option<bool>,

    // Security options for remote schemas

    /// Expected SHA-256 hash for remote schema integrity verification
    #[serde(default)]
    pub verify_hash: Option<String>,

    /// Custom CA certificate path for enterprise TLS
    #[serde(default)]
    pub ca_cert: Option<String>,

    /// Rate limit in seconds between remote schema fetches (default: 60)
    #[serde(default)]
    pub rate_limit_seconds: Option<u64>,

    /// Default output format (text, json, markdown, etc.)
    #[serde(default)]
    pub format: Option<String>,
}

/// Known valid config keys
const VALID_CONFIG_KEYS: &[&str] = &[
    "schema",
    "env",
    "allow_missing_env",
    "detect_secrets",
    "no_cache",
    "no_color",
    "verify_hash",
    "ca_cert",
    "rate_limit_seconds",
    "format",
];

impl Config {
    /// Load config from a specific path or .zenvrc in current or parent directories
    pub fn load() -> Option<Self> {
        Self::load_from(None)
    }

    /// Load config from a specific path, or search current/parent directories
    pub fn load_from(path: Option<&str>) -> Option<Self> {
        let config_path = if let Some(p) = path {
            let pb = PathBuf::from(p);
            if !pb.exists() {
                eprintln!("zenv warning: config file not found: {}", p);
                return None;
            }
            pb
        } else {
            find_config_file()?
        };
        let content = fs::read_to_string(&config_path).ok()?;

        // First parse as Value to check for unknown keys
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(obj) = value.as_object() {
                let unknown_keys: Vec<&String> = obj
                    .keys()
                    .filter(|k| !VALID_CONFIG_KEYS.contains(&k.as_str()))
                    .collect();

                if !unknown_keys.is_empty() && env::var("ZENV_QUIET").is_err() {
                    eprintln!(
                        "zenv warning: unknown key(s) in {}: {}",
                        config_path.display(),
                        unknown_keys
                            .iter()
                            .map(|k| format!("'{}'", k))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    eprintln!(
                        "  Valid keys: {}",
                        VALID_CONFIG_KEYS.join(", ")
                    );
                }
            }
        }

        match serde_json::from_str::<Config>(&content) {
            Ok(config) => {
                // Only print if config was successfully loaded and has content
                if (config.schema.is_some() || config.env.is_some())
                    && env::var("ZENV_QUIET").is_err()
                {
                    eprintln!("zenv: loaded config from {}", config_path.display());
                }
                Some(config)
            }
            Err(e) => {
                if env::var("ZENV_QUIET").is_err() {
                    eprintln!("zenv warning: invalid .zenvrc at {}: {}", config_path.display(), e);
                }
                None
            }
        }
    }

    /// Get schema path with fallback to default
    pub fn schema_or(&self, default: &str) -> String {
        self.schema.clone().unwrap_or_else(|| default.to_string())
    }

    /// Get env path with fallback to default
    pub fn env_or(&self, default: &str) -> String {
        self.env.clone().unwrap_or_else(|| default.to_string())
    }

    /// Get allow_missing_env with fallback to default
    pub fn allow_missing_env_or(&self, default: bool) -> bool {
        self.allow_missing_env.unwrap_or(default)
    }

    /// Get detect_secrets with fallback to default
    pub fn detect_secrets_or(&self, default: bool) -> bool {
        self.detect_secrets.unwrap_or(default)
    }

    /// Get no_cache with fallback to default
    pub fn no_cache_or(&self, default: bool) -> bool {
        self.no_cache.unwrap_or(default)
    }

    /// Get no_color setting (also respects NO_COLOR env var)
    pub fn no_color_or(&self, default: bool) -> bool {
        // NO_COLOR environment variable takes precedence (https://no-color.org/)
        if env::var("NO_COLOR").is_ok() {
            return true;
        }
        self.no_color.unwrap_or(default)
    }

    /// Get verify_hash setting
    pub fn verify_hash(&self) -> Option<String> {
        self.verify_hash.clone()
    }

    /// Get ca_cert setting
    pub fn ca_cert(&self) -> Option<String> {
        self.ca_cert.clone()
    }

    /// Get rate_limit_seconds setting
    pub fn rate_limit_seconds(&self) -> Option<u64> {
        self.rate_limit_seconds
    }

    /// Get format with fallback to default
    pub fn format_or(&self, default: &str) -> String {
        self.format.clone().unwrap_or_else(|| default.to_string())
    }
}

/// Find .zenvrc file starting from current directory and walking up
fn find_config_file() -> Option<PathBuf> {
    let mut current = env::current_dir().ok()?;

    loop {
        let config_path = current.join(CONFIG_FILENAME);
        if config_path.exists() {
            return Some(config_path);
        }

        // Move to parent directory
        if !current.pop() {
            break;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_config() {
        let json = r#"{
            "schema": "custom.schema.json",
            "env": ".env.local",
            "allow_missing_env": true,
            "detect_secrets": true,
            "no_cache": false,
            "verify_hash": "abc123",
            "ca_cert": "/path/to/cert.pem",
            "rate_limit_seconds": 120
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.schema, Some("custom.schema.json".to_string()));
        assert_eq!(config.env, Some(".env.local".to_string()));
        assert_eq!(config.allow_missing_env, Some(true));
        assert_eq!(config.detect_secrets, Some(true));
        assert_eq!(config.no_cache, Some(false));
        assert_eq!(config.verify_hash, Some("abc123".to_string()));
        assert_eq!(config.ca_cert, Some("/path/to/cert.pem".to_string()));
        assert_eq!(config.rate_limit_seconds, Some(120));
    }

    #[test]
    fn test_parse_partial_config() {
        let json = r#"{"schema": "my.schema.json"}"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.schema, Some("my.schema.json".to_string()));
        assert_eq!(config.env, None);
        assert_eq!(config.allow_missing_env, None);
    }

    #[test]
    fn test_parse_empty_config() {
        let json = "{}";

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.schema, None);
        assert_eq!(config.env, None);
    }

    #[test]
    fn test_schema_or_default() {
        let config = Config {
            schema: Some("custom.json".to_string()),
            ..Default::default()
        };
        assert_eq!(config.schema_or("default.json"), "custom.json");

        let empty = Config::default();
        assert_eq!(empty.schema_or("default.json"), "default.json");
    }

    #[test]
    fn test_env_or_default() {
        let config = Config {
            env: Some(".env.prod".to_string()),
            ..Default::default()
        };
        assert_eq!(config.env_or(".env"), ".env.prod");

        let empty = Config::default();
        assert_eq!(empty.env_or(".env"), ".env");
    }

    #[test]
    fn test_bool_or_defaults() {
        let config = Config {
            allow_missing_env: Some(true),
            detect_secrets: Some(true),
            no_cache: Some(true),
            ..Default::default()
        };

        assert!(config.allow_missing_env_or(false));
        assert!(config.detect_secrets_or(false));
        assert!(config.no_cache_or(false));

        let empty = Config::default();
        assert!(!empty.allow_missing_env_or(false));
        assert!(!empty.detect_secrets_or(false));
        assert!(!empty.no_cache_or(false));
    }

    #[test]
    fn test_find_config_file_returns_path() {
        // Test that find_config_file() searches current dir
        // We just verify it doesn't panic and returns Option
        let result = find_config_file();
        // Result can be Some or None depending on whether .zenvrc exists
        // The important thing is it doesn't crash
        let _ = result;
    }

    #[test]
    fn test_load_valid_json_string() {
        // Test Config parsing directly without filesystem
        let json = r#"{"schema": "test.json", "env": ".env.local"}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.schema, Some("test.json".to_string()));
        assert_eq!(config.env, Some(".env.local".to_string()));
    }

    #[test]
    fn test_load_invalid_json_string() {
        // Test that invalid JSON fails to parse
        let json = "not valid json";
        let result: Result<Config, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_valid_config_keys_list() {
        // Ensure all struct fields are in the valid keys list
        assert!(VALID_CONFIG_KEYS.contains(&"schema"));
        assert!(VALID_CONFIG_KEYS.contains(&"env"));
        assert!(VALID_CONFIG_KEYS.contains(&"allow_missing_env"));
        assert!(VALID_CONFIG_KEYS.contains(&"detect_secrets"));
        assert!(VALID_CONFIG_KEYS.contains(&"no_cache"));
        assert!(VALID_CONFIG_KEYS.contains(&"no_color"));
        assert!(VALID_CONFIG_KEYS.contains(&"verify_hash"));
        assert!(VALID_CONFIG_KEYS.contains(&"ca_cert"));
        assert!(VALID_CONFIG_KEYS.contains(&"rate_limit_seconds"));
        assert!(VALID_CONFIG_KEYS.contains(&"format"));
    }

    #[test]
    fn test_unknown_keys_ignored_but_parseable() {
        // Unknown keys should not prevent parsing
        let json = r#"{"schema": "test.json", "unknown_key": "value"}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.schema, Some("test.json".to_string()));
    }

    // ====== Edge Case Tests ======

    #[test]
    fn test_config_default_values() {
        let config = Config::default();
        assert!(config.schema.is_none());
        assert!(config.env.is_none());
        assert!(config.allow_missing_env.is_none());
        assert!(config.detect_secrets.is_none());
        assert!(config.no_cache.is_none());
        assert!(config.no_color.is_none());
        assert!(config.verify_hash.is_none());
        assert!(config.ca_cert.is_none());
        assert!(config.rate_limit_seconds.is_none());
        assert!(config.format.is_none());
    }

    #[test]
    fn test_config_all_fields_set() {
        let config = Config {
            schema: Some("test.json".to_string()),
            env: Some(".env.test".to_string()),
            allow_missing_env: Some(true),
            detect_secrets: Some(true),
            no_cache: Some(true),
            no_color: Some(true),
            verify_hash: Some("abc123".to_string()),
            ca_cert: Some("/path/to/cert.pem".to_string()),
            rate_limit_seconds: Some(60),
            format: Some("json".to_string()),
        };

        assert_eq!(config.schema_or("default"), "test.json");
        assert_eq!(config.env_or("default"), ".env.test");
        assert!(config.allow_missing_env_or(false));
        assert!(config.detect_secrets_or(false));
        assert!(config.no_cache_or(false));
        assert!(config.no_color_or(false));
    }

    // ====== Config File Edge Cases ======

    #[test]
    fn test_config_with_unknown_keys_still_parses() {
        // Unknown keys should not prevent parsing the valid keys
        let json = r#"{
            "schema": "test.json",
            "unknown_key_1": "value1",
            "another_unknown": 123,
            "env": ".env.test"
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.schema, Some("test.json".to_string()));
        assert_eq!(config.env, Some(".env.test".to_string()));
    }

    #[test]
    fn test_config_all_valid_keys() {
        // All valid keys should be recognized
        for key in VALID_CONFIG_KEYS.iter() {
            assert!(
                ["schema", "env", "allow_missing_env", "detect_secrets",
                 "no_cache", "no_color", "verify_hash", "ca_cert", "rate_limit_seconds", "format"]
                    .contains(key),
                "Key '{}' is in VALID_CONFIG_KEYS but not in expected list",
                key
            );
        }
    }

    #[test]
    fn test_config_empty_values() {
        // Empty string values should be allowed
        let json = r#"{"schema": "", "env": ""}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.schema, Some("".to_string()));
        assert_eq!(config.env, Some("".to_string()));
    }

    #[test]
    fn test_config_rate_limit_edge_values() {
        // Zero rate limit should be valid
        let json = r#"{"rate_limit_seconds": 0}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.rate_limit_seconds, Some(0));

        // Large rate limit should be valid
        let json2 = r#"{"rate_limit_seconds": 86400}"#;
        let config2: Config = serde_json::from_str(json2).unwrap();
        assert_eq!(config2.rate_limit_seconds, Some(86400));
    }
}
