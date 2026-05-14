use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

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
    line_numbers: &HashMap<String, usize>,
    schema: Option<&Schema>,
) -> Vec<SecretWarning> {
    let mut warnings = Vec::new();

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
        for pattern in patterns {
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

        // Check for high-entropy strings (potential secrets).
        // is_high_entropy already enforces the >=16 length floor internally.
        if is_high_entropy(value) {
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

fn get_secret_patterns() -> &'static [SecretPattern] {
    static PATTERNS: OnceLock<Vec<SecretPattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            // AWS Access Key ID
            SecretPattern {
                name: "AWS Access Key ID",
                pattern: Regex::new(r"^AKIA[0-9A-Z]{16}$").unwrap(),
            },
            // AWS Secret Access Key (40 char base64-ish). Tightened to
            // require base64 padding-style ending so we don't false-positive
            // on every 40-char hex/base64 blob (Git SHA-1, hash digests,
            // etc.). Pure-hex 40-char values still hit via entropy fallback.
            SecretPattern {
                name: "AWS Secret Access Key",
                pattern: Regex::new(r"^(?:[A-Za-z0-9+/]{40}|[A-Za-z0-9+/]{38}==|[A-Za-z0-9+/]{39}=)$")
                    .unwrap(),
            },
            // OpenAI API keys (sk-... and sk-proj-...)
            SecretPattern {
                name: "OpenAI API key",
                pattern: Regex::new(r"^sk-(proj-)?[A-Za-z0-9_\-]{20,}$").unwrap(),
            },
            // Anthropic API keys (sk-ant-api... / sk-ant-admin...)
            SecretPattern {
                name: "Anthropic API key",
                pattern: Regex::new(r"^sk-ant-(api|admin)\d+-[A-Za-z0-9_\-]{20,}$").unwrap(),
            },
            // Discord bot tokens (3-segment dot-separated)
            SecretPattern {
                name: "Discord bot token",
                pattern: Regex::new(r"^[MN][A-Za-z\d]{23}\.[\w-]{6}\.[\w-]{27}$").unwrap(),
            },
            // Discord webhook URL (the URL itself IS the secret)
            SecretPattern {
                name: "Discord webhook URL",
                pattern: Regex::new(
                    r"^https?://(?:ptb\.|canary\.)?discord(?:app)?\.com/api/webhooks/\d+/[A-Za-z0-9_-]+$",
                )
                .unwrap(),
            },
            // Slack webhook URL
            SecretPattern {
                name: "Slack webhook URL",
                pattern: Regex::new(r"^https?://hooks\.slack\.com/services/T[A-Z0-9]+/B[A-Z0-9]+/[A-Za-z0-9]+$")
                    .unwrap(),
            },
            // HuggingFace user access tokens
            SecretPattern {
                name: "HuggingFace token",
                pattern: Regex::new(r"^hf_[A-Za-z0-9]{34,}$").unwrap(),
            },
            // Datadog API keys (32 hex chars; pair with key-name context to reduce FP)
            SecretPattern {
                name: "Datadog API key",
                pattern: Regex::new(r"^[a-f0-9]{32}$").unwrap(),
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
                pattern: Regex::new(r"-----BEGIN (RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY-----")
                    .unwrap(),
            },
            // JWT tokens
            SecretPattern {
                name: "JWT token",
                pattern: Regex::new(r"^eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$")
                    .unwrap(),
            },
            // Google API keys
            SecretPattern {
                name: "Google API key",
                pattern: Regex::new(r"^AIza[0-9A-Za-z\-_]{35}$").unwrap(),
            },
            // Heroku API key
            SecretPattern {
                name: "Heroku API key",
                pattern: Regex::new(
                    r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
                )
                .unwrap(),
            },
            // Generic API key patterns (common prefixes)
            SecretPattern {
                name: "API key (common prefix)",
                pattern: Regex::new(r"^(api[_-]?key|apikey|api[_-]?secret)[_-]?[0-9a-zA-Z]{16,}$")
                    .unwrap(),
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
    })
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
    if lower.contains("example")
        || lower.contains("placeholder")
        || lower.contains("changeme")
        || lower.contains("your_")
        || lower.contains("xxx")
        || lower == "development"
        || lower == "production"
        || lower == "staging"
        || lower == "localhost"
        || lower == "true"
        || lower == "false"
    {
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
    static URL_PASS_DETECT: OnceLock<Regex> = OnceLock::new();
    static URL_PASS_CAPTURE: OnceLock<Regex> = OnceLock::new();

    let url_with_pass = URL_PASS_DETECT.get_or_init(|| Regex::new(r"://[^:]+:[^@]+@").unwrap());
    let url_pass_capture =
        URL_PASS_CAPTURE.get_or_init(|| Regex::new(r"://[^:]+:([^@]+)@").unwrap());

    if url_with_pass.is_match(value) {
        if let Some(caps) = url_pass_capture.captures(value) {
            if let Some(password) = caps.get(1) {
                let pass = password.as_str().to_lowercase();
                // Skip common placeholders
                if pass == "password"
                    || pass == "pass"
                    || pass == "secret"
                    || pass.contains("xxx")
                    || pass.contains("example")
                    || pass.contains("changeme")
                    || pass.contains("your")
                {
                    return false;
                }
                return true;
            }
        }
    }
    false
}

/// Returns true if the value looks like it contains a secret regardless
/// of the key name -- catches `DATABASE_URL=postgres://u:p@h` even when
/// the key heuristic doesn't fire, and catches Slack/Discord webhook
/// URLs whose path IS the secret.
pub fn value_looks_secret(value: &str) -> bool {
    if contains_url_password(value) {
        return true;
    }
    // Slack and Discord webhook URLs: path component is the secret.
    static WEBHOOK_DETECT: OnceLock<Regex> = OnceLock::new();
    let re = WEBHOOK_DETECT.get_or_init(|| {
        Regex::new(
            r"https?://(hooks\.slack\.com/services/|(?:ptb\.|canary\.)?discord(?:app)?\.com/api/webhooks/)",
        )
        .unwrap()
    });
    re.is_match(value)
}

/// Check if a key name suggests it contains sensitive data.
///
/// Audit-extended (May 2026 second pass) with names that frequently embed
/// secrets in production: DATABASE_URL / REDIS_URL / MONGO_URL all
/// commonly contain `user:password@host`; webhook URLs ARE the secret;
/// MNEMONIC / SALT / SEED are cryptographic material; DSN often holds
/// a Sentry/database connection key.
pub fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    let sensitive_patterns = [
        "password",
        "passwd",
        "secret",
        "token",
        "api_key",
        "apikey",
        "private_key",
        "privatekey",
        // Audit-added patterns:
        "database_url",
        "database_uri",
        "redis_url",
        "mongo_url",
        "mongodb_uri",
        "connection_string",
        "conn_string",
        "dsn",
        "webhook",
        "mnemonic",
        "salt",
        "seed_phrase",
        "auth",
        "credential",
        "jwt",
        "bearer",
        "access_key",
        "accesskey",
        "secret_key",
        "secretkey",
        "encryption_key",
        "encryptionkey",
        "signing_key",
        "signingkey",
    ];

    for pattern in sensitive_patterns {
        if lower.contains(pattern) {
            return true;
        }
    }

    // Also check for common suffixes
    lower.ends_with("_key")
        || lower.ends_with("_token")
        || lower.ends_with("_secret")
        || lower.ends_with("_url")  // URL values often embed credentials
        || lower.ends_with("_uri")
}

/// Mask sensitive values for safe display (truncates non-sensitive values)
pub fn mask_value(key: &str, value: &str) -> String {
    mask_value_with_spec(key, value, None)
}

/// Mask a value if the key is sensitive, the schema explicitly marks it
/// secret, OR the value itself contains an embedded URL password
/// (e.g. `postgres://user:secret@host` regardless of key name).
pub fn mask_value_with_spec(key: &str, value: &str, spec_secret: Option<bool>) -> String {
    if spec_secret.unwrap_or(false) || is_sensitive_key(key) || value_looks_secret(value) {
        "***MASKED***".to_string()
    } else {
        truncate_value(value)
    }
}

/// Truncate a value for display (max 30 chars). pub(crate) so the watch-mode
/// change feed in commands::check can format old/new values consistently.
pub(crate) fn truncate_value(value: &str) -> String {
    if value.len() <= 30 {
        value.replace('\n', "\\n")
    } else {
        format!("{}...", &value[..27].replace('\n', "\\n"))
    }
}

/// Truncate a value for display with a caller-specified max length, masking
/// if the key looks sensitive OR the value itself looks secret (URL with
/// embedded password, Slack/Discord webhook). Consolidates the parallel
/// 3-arg truncate previously duplicated in commands::diff -- callers there
/// pass max_len=40 or 50 vs the watch-mode 30-char default.
pub(crate) fn truncate_value_for_display(key: &str, value: &str, max_len: usize) -> String {
    if is_sensitive_key(key) || value_looks_secret(value) {
        return "***MASKED***".to_string();
    }
    let display = value.replace('\n', "\\n").replace('\r', "\\r");
    if display.len() <= max_len {
        display
    } else {
        format!("{}...", &display[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_env(pairs: Vec<(&str, &str)>) -> HashMap<String, String> {
        pairs
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn make_lines(content: &str) -> HashMap<String, usize> {
        crate::envfile::parse_env_str_detailed(content).line_numbers
    }

    #[test]
    fn test_detects_aws_access_key() {
        let env = make_env(vec![("AWS_KEY", "AKIAIOSFODNN7EXAMPLE")]);
        let content = "AWS_KEY=AKIAIOSFODNN7EXAMPLE";
        let warnings = detect_secrets(&env, &make_lines(content), None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("AWS"));
    }

    #[test]
    fn test_detects_stripe_key() {
        let env = make_env(vec![("STRIPE_KEY", "sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx")]);
        let content = "STRIPE_KEY=sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let warnings = detect_secrets(&env, &make_lines(content), None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("Stripe"));
    }

    #[test]
    fn test_detects_github_token() {
        let env = make_env(vec![(
            "GH_TOKEN",
            "ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        )]);
        let content = "GH_TOKEN=ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let warnings = detect_secrets(&env, &make_lines(content), None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("GitHub"));
    }

    #[test]
    fn test_detects_private_key() {
        let env = make_env(vec![("KEY", "-----BEGIN RSA PRIVATE KEY-----")]);
        let content = "KEY=-----BEGIN RSA PRIVATE KEY-----";
        let warnings = detect_secrets(&env, &make_lines(content), None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("Private key"));
    }

    #[test]
    fn test_detects_url_with_password() {
        let env = make_env(vec![(
            "DB_URL",
            "postgres://user:actualPassword123@host/db",
        )]);
        let content = "DB_URL=postgres://user:actualPassword123@host/db";
        let warnings = detect_secrets(&env, &make_lines(content), None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].reason.contains("password"));
    }

    #[test]
    fn test_ignores_url_with_placeholder_password() {
        let env = make_env(vec![("DB_URL", "postgres://user:password@host/db")]);
        let content = "DB_URL=postgres://user:password@host/db";
        let warnings = detect_secrets(&env, &make_lines(content), None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_ignores_empty_values() {
        let env = make_env(vec![("EMPTY", "")]);
        let content = "EMPTY=";
        let warnings = detect_secrets(&env, &make_lines(content), None);
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
        let warnings = detect_secrets(&env, &make_lines(content), None);
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
        let warnings = detect_secrets(&env, &make_lines(content), None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_line_numbers() {
        let env = make_env(vec![("STRIPE_KEY", "sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx")]);
        let content = "# Comment\nNODE_ENV=prod\nSTRIPE_KEY=sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let warnings = detect_secrets(&env, &make_lines(content), None);
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
        let warnings = detect_secrets(&env, &make_lines(content), None);
        assert_eq!(warnings.len(), 1);

        // With schema whitelist - skipped
        let warnings = detect_secrets(&env, &make_lines(content), Some(&schema));
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

        let warnings = detect_secrets(&env, &make_lines(content), Some(&schema));
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].key, "REAL_SECRET");
    }

    #[test]
    fn test_whitelist_secret_none_still_checks() {
        let env = make_env(vec![("STRIPE_KEY", "sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx")]);
        let content = "STRIPE_KEY=sk_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let schema = make_schema(vec![("STRIPE_KEY", false)]); // secret: None (not whitelisted)

        let warnings = detect_secrets(&env, &make_lines(content), Some(&schema));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_truncate_value_for_display_short() {
        assert_eq!(truncate_value_for_display("FOO", "short", 10), "short");
    }

    #[test]
    fn test_truncate_value_for_display_long() {
        assert_eq!(
            truncate_value_for_display("FOO", "this is a very long value", 10),
            "this is a ..."
        );
    }

    #[test]
    fn test_truncate_value_for_display_newlines() {
        assert_eq!(
            truncate_value_for_display("FOO", "line1\nline2", 20),
            "line1\\nline2"
        );
    }

    #[test]
    fn test_truncate_value_for_display_carriage_returns() {
        assert_eq!(
            truncate_value_for_display("FOO", "line1\r\nline2", 20),
            "line1\\r\\nline2"
        );
    }

    #[test]
    fn test_truncate_value_for_display_masks_sensitive_key() {
        assert_eq!(
            truncate_value_for_display("API_KEY", "sk_live_abc123", 50),
            "***MASKED***"
        );
        assert_eq!(
            truncate_value_for_display("DB_PASSWORD", "hunter2", 50),
            "***MASKED***"
        );
        assert_eq!(
            truncate_value_for_display("JWT_SECRET", "x", 50),
            "***MASKED***"
        );
    }

    #[test]
    fn test_truncate_value_for_display_masks_url_password() {
        // Strict security improvement over the old diff.rs version: value-aware
        // masking catches embedded URL passwords even when the key name is
        // innocuous (e.g. plain FOO holding a postgres connection string).
        assert_eq!(
            truncate_value_for_display("FOO", "postgres://user:hunter2@host/db", 60),
            "***MASKED***"
        );
    }

    #[test]
    fn test_truncate_value_for_display_masks_slack_webhook() {
        assert_eq!(
            truncate_value_for_display(
                "HOOK",
                "https://hooks.slack.com/services/T000/B000/XXXXXXXXXXXXXXXX",
                80
            ),
            "***MASKED***"
        );
    }

    #[test]
    fn test_truncate_value_for_display_exact_limit() {
        let value = "0123456789";
        assert_eq!(truncate_value_for_display("FOO", value, 10), value);
    }

    #[test]
    fn test_truncate_value_for_display_one_over_limit() {
        let value = "01234567890";
        let result = truncate_value_for_display("FOO", value, 10);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_value_for_display_empty() {
        assert_eq!(truncate_value_for_display("FOO", "", 10), "");
    }
}
