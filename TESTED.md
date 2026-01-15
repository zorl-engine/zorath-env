# zenv - Tested and Verified

**Version:** 0.3.0
**Status:** Verified Working
**Test Date:** January 15, 2025

---

## Test Summary

zenv was tested on a real-world production codebase with 50+ environment variables across multiple `.env` files. All core features performed as expected.

### Unit Test Coverage

**v0.3.0** includes 168 unit tests covering all core functionality:

| Module | Tests | Coverage |
|--------|-------|----------|
| `envfile.rs` | 39 | Parser, multiline, escapes, variable interpolation |
| `schema.rs` | 20 | Type parsing, serialization, inheritance, error handling |
| `commands/check.rs` | 48 | Type validations, validation rules, required fields |
| `commands/init.rs` | 22 | Type inference: bool, int, float, url, string |
| `commands/docs.rs` | 14 | Markdown and JSON output formats, sorting |
| `commands/version.rs` | 1 | Version output |
| `commands/completions.rs` | 4 | Shell completions for bash, zsh, fish, powershell |
| `commands/example.rs` | 19 | .env.example generation from schema |

All tests pass with zero warnings.

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
| `zenv docs` | Working |
| `zenv docs --format json` | Working |
| `zenv version` | Working |
| `zenv completions` | Working |
| `zenv example` | Working |
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
| GitHub Action | Working |

---

## Conclusion

zenv v0.3.0 adds shell completions for all major shells, `zenv example` command for generating .env.example files from schemas, and a GitHub Action for CI/CD integration. Now with 168 unit tests for comprehensive coverage. Ready for production use.

---

## Feedback

Found a bug? Have a feature request?

- GitHub Issues: [github.com/zorl-engine/zorath-env/issues](https://github.com/zorl-engine/zorath-env/issues)

---

**Built by Zorath LLC**
