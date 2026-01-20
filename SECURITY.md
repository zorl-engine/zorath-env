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

## Remote Schema Security (v0.3.5+)

zenv provides security features for remote schema fetching:

### Hash Verification (`--verify-hash`)

Verify schema integrity with SHA-256:

```bash
zenv check --schema https://example.com/schema.json --verify-hash abc123def456...
```

- Prevents man-in-the-middle attacks
- Verifies both cached and fresh content
- Supports full hash or prefix matching (16+ chars)

### Custom CA Certificates (`--ca-cert`)

For enterprise environments with internal HTTPS servers:

```bash
zenv check --schema https://internal.corp/schema.json --ca-cert /path/to/ca.pem
```

- PEM format required
- Certificate validated before use

### Rate Limiting

Remote schema fetches are rate-limited by default:

- 60 seconds between fetches per URL
- Prevents abuse and excessive requests
- Configurable in `.zenvrc`: `"rate_limit_seconds": 120`
- Disabled with `--no-cache` flag

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
