# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.3.x   | Yes                |
| 0.2.x   | Yes                |
| 0.1.x   | No                 |

## Reporting a Vulnerability

We take security seriously. If you discover a security vulnerability in zenv, please report it responsibly.

### How to Report

**Do NOT open a public issue for security vulnerabilities.**

Instead, please report security issues via:

**GitHub Security Advisories**: [Report a vulnerability](https://github.com/zorl-engine/zorath-env/security/advisories/new)

### What to Include

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

### What to Expect

- **Acknowledgment**: Within 48 hours
- **Initial Assessment**: Within 7 days
- **Resolution Timeline**: Depends on severity
  - Critical: 24-48 hours
  - High: 7 days
  - Medium: 30 days
  - Low: Next release

### What Qualifies as a Security Issue

- Arbitrary code execution
- Path traversal vulnerabilities
- Denial of service in parsing
- Information disclosure
- Dependency vulnerabilities

## Security Features

### Secret Masking (v0.3.8+)

zenv masks recognized sensitive values in validation diagnostics, diff output, fix previews, and the masked MCP env resource:

- **Error messages**: Sensitive values shown as `***MASKED***`
- **fix --dry-run**: Previewed fixes mask sensitive values
- **Auto-detection**: Keys containing `password`, `secret`, `token`, `key`, `api_key` are masked

Masking is a defense against accidental exposure, not a guarantee that every secret is recognized. Set `"secret": true` in the schema for known sensitive variables and pass that schema to `diff`. Review output before sharing it. Exports contain actual values; generated documentation and `example --include-defaults` can include schema defaults. Never put real credentials in defaults intended for publication.

`"secret": false` opts a variable out of secret detection; it does not disable all output masking.

### Secret Detection Patterns (--detect-secrets)

When `--detect-secrets` is enabled, zenv scans for 22 regex patterns plus URL-password and high-entropy checks:

| Pattern | Detects | Example Match |
|---------|---------|---------------|
| AWS Access Key ID | AWS access keys | `AKIAIOSFODNN7EXAMPLE` |
| AWS Secret Key | 40-char AWS secrets | Base64-encoded 40-char strings |
| OpenAI API Key | OpenAI keys | `sk-...`, `sk-proj-...` |
| Anthropic API Key | Anthropic keys | `sk-ant-api...`, `sk-ant-admin...` |
| Discord Bot Token | Discord bot credentials | Three-segment token format |
| Discord Webhook | Webhook URL credentials | Discord webhook URLs |
| Slack Webhook | Webhook URL credentials | Slack incoming webhook URLs |
| HuggingFace Token | User access tokens | `hf_...` |
| Datadog API Key | 32-character hex keys under Datadog-related names | `DATADOG_API_KEY`, `DD_API_KEY` |
| Stripe API Key | Stripe keys | `sk_live_...`, `pk_test_...` |
| GitHub Token | GitHub PATs | `ghp_...`, `gho_...`, `ghs_...` |
| GitLab Token | GitLab PATs | `glpat-...` |
| Slack Token | Slack bot/app tokens | `xoxb-...`, `xoxa-...` |
| Private Key | PEM-encoded keys | `-----BEGIN RSA PRIVATE KEY-----` |
| JWT Token | JSON Web Tokens | `eyJhbGciOiJIUzI1NiIs...` |
| Google API Key | Google Cloud keys | `AIza...` |
| Heroku API Key | Heroku UUIDs | UUID format credentials |
| Generic API Key | Common prefixes | `api_key_...`, `apikey...` |
| npm Token | npm auth tokens | `npm_...` |
| SendGrid Key | SendGrid API keys | `SG....` |
| Twilio Credentials | Twilio SID/tokens | `AC...`, `SK...` |
| Mailchimp Key | Mailchimp keys | `...-us1`, `...-us2` |
| High-Entropy | Statistical detection | Strings with entropy > 4.0 |

**Usage:**
```bash
zenv check --detect-secrets
zenv check --detect-secrets true   # explicit
```

Secret findings are warnings: `check` can exit 0 with nonempty `secret_warnings`. A CI policy that rejects findings must inspect the JSON report, as shown in [CI Integration](https://github.com/zorl-engine/zorath-env/wiki/CI-Integration#fail-ci-on-secret-findings). A clean report is not proof that a file is safe to publish.

**Configure in .zenvrc:**
```json
{
  "detect_secrets": true
}
```

## Remote Schema Security (v0.3.5+)

zenv provides security features for remote schema fetching:

### Hash Verification (`--verify-hash`)

Verify schema integrity with SHA-256:

```bash
zenv check --schema https://example.com/schema.json --verify-hash "$SCHEMA_SHA256"
```

- Detects changed content when the expected digest comes from a separately trusted source
- Verifies both cached and fresh content
- Supports full SHA-256 hashes or prefixes of at least 32 hexadecimal characters

Pinned inheritance uses a distinct `extends_hash` for each immediate parent. Pinning the root requires a complete pinned chain; an unpinned parent is rejected.

Remote requests connect directly with no environment proxy, validate the addresses actually used by the connection, reject internal/nonpublic destinations, and do not follow redirects.

### Custom CA Certificates (`--ca-cert`)

For custom trust roots on public HTTPS endpoints (private network destinations remain blocked):

```bash
zenv check --schema https://schemas.example.com/schema.json --ca-cert /path/to/ca.pem
```

- PEM format required
- Certificate validated before use

### Rate Limiting

Remote schema fetches are rate-limited by default:

- 60 seconds between fetches per URL
- Prevents abuse and excessive requests
- Configurable in `.zenvrc`: `"rate_limit_seconds": 120`
- A fresh, valid cached schema is usable during the rate-limit window
- `--no-cache` bypasses both the cache and rate limit
- `zenv cache clear <URL>` removes that URL's cached content and rate-limit metadata

### Security Best Practices

1. Always use `--verify-hash` for production schemas
2. Store trusted hashes securely (CI secrets, config)
3. Review schema changes before updating hashes
4. Use HTTPS only (HTTP is rejected)
5. Protect CA certificates from unauthorized access

### What Does NOT Qualify

- Bugs that don't have security implications
- Feature requests
- General questions

### Safe Harbor

We support safe harbor for security researchers who:

- Make a good faith effort to avoid privacy violations
- Avoid disruption to our services
- Do not exploit vulnerabilities beyond demonstrating them
- Report findings promptly

Thank you for helping keep zenv secure.
