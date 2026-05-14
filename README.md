<p align="center">
  <img src="assets/logo.png" alt="Zorath" width="200">
</p>

# zorath-env

[![Crates.io](https://img.shields.io/crates/v/zorath-env.svg)](https://crates.io/crates/zorath-env)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Docs](https://img.shields.io/badge/docs-zorl.cloud-blue)](https://zorl.cloud/zenv)

**Package:** `zorath-env` | **Binary:** `zenv`

**Built by Zorath -- infrastructure for builders.**

A fast, single-binary CLI that validates `.env` files against typed schemas, detects secrets, scans your code, generates docs, exports to 7 deployment formats, and auto-fixes issues. No runtime dependencies. Works with any stack.

`14 types` -- `7 export formats` -- `9 language scan` -- `15 secret patterns` -- `6 framework presets` -- `686 tests`

## Why zenv

`.env` files drift. Teams copy/paste secrets. CI fails at 3am. Docs go stale. Nobody catches the typo until production.

`zenv` catches config bugs at build time, not runtime. Your schema is the single source of truth -- docs, examples, and validation are all generated from it.

**What makes zenv different:**

- **Validate with real types** -- 14 validators (url, email, port, semver, uuid, ipv4, ipv6, date, hostname, and more), not just "is it a string?"
- **Scan your code** -- find env vars used in source code across 9 languages that aren't in your schema yet
- **Export anywhere** -- shell, Docker, Kubernetes, systemd, GitHub Secrets, JSON, dotenv
- **Detect secrets** -- 15 patterns (AWS, Stripe, GitHub, Slack, JWT, PGP keys) plus entropy analysis
- **Auto-fix** -- add missing vars, remove unknown keys, preview changes safely with `--dry-run`
- **One binary, any stack** -- Rust binary with no runtime deps. Node, Python, Go, Ruby, Java, PHP, Kotlin -- doesn't matter

> See how zenv compares to dotenv-linter, envalid, dotenvx, and others: [docs/comparison.md](docs/comparison.md)

## Privacy

zenv runs locally. No uploads, no telemetry, no phoning home.

## Install

### Via cargo (recommended)
```bash
cargo install zorath-env
```

### From source
```bash
cargo install --path .
```

### Run locally

```bash
cargo run -- check
```

## Library usage

zenv can be embedded in other Rust tools:

```rust
use zorath_env::commands::{check, docs, example, export};
use zorath_env::schema::{load_schema_with_options, LoadOptions};

// Load schema
let opts = LoadOptions::default();
let schema = load_schema_with_options("env.schema.json", &opts)?;

// Validate files directly
let errors = check::validate_files(".env", "env.schema.json", &opts)?;

// Generate documentation
let markdown = docs::generate(&schema, "markdown")?;
let json_docs = docs::generate(&schema, "json")?;

// Generate .env.example content
let example_content = example::generate(&schema, true); // include defaults

// Export to deployment formats
use zorath_env::commands::export::ExportFormat;
let docker_env = export::export_to_string(&env_map, ExportFormat::Docker)?;
let k8s_config = export::export_to_string(&env_map, ExportFormat::K8s)?;
```

Add to your `Cargo.toml`:

```toml
[dependencies]
zorath-env = "0.3"
```

## Quick start

**1. Create a schema from your existing .env:**

```bash
zenv init                       # Infers types from .env.example
zenv init --preset nextjs       # Or start from a framework preset
```

**2. Validate your .env against the schema:**

```bash
zenv check                      # Text output for humans
zenv check --format json        # JSON output for CI/CD
zenv check --detect-secrets     # Also scan for leaked credentials
zenv check --watch              # Re-validate on every file save
```

**3. Find env vars in your code that aren't in your schema:**

```bash
zenv scan --show-unused         # JS, Python, Go, Rust, PHP, Ruby, Java, C#, Kotlin
```

**4. Export to deployment formats:**

```bash
zenv export --env .env --format k8s            # Kubernetes ConfigMap
zenv export --env .env --format docker         # Dockerfile ENV directives
zenv export --env .env --format github-secrets # GitHub CLI gh secret set
```

**5. Generate docs and examples:**

```bash
zenv docs > ENVIRONMENT.md      # Markdown documentation from schema
zenv example -o .env.example    # Generate .env.example from schema
```

## Commands

### `zenv check`

Validates `.env` against `env.schema.json`.

**Exit codes (CI-friendly):**
* `0` - Valid
* `1` - Validation failed (invalid values, missing required)
* `2` - Input/file error (file not found, failed to read)
* `3` - Schema error (invalid JSON, parse failure)

When fixable issues are found, check suggests running `zenv fix`.

```bash
zenv check                       # Basic validation
zenv check --detect-secrets      # Also scan for potential secrets
zenv check --format json         # JSON output for CI/CD
```

**JSON Output** (`--format json`):

Machine-readable output for CI/CD pipelines:
```json
{
  "valid": true,
  "errors": [],
  "warnings": [],
  "secret_warnings": [],
  "stats": { "total_variables": 10, "schema_variables": 10 }
}
```

**Secret Detection** (`--detect-secrets`):

Scans for potential secrets that shouldn't be committed:
- AWS Access Keys and Secret Keys
- Stripe, GitHub, GitLab, Slack tokens
- Google, Heroku, SendGrid, Twilio, Mailchimp API keys
- npm tokens
- Private key headers (RSA, SSH, PGP)
- JWT tokens
- URLs with embedded passwords
- High-entropy strings

**Watch Mode** (`--watch`):

Watches for file changes and re-validates automatically:

```bash
zenv check --watch                  # Watch .env and schema
zenv check --watch --detect-secrets # Watch with secret detection
```

Features:
- Delta detection: shows exactly which variable changed
- Targeted validation: only validates changed keys
- Content-hash skip: ignores saves without changes
- Local timestamps
- Terminal bell on errors

### `zenv docs`

Generates documentation for all env vars in the schema.

```bash
zenv docs                      # Markdown (default)
zenv docs --format json        # JSON output
zenv docs --format json > schema.json
```

### `zenv init`

Creates `env.schema.json` from `.env.example` (best-effort inference, you refine types after).

### `zenv version`

Shows installed version and optionally checks for updates.

```bash
zenv version                  # Show installed version
zenv version --check-update   # Check crates.io for newer version
```

### `zenv completions`

Generates shell completions for bash, zsh, fish, and PowerShell.

```bash
zenv completions bash > /etc/bash_completion.d/zenv
zenv completions zsh > ~/.zfunc/_zenv
zenv completions fish > ~/.config/fish/completions/zenv.fish
zenv completions powershell > zenv.ps1

# Or evaluate directly
eval "$(zenv completions bash)"
```

### `zenv example`

Generates `.env.example` from schema (reverse of `init`).

```bash
zenv example                     # Output to stdout
zenv example --include-defaults  # Include default values
zenv example --output .env.example  # Write to file
```

### `zenv diff`

Compares two `.env` files and shows differences.

```bash
zenv diff .env.development .env.production
zenv diff .env.dev .env.prod --schema env.schema.json
zenv diff .env.dev .env.prod --format json  # Machine-readable output
```

Shows:
- Variables only in first file
- Variables only in second file
- Variables with different values
- Possible typos ("Did you mean?" suggestions)
- Optional schema compliance check for both files

### `zenv fix`

Auto-fix common `.env` issues with backup.

```bash
zenv fix                          # Fix issues, create backup
zenv fix --dry-run                # Preview fixes without applying
zenv fix --remove-unknown         # Also remove keys not in schema
```

**What it fixes:**
- Missing optional variables (adds with schema defaults)
- Unknown keys (with `--remove-unknown`)

**Security:** `--dry-run` masks sensitive values (passwords, keys, tokens) as `***MASKED***`.

**What it reports but doesn't fix:**
- Invalid types (needs human input)
- Missing required variables (needs human input)

### `zenv scan`

Scan source code for environment variable usage.

```bash
zenv scan                         # Scan current directory
zenv scan --path ./src            # Scan specific directory
zenv scan --show-unused           # Show vars in schema but not in code
zenv scan --show-paths            # Show file:line for all found vars
zenv scan --format json           # JSON output for CI
```

**Supported languages:** JavaScript/TypeScript, Python, Go, Rust, PHP, Ruby, Java, C#, Kotlin

### `zenv cache`

Manage remote schema cache.

```bash
zenv cache list                   # Show cached schemas
zenv cache stats                  # Show cache statistics
zenv cache clear                  # Clear all cached schemas
zenv cache clear https://...      # Clear specific cached schema
zenv cache path                   # Show cache directory location
```

### `zenv export`

Export `.env` to various formats for deployment.

```bash
zenv export --env .env --format shell          # Shell script (export FOO="bar")
zenv export --env .env --format docker         # Dockerfile (ENV FOO=bar)
zenv export --env .env --format k8s            # Kubernetes ConfigMap YAML
zenv export --env .env --format json           # JSON object
zenv export --env .env --format systemd        # systemd Environment directives
zenv export --env .env --format dotenv         # Standard .env format
zenv export --env .env --format github-secrets # GitHub CLI (gh secret set)

zenv export --env .env --schema s.json         # Only export vars in schema
zenv export --env .env -f shell -o setup.sh    # Write to file
```

### `zenv doctor`

Run health check and diagnostics.

```bash
zenv doctor
```

Checks:
- Schema file exists and is valid
- .env file exists and parses correctly
- Config file (.zenvrc) is valid JSON
- Remote schema cache is accessible
- Validation passes (if schema and env exist)

Output shows `[OK]`, `[WARN]`, or `[ERROR]` with actionable suggestions.

### `zenv template`

Generate CI/CD configuration templates for popular platforms.

```bash
zenv template github              # Output GitHub Actions workflow
zenv template gitlab -o .gitlab-ci.yml  # Write GitLab CI config
zenv template circleci            # Output CircleCI config
zenv template --list              # List available templates
```

**Available templates:**
- `github` (aliases: `gh`, `github-actions`) - GitHub Actions workflow
- `gitlab` (aliases: `gl`, `gitlab-ci`) - GitLab CI configuration
- `circleci` (aliases: `circle`) - CircleCI configuration

## Files

By default, `zenv` looks for:

* `.env` (optional)
* `.env.example` (optional)
* `env.schema.json` (preferred)

### Env file fallback

If `.env` doesn't exist, `zenv check` will automatically try:

1. `.env.local`
2. `.env.development`
3. `.env.development.local`

This is useful for Next.js and other frameworks that use `.env.local` for secrets.

You can override paths:

```bash
zenv check --env .env --schema env.schema.json
zenv docs  --schema env.schema.json
zenv init  --example .env.example --schema env.schema.json
```

## Schema format (v0.3)

Schemas can be written in **JSON** or **YAML** (auto-detected by file extension).

### JSON schema

`env.schema.json` is a JSON object where each key is an env var name.

```json
{
  "DATABASE_URL": {
    "type": "url",
    "required": true,
    "description": "Primary database connection string"
  },
  "NODE_ENV": {
    "type": "enum",
    "values": ["development", "staging", "production"],
    "default": "development",
    "required": true,
    "description": "Runtime environment"
  },
  "PORT": {
    "type": "port",
    "default": 3000,
    "required": false,
    "description": "HTTP port"
  }
}
```

### YAML schema

Use `.yaml` or `.yml` extension for YAML schemas:

```yaml
# env.schema.yaml - more readable, supports comments
DATABASE_URL:
  type: url
  required: true
  description: Primary database connection string

NODE_ENV:
  type: enum
  values:
    - development
    - staging
    - production
  default: development
  description: Runtime environment

SERVER_PORT:
  type: port
  default: 3000
  description: HTTP port
```

### Supported types

| Type | Description | Example |
|------|-------------|---------|
| `string` | Any string value | `"hello"` |
| `int` | Integer number | `42` |
| `float` | Floating point number | `3.14` |
| `bool` | Boolean (true/false/1/0/yes/no) | `true` |
| `url` | Valid URL | `https://example.com` |
| `enum` | One of specified values | `"development"` |
| `uuid` | UUID format | `550e8400-e29b-41d4-a716-446655440000` |
| `email` | Email address | `user@example.com` |
| `ipv4` | IPv4 address | `192.168.1.1` |
| `ipv6` | IPv6 address | `2001:0db8:85a3::8a2e:0370:7334` |
| `semver` | Semantic version | `1.2.3-beta.1` |
| `port` | Port number (1-65535) | `8080` |
| `date` | ISO 8601 date | `2024-06-15` |
| `hostname` | RFC 1123 hostname | `api.example.com` |

### Validation rules

Add constraints with the `validate` field:

```json
{
  "PORT": { "type": "int", "validate": { "min": 1024, "max": 65535 } },
  "RATE": { "type": "float", "validate": { "min_value": 0.0, "max_value": 1.0 } },
  "API_KEY": { "type": "string", "validate": { "min_length": 32, "pattern": "^sk_" } }
}
```

### Severity levels

Mark non-critical validations as warnings (don't cause exit code 1):

```json
{
  "DEBUG": {
    "type": "bool",
    "severity": "warning",
    "description": "Enable debug mode (optional, won't fail CI)"
  },
  "DATABASE_URL": {
    "type": "url",
    "required": true,
    "severity": "error"
  }
}
```

Default severity is `"error"`. Warnings are reported but don't fail validation.

### Schema inheritance

Schemas can extend other schemas:

```json
{
  "extends": "base.schema.json",
  "EXTRA_VAR": { "type": "string" }
}
```

Inheritance supports up to 10 levels of depth. Circular references are detected and will cause an error.

### Remote schemas

Fetch schemas from HTTPS URLs for shared team configurations:

```bash
# Validate against remote schema
zenv check --schema https://example.com/schemas/env.schema.json

# Generate docs from remote schema
zenv docs --schema https://raw.githubusercontent.com/org/repo/main/env.schema.json

# Force fresh fetch (skip cache)
zenv check --schema https://example.com/schema.json --no-cache
```

**Features:**
- HTTPS only (HTTP rejected for security)
- Automatic caching with 1-hour TTL
- `--no-cache` flag to bypass cache
- Remote schemas can extend other schemas (URLs resolved relative to parent)

#### Remote schema security

**Hash verification** - Verify schema integrity with SHA-256:

```bash
# Verify schema hasn't been tampered with
zenv check --schema https://example.com/schema.json --verify-hash abc123def456...

# Hash prefix matching supported (first 16+ chars)
zenv check --schema https://example.com/schema.json --verify-hash abc123def456
```

**Custom CA certificates** - For enterprise internal servers:

```bash
zenv check --schema https://internal.corp/schema.json --ca-cert /path/to/ca.pem
```

**Rate limiting** - Prevents excessive fetching:
- Default: 60 seconds between fetches per URL
- Bypassed by `--no-cache`
- Configure in `.zenvrc`: `"rate_limit_seconds": 120`

## Configuration File (.zenvrc)

Create a `.zenvrc` file in your project root to set default options. CLI arguments always override `.zenvrc` settings.

```json
{
  "schema": "env.schema.json",
  "env": ".env.local",
  "detect_secrets": true
}
```

### All Configuration Keys

| Key | Type | Description |
|-----|------|-------------|
| `schema` | string | Path to schema file (default: `env.schema.json`) |
| `env` | string | Path to .env file (default: `.env`) |
| `allow_missing_env` | boolean | Allow validation when .env file is missing |
| `detect_secrets` | boolean | Enable secret detection during validation |
| `no_cache` | boolean | Skip cache when fetching remote schemas |
| `no_color` | boolean | Disable colored terminal output |
| `verify_hash` | string | SHA-256 hash (full or >=32-char prefix) to verify remote schema integrity |
| `ca_cert` | string | Path to custom CA certificate (PEM format) |
| `rate_limit_seconds` | number | Seconds between remote schema fetches (default: 60) |
| `format` | string | Default output format for `check`/`docs`/`diff`/`scan`/`export` (per-command default if omitted) |

### Example: Full Configuration

```json
{
  "schema": "config/env.schema.json",
  "env": ".env.local",
  "allow_missing_env": false,
  "detect_secrets": true,
  "no_cache": false,
  "no_color": false,
  "verify_hash": "a1b2c3d4e5f6...",
  "ca_cert": "/etc/ssl/certs/internal-ca.pem",
  "rate_limit_seconds": 120,
  "format": "json"
}
```

**Note:** Unknown keys in `.zenvrc` will trigger a warning but won't cause failures.

## .env features

### Comments and Blank Lines

Full-line comments, inline comments, and blank lines are supported:

```env
# This is a full-line comment
DATABASE_URL=postgres://localhost/db  # inline comment

# Blank lines are ignored
PORT=3000
```

### Export prefix

Shell-style export prefix is supported for compatibility:

```env
export DATABASE_URL=postgres://localhost/db
export NODE_ENV=development
```

### Variable interpolation

Reference other variables with `${VAR}` or `$VAR`:

```env
BASE_URL=https://api.example.com
API_ENDPOINT=${BASE_URL}/v2
```

### Multiline values

Use quoted strings for multiline:

```env
SSH_KEY="-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEA...
-----END RSA PRIVATE KEY-----"
```

### Escape sequences

Double-quoted strings support `\n`, `\t`, `\r`, `\\`, `\"`

## Example output

### Success

```bash
$ zenv check
zenv: OK
```

### Validation errors

```bash
$ zenv check
zenv check failed:

- DATABASE_URL: expected url, got 'not-a-url'
- NODE_ENV: expected one of ["development", "staging", "production"], got 'dev'
- API_KEY: missing (required)
```

When unknown variables are found in your `.env` that are not in the schema, zenv will show a helpful tip suggesting you update your schema.

## Pre-commit hook

```bash
# .git/hooks/pre-commit (make executable)
#!/usr/bin/env bash
set -e

if [ -f "env.schema.json" ]; then
  if command -v zenv >/dev/null 2>&1; then
    zenv check || exit 1
  else
    cargo run --quiet -- check || exit 1
  fi
fi
```

## AI agent integration

For developers using Claude Code or any other AI coding agent: there is
a ready-made skill in [`integrations/claude-code/`](integrations/claude-code/)
that teaches the agent how to drive zenv against any `.env` file in any
project. The skill covers every subcommand, the right flags for
programmatic use, the structured exit codes, and the anti-patterns the
agent must not commit (e.g. never run `zenv fix` without `--dry-run` first).

Install (one-liner):

```bash
# Linux / macOS
mkdir -p ~/.claude/skills/zenv && \
  curl -fsSL https://raw.githubusercontent.com/zorl-engine/zorath-env/main/integrations/claude-code/SKILL.md \
  -o ~/.claude/skills/zenv/SKILL.md
```

The skill is plain markdown, runs entirely on your machine, makes no
network calls, and has no hosted dependency. See
[`integrations/claude-code/README.md`](integrations/claude-code/README.md)
for the Windows install, prompts that demonstrate the integration, and
the privacy notes.

A future release will add a `zenv mcp` stdio subcommand for clients that
prefer the MCP protocol over the skill format -- still local, still no
hosting.

## GitHub Action

Validate `.env` files in your CI/CD pipeline:

```yaml
- name: Validate .env
  uses: zorl-engine/zorath-env/.github/actions/zenv-action@main
  with:
    schema: env.schema.json
    env-file: .env.example
```

**Inputs:**
- `schema` - Path to schema file (default: `env.schema.json`)
- `env-file` - Path to .env file (default: `.env`)
- `allow-missing-env` - Allow missing .env (default: `false`)
- `version` - zenv version to use (default: `latest`)

**Outputs:**
- `valid` - `true` if validation passed
- `errors` - JSON array of error messages

## Connect

- Official site: [zorl.cloud](https://zorl.cloud)
- Documentation: [zorl.cloud/zenv/docs](https://zorl.cloud/zenv/docs)
- GitHub: [github.com/zorl-engine/zorath-env](https://github.com/zorl-engine/zorath-env)
- crates.io: [crates.io/crates/zorath-env](https://crates.io/crates/zorath-env)
- All links: [edgeurl.io/p/zorl-engine](https://edgeurl.io/p/zorl-engine)

## License

MIT

