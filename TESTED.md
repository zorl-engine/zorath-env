# zenv - Tested and Verified

**Version:** 0.3.2
**Status:** Verified Working
**Test Date:** January 16, 2026

---

## Test Summary

zenv was tested on a production Next.js codebase (56 environment variables) and passed all tests. All 7 commands perform as expected.

---

## Real-World Testing

Tested on production schema with 56 variables (Supabase, Stripe, Redis, Vercel integrations):

| Command | Result | Details |
|---------|--------|---------|
| `zenv check` | PASS | Correctly identified 26 missing required vars, 4 unknown keys |
| `zenv check --detect-secrets` | PASS | Detected potential secrets in test files |
| `zenv docs` | PASS | Generated markdown documentation for all 56 variables |
| `zenv docs --format json` | PASS | Valid JSON output with all schema fields |
| `zenv example` | PASS | Generated .env.example with type/required comments |
| `zenv example --include-defaults` | PASS | Included default values |
| `zenv diff` | PASS | Correctly compared two .env files |
| `zenv diff --schema` | PASS | Schema compliance check for both files |
| `zenv completions bash` | PASS | Valid bash completion script |
| `zenv completions powershell` | PASS | Valid PowerShell completion script |
| `zenv version` | PASS | Shows `zenv v0.3.2` |
| `zenv version --check-update` | PASS | Reports "latest version" (matches crates.io) |
| `zenv --help` | PASS | Shows all 7 commands |

**Schema complexity:** 56 variables including URLs, strings, bools, and enums with defaults.

### Unit Test Coverage

**v0.3.2** includes 185 unit tests covering all core functionality:

| Module | Tests | Coverage |
|--------|-------|----------|
| `envfile.rs` | 39 | Parser, multiline, escapes, variable interpolation |
| `schema.rs` | 20 | Type parsing, serialization, inheritance, error handling |
| `secrets.rs` | 10 | Secret detection patterns, high-entropy strings, URL passwords |
| `commands/check.rs` | 48 | Type validations, validation rules, required fields |
| `commands/diff.rs` | 5 | File comparison, truncation, schema compliance |
| `commands/init.rs` | 22 | Type inference: bool, int, float, url, string |
| `commands/docs.rs` | 14 | Markdown and JSON output formats, sorting |
| `commands/version.rs` | 1 | Version output |
| `commands/completions.rs` | 4 | Shell completions for bash, zsh, fish, powershell |
| `commands/example.rs` | 19 | .env.example generation from schema |

All tests pass with zero warnings.

---

## New in v0.3.2

### Secret Detection

Detect potential secrets in .env files with `--detect-secrets`:

```bash
zenv check --detect-secrets
```

Detects:
- AWS Access Keys and Secret Keys (AKIA...)
- Stripe API keys (sk_live_, pk_test_)
- GitHub/GitLab/Slack tokens
- Private key headers (RSA, SSH, PGP)
- JWT tokens
- URLs with embedded passwords
- High-entropy strings (possible secrets)

Example output:
```
Warning: Potential secrets detected:

- AWS_SECRET_KEY (line 12): AWS Secret Access Key
- DATABASE_URL (line 15): URL contains embedded password
- API_TOKEN (line 22): High-entropy string (possible secret)

These values may be real secrets. Consider using placeholders in committed files.
```

### Diff Command

Compare two .env files:

```bash
zenv diff .env.development .env.production
zenv diff .env.dev .env.prod --schema env.schema.json
```

Shows:
- Variables only in first file
- Variables only in second file
- Variables with different values
- Optional schema compliance check

---

## New in v0.3.0

### Shell Completions

Generate shell completions for bash, zsh, fish, and PowerShell:

```bash
# Bash
zenv completions bash > /etc/bash_completion.d/zenv

# Zsh
zenv completions zsh > ~/.zfunc/_zenv

# Fish
zenv completions fish > ~/.config/fish/completions/zenv.fish

# PowerShell
zenv completions powershell > zenv.ps1

# Or evaluate directly
eval "$(zenv completions bash)"
```

### Example Command

Generate `.env.example` from schema (reverse of `init`):

```bash
# Output to stdout
zenv example

# Include default values
zenv example --include-defaults

# Write to file
zenv example --output .env.example
```

### GitHub Action

CI/CD integration via GitHub Action:

```yaml
- name: Validate .env
  uses: zorl-engine/zorath-env/.github/actions/zenv-action@main
  with:
    schema: env.schema.json
    env-file: .env.example
```

**Inputs:** `schema`, `env-file`, `allow-missing-env`, `version`
**Outputs:** `valid`, `errors`
**Platforms:** Linux, macOS (Intel/ARM), Windows

---

## New in v0.2.2

### Version Command

Check installed version and optionally query crates.io for updates:

```bash
zenv version                  # Show installed version
zenv version --check-update   # Check for newer version
```

### Env File Fallback

When `.env` doesn't exist, zenv automatically checks:
1. `.env.local`
2. `.env.development`
3. `.env.development.local`

This improves compatibility with Next.js and similar frameworks.

### Improved Error Messages

Better error output when env files are missing, showing which paths were checked.

---

## New in v0.2.1

### JSON Output Format

The `docs` command now supports JSON output for tooling integration:

```bash
# Default Markdown output
zenv docs

# JSON output
zenv docs --format json

# Save to file
zenv docs --format json > schema.json
```

Supported format values: `markdown`, `md`, `json`

---

## New in v0.2.0

### Variable Interpolation

Support for `${VAR}` and `$VAR` syntax:

```env
BASE_URL=https://api.example.com
API_ENDPOINT=${BASE_URL}/v2
GREETING=Hello $USER!
```

Circular references are detected and reported as errors.

### Multiline Values

Quoted strings can span multiple lines:

```env
SSH_KEY="-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEA...
-----END RSA PRIVATE KEY-----"
```

### Escape Sequences

Double-quoted strings support escape sequences:

| Escape | Character |
|--------|-----------|
| `\n` | newline |
| `\t` | tab |
| `\r` | carriage return |
| `\\` | backslash |
| `\"` | double quote |

Single-quoted strings are literal (no escape processing).

### Custom Validation Rules

Add constraints to your schema:

```json
{
  "PORT": {
    "type": "int",
    "validate": {
      "min": 1024,
      "max": 65535
    }
  },
  "EMAIL": {
    "type": "string",
    "validate": {
      "pattern": "^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$"
    }
  },
  "API_KEY": {
    "type": "string",
    "validate": {
      "min_length": 32,
      "max_length": 64
    }
  }
}
```

Available rules:
- `min`, `max` - for int types
- `min_value`, `max_value` - for float types
- `min_length`, `max_length` - for string types
- `pattern` - regex pattern for string types

### Schema Inheritance

Schemas can extend base schemas:

```json
// base.schema.json
{
  "DATABASE_URL": { "type": "url", "required": true }
}

// production.schema.json
{
  "extends": "base.schema.json",
  "REDIS_URL": { "type": "url", "required": true }
}
```

Child schemas inherit all variables from parent and can override them.

---

## Test Results

### 1. Schema Generation (`zenv init`)

```bash
$ zenv init --example .env.example --schema env.schema.json
zenv: wrote schema to env.schema.json
```

**Result:** Schema successfully generated from example file. Types were correctly inferred.

---

### 2. Validation (`zenv check`)

```bash
$ zenv check --env .env --schema env.schema.json
zenv check failed:

- PORT: value 80 is less than minimum 1024
- EMAIL: value 'not-email' does not match pattern
- SECRET_KEY: length 10 is less than minimum 32
```

**Result:** Validation correctly detected type errors and validation rule violations.

---

### 3. Interpolation

```bash
$ cat .env
BASE=https://api.example.com
FULL_URL=${BASE}/endpoint

$ zenv check
zenv: OK
```

**Result:** Variable references correctly interpolated before validation.

---

### 4. Inheritance

```bash
$ cat base.schema.json
{"BASE_VAR": {"type": "string"}}

$ cat child.schema.json
{"extends": "base.schema.json", "CHILD_VAR": {"type": "int"}}

$ zenv docs --schema child.schema.json
# Environment Variables

## `BASE_VAR`
- Type: `string`
...

## `CHILD_VAR`
- Type: `int`
```

**Result:** Child schema correctly inherits variables from parent.

---

## Features Verified

| Feature | Status |
|---------|--------|
| `zenv init` | Working |
| `zenv check` | Working |
| `zenv check --detect-secrets` | Working |
| `zenv docs` | Working |
| `zenv docs --format json` | Working |
| `zenv version` | Working |
| `zenv completions` | Working |
| `zenv example` | Working |
| `zenv diff` | Working |
| `zenv diff --schema` | Working |
| Type inference (string, int, bool, url) | Working |
| Required field validation | Working |
| Unknown key detection | Working |
| Variable interpolation (${VAR}, $VAR) | Working |
| Multiline quoted values | Working |
| Escape sequences in double quotes | Working |
| Validation rules (min/max/pattern/length) | Working |
| Schema inheritance (extends) | Working |
| Circular reference detection | Working |
| Custom file paths | Working |
| Exit codes (0 = pass, 1 = fail) | Working |
| `--check-update` flag | Working |
| Env file fallback (.env.local, etc.) | Working |
| Shell completions (bash/zsh/fish/powershell) | Working |
| Secret detection (AWS, Stripe, GitHub, etc.) | Working |
| GitHub Action | Working |

---

## Conclusion

zenv v0.3.2 adds secret detection (`--detect-secrets`) and `zenv diff` command for comparing .env files. v0.3.1 added Windows support in GitHub Action. v0.3.0 introduced shell completions, `zenv example` command, and GitHub Action. 185 unit tests for comprehensive coverage. Ready for production use.

---

## Feedback

Found a bug? Have a feature request?

- GitHub Issues: [github.com/zorl-engine/zorath-env/issues](https://github.com/zorl-engine/zorath-env/issues)

---

**Built by Zorath LLC**
