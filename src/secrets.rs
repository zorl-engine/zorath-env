use std::collections::HashMap;
use regex::Regex;

use crate::schema::Schema;

/// A detected potential secret
#[derive(Debug)]
pub struct SecretWarning {
    pub key: String,
    pub line: usize,
    pub reason: String,
}

/// Patterns that indicate potential secrets
struct SecretPattern {
    name: &'static str,
    pattern: Regex,
}

/// Check env values for potential secrets
/// Pass schema to respect `"secret": false` whitelist entries
pub fn detect_secrets(
    env_map: &HashMap<String, String>,
    content: &str,
    schema: Option<&Schema>,
) -> Vec<SecretWarning> {
    let mut warnings = Vec::new();

    // Build line number lookup
    let line_numbers = build_line_lookup(content);

    // Define secret patterns
    let patterns = get_secret_patterns();

    for (key, value) in env_map {
        // Skip empty values
        if value.is_empty() {
            continue;
        }

        // Skip if schema marks this key as safe (secret: false)
        if let Some(schema) = schema {
            if let Some(spec) = schema.get(key) {
                if spec.secret == Some(false) {
                    continue;
                }
            }
        }

        // Check for URLs with embedded passwords first
        if contains_url_password(value) {
            let line = line_numbers.get(key).copied().unwrap_or(0);
            warnings.push(SecretWarning {
                key: key.clone(),
                line,
                reason: "URL contains embedded password".to_string(),
            });
            continue; // Skip other checks for this key
        }

        // Check against all patterns
        let mut pattern_matched = false;
        for pattern in &patterns {
            if pattern.pattern.is_match(value) {
                let line = line_numbers.get(key).copied().unwrap_or(0);
                warnings.push(SecretWarning {
                    key: key.clone(),
                    line,
                    reason: pattern.name.to_string(),
                });
                pattern_matched = true;
                break; // Only report first match per key
            }
        }

        if pattern_matched {
            continue;
        }

        // Check for high-entropy strings (potential secrets)
        if is_high_entropy(value) && value.len() >= 16 {
            let line = line_numbers.get(key).copied().unwrap_or(0);
            warnings.push(SecretWarning {
                key: key.clone(),
                line,
                reason: "High-entropy string (possible secret)".to_string(),
            });
        }
    }

    // Sort by line number
    warnings.sort_by_key(|w| w.line);

    warnings
}

fn get_secret_patterns() -> Vec<SecretPattern> {
    vec![
        // AWS Access Key ID
        SecretPattern {
            name: "AWS Access Key ID",
            pattern: Regex::new(r"^AKIA[0-9A-Z]{16}$").unwrap(),
        },
        // AWS Secret Access Key (40 char base64-ish)
        SecretPattern {
            name: "AWS Secret Access Key",
            pattern: Regex::new(r"^[A-Za-z0-9/+=]{40}$").unwrap(),
        },
        // Stripe API keys
        SecretPattern {
            name: "Stripe API key",
            pattern: Regex::new(r"^(sk|pk)_(live|test)_[0-9a-zA-Z]{24,}$").unwrap(),
        },
        // GitHub tokens
        SecretPattern {
            name: "GitHub token",
            pattern: Regex::new(r"^(ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36,}$").unwrap(),
        },
        // GitLab tokens
        SecretPattern {
            name: "GitLab token",
            pattern: Regex::new(r"^glpat-[A-Za-z0-9\-]{20,}$").unwrap(),
        },
        // Slack tokens
        SecretPattern {
            name: "Slack token",
            pattern: Regex::new(r"^xox[baprs]-[0-9A-Za-z\-]+$").unwrap(),
        },
        // Private key headers
        SecretPattern {
            name: "Private key",
            pattern: Regex::new(r"-----BEGIN (RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY-----").unwrap(),
        },
        // JWT tokens
        SecretPattern {
            name: "JWT token",
            pattern: Regex::new(r"^eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$").unwrap(),
        },
        // Google API keys
        SecretPattern {
            name: "Google API key",
            pattern: Regex::new(r"^AIza[0-9A-Za-z\-_]{35}$").unwrap(),
        },
        // Heroku API key
        SecretPattern {
            name: "Heroku API key",
            pattern: Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").unwrap(),
        },
        // Generic API key patterns (common prefixes)
        SecretPattern {
            name: "API key (common prefix)",
            pattern: Regex::new(r"^(api[_-]?key|apikey|api[_-]?secret)[_-]?[0-9a-zA-Z]{16,}$").unwrap(),
        },
        // npm tokens
        SecretPattern {
            name: "npm token",
            pattern: Regex::new(r"^npm_[A-Za-z0-9]{36}$").unwrap(),
        },
        // SendGrid API key
        SecretPattern {
            name: "SendGrid API key",
            pattern: Regex::new(r"^SG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}$").unwrap(),
        },
        // Twilio credentials
        SecretPattern {
            name: "Twilio credentials",
            pattern: Regex::new(r"^(AC[a-z0-9]{32}|SK[a-z0-9]{32})$").unwrap(),
        },
        // Mailchimp API key
        SecretPattern {
            name: "Mailchimp API key",
            pattern: Regex::new(r"^[a-z0-9]{32}-us[0-9]{1,2}$").unwrap(),
        },
    ]
}

/// Check if a string has high entropy (randomness) - indicator of secrets
fn is_high_entropy(s: &str) -> bool {
    if s.len() < 16 {
        return false;
    }

    // Skip common non-secret patterns
    // URLs without passwords
    if (s.starts_with("http://") || s.starts_with("https://")) && !contains_url_password(s) {
        return false;
    }

    // Skip paths
    if s.starts_with('/') || s.contains(":\\") || s.starts_with("./") {
        return false;
    }

    // Skip common placeholder values
    let lower = s.to_lowercase();
    if lower.contains("example") || lower.contains("placeholder") ||
       lower.contains("changeme") || lower.contains("your_") ||
       lower.contains("xxx") || lower == "development" ||
       lower == "production" || lower == "staging" ||
       lower == "localhost" || lower == "true" || lower == "false" {
        return false;
    }

    // Calculate Shannon entropy
    let entropy = calculate_entropy(s);

    // High entropy threshold (secrets typically have entropy > 3.5)
    entropy > 4.0 && has_mixed_chars(s)
}

fn calculate_entropy(s: &str) -> f64 {
    let mut freq = [0u32; 256];
    let len = s.len() as f64;

    for byte in s.bytes() {
        freq[byte as usize] += 1;
    }

    let mut entropy = 0.0;
    for count in freq.iter() {
        if *count > 0 {
            let p = (*count as f64) / len;
            entropy -= p * p.log2();
        }
    }

    entropy
}

/// Check if string has mixed character types (common in secrets)
fn has_mixed_chars(s: &str) -> bool {
    let has_upper = s.chars().any(|c| c.is_ascii_uppercase());
    let has_lower = s.chars().any(|c| c.is_ascii_lowercase());
    let has_digit = s.chars().any(|c| c.is_ascii_digit());

    // At least 2 of 3 character types
    (has_upper as u8 + has_lower as u8 + has_digit as u8) >= 2
}

/// Check if a URL contains an embedded password
fn contains_url_password(value: &str) -> bool {
    // Match URLs with user:password@host pattern
    let url_with_pass = Regex::new(r"://[^:]+:[^@]+@").unwrap();

    if url_with_pass.is_match(value) {
        // Extract the password part and check it's not a placeholder
        if let Some(caps) = Regex::new(r"://[^:]+:([^@]+)@").unwrap().captures(value) {
            if let Some(password) = caps.get(1) {
                let pass = password.as_str().to_lowercase();
                // Skip common placeholders
                if pass == "password" || pass == "pass" || pass == "secret" ||
                   pass.contains("xxx") || pass.contains("example") ||
                   pass.contains("changeme") || pass.contains("your") {
                    return false;
                }
                return true;
            }
        }
    }
    false
}

/// Build a map of key -> line number from raw content
fn build_line_lookup(content: &str) -> HashMap<String, usize> {
    let mut lookup = HashMap::new();

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Handle export prefix
        let key_line = trimmed.strip_prefix("export ").unwrap_or(trimmed);

        // Extract key (before =)
        if let Some(eq_pos) = key_line.find('=') {
            let key = key_line[..eq_pos].trim().to_string();
            if !key.is_empty() {
                lookup.insert(key, line_num + 1); // 1-indexed lines
            }
        }
    }

    lookup
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_env(pairs: Vec<(&str, &str)>) -> HashMap<String, String> {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn test_detects_aws_access_key() {
        let env = make_env(vec![("AWS_KEY", "AKIAIOSFODNN7EXAMPLE")]);
        let content = "AWS_KEY=AKIAIOSFODNN7EXAMPLE";
        let warnings = detect_secrets(&env, content, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("AWS"));
    }

    #[test]
    fn test_detects_stripe_key() {
        let env = make_env(vec![("STRIPE_KEY", "sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx")]);
        let content = "STRIPE_KEY=sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let warnings = detect_secrets(&env, content, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("Stripe"));
    }

    #[test]
    fn test_detects_github_token() {
        let env = make_env(vec![("GH_TOKEN", "ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")]);
        let content = "GH_TOKEN=ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let warnings = detect_secrets(&env, content, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("GitHub"));
    }

    #[test]
    fn test_detects_private_key() {
        let env = make_env(vec![("KEY", "-----BEGIN RSA PRIVATE KEY-----")]);
        let content = "KEY=-----BEGIN RSA PRIVATE KEY-----";
        let warnings = detect_secrets(&env, content, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("Private key"));
    }

    #[test]
    fn test_detects_url_with_password() {
        let env = make_env(vec![("DB_URL", "postgres://user:actualPassword123@host/db")]);
        let content = "DB_URL=postgres://user:actualPassword123@host/db";
        let warnings = detect_secrets(&env, content, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("password"));
    }

    #[test]
    fn test_ignores_url_with_placeholder_password() {
        let env = make_env(vec![("DB_URL", "postgres://user:password@host/db")]);
        let content = "DB_URL=postgres://user:password@host/db";
        let warnings = detect_secrets(&env, content, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_ignores_empty_values() {
        let env = make_env(vec![("EMPTY", "")]);
        let content = "EMPTY=";
        let warnings = detect_secrets(&env, content, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_ignores_normal_values() {
        let env = make_env(vec![
            ("NODE_ENV", "production"),
            ("PORT", "3000"),
            ("DEBUG", "true"),
        ]);
        let content = "NODE_ENV=production\nPORT=3000\nDEBUG=true";
        let warnings = detect_secrets(&env, content, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_ignores_placeholders() {
        let env = make_env(vec![
            ("API_KEY", "your_api_key_here"),
            ("SECRET", "changeme"),
            ("TOKEN", "xxx-placeholder-xxx"),
        ]);
        let content = "API_KEY=your_api_key_here\nSECRET=changeme\nTOKEN=xxx-placeholder-xxx";
        let warnings = detect_secrets(&env, content, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_line_numbers() {
        let env = make_env(vec![("STRIPE_KEY", "sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx")]);
        let content = "# Comment\nNODE_ENV=prod\nSTRIPE_KEY=sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let warnings = detect_secrets(&env, content, None);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].line, 3);
    }

    // Whitelist tests
    use crate::schema::{VarSpec, VarType};

    fn make_schema(entries: Vec<(&str, bool)>) -> Schema {
        entries
            .into_iter()
            .map(|(k, secret_safe)| {
                (
                    k.to_string(),
                    VarSpec {
                        var_type: VarType::String,
                        required: false,
                        description: None,
                        values: None,
                        default: None,
                        validate: None,
                        secret: if secret_safe { Some(false) } else { None },
                        ..Default::default()
                    },
                )
            })
            .collect()
    }

    #[test]
    fn test_whitelist_skips_detection() {
        let env = make_env(vec![("STRIPE_KEY", "sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx")]);
        let content = "STRIPE_KEY=sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let schema = make_schema(vec![("STRIPE_KEY", true)]); // secret: false = safe

        // Without schema - detected
        let warnings = detect_secrets(&env, content, None);
        assert_eq!(warnings.len(), 1);

        // With schema whitelist - skipped
        let warnings = detect_secrets(&env, content, Some(&schema));
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_whitelist_only_affects_marked_keys() {
        let env = make_env(vec![
            ("SAFE_KEY", "sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
            ("REAL_SECRET", "sk_live_xxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
        ]);
        let content = "SAFE_KEY=sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx\nREAL_SECRET=sk_live_xxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let schema = make_schema(vec![("SAFE_KEY", true)]); // Only SAFE_KEY is whitelisted

        let warnings = detect_secrets(&env, content, Some(&schema));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].key, "REAL_SECRET");
    }

    #[test]
    fn test_whitelist_secret_none_still_checks() {
        let env = make_env(vec![("STRIPE_KEY", "sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx")]);
        let content = "STRIPE_KEY=sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let schema = make_schema(vec![("STRIPE_KEY", false)]); // secret: None (not whitelisted)

        let warnings = detect_secrets(&env, content, Some(&schema));
        assert_eq!(warnings.len(), 1);
    }
}
