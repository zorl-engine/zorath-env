# TODO - zenv Roadmap

**Current Version:** 0.2.2
**Last Updated:** January 13, 2025

---

## High Impact (Recommended)

| Feature | Why | Effort | Status |
|---------|-----|--------|--------|
| GitHub Action | Official action in marketplace = discovery + easy CI adoption | Medium | Planned |
| Shell completions | bash/zsh/fish/powershell - table stakes for CLI tools | Low | Planned |
| VSCode extension | Schema validation + autocomplete in editor | High | Planned |
| `zenv example` | Generate .env.example from schema (reverse of init) | Low | Planned |

---

### GitHub Action

Official GitHub Action for the marketplace.

**Sample usage (what users would write):**

```yaml
# .github/workflows/validate-env.yml
name: Validate Environment

on: [push, pull_request]

jobs:
  validate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Validate .env
        uses: zorl-engine/zenv-action@v1
        with:
          schema: env.schema.json
          env-file: .env.example  # validate example, not secrets
```

**Features to include:**
- Input: `schema` (path to schema file)
- Input: `env-file` (path to .env file)
- Input: `allow-missing-env` (boolean)
- Output: `valid` (boolean)
- Output: `errors` (JSON array of error messages)

---

### Shell Completions

Auto-complete for bash, zsh, fish, and PowerShell.

**Implementation:** Use clap's built-in `clap_complete` crate.

**Sample usage:**

```bash
# Generate completions
zenv completions bash > /etc/bash_completion.d/zenv
zenv completions zsh > ~/.zfunc/_zenv
zenv completions fish > ~/.config/fish/completions/zenv.fish
zenv completions powershell > zenv.ps1

# Or install directly
eval "$(zenv completions bash)"
```

**What it enables:**

```bash
$ zenv ch<TAB>
$ zenv check

$ zenv check --<TAB>
--env      --schema      --allow-missing-env

$ zenv docs --format <TAB>
json      markdown      md
```

---

### VSCode Extension

Real-time schema validation and autocomplete in the editor.

**Features:**
- Validate `.env` files against `env.schema.json` on save
- Autocomplete variable names from schema
- Hover for descriptions and types
- Red squiggles for errors
- Quick fixes for common issues

**Sample extension.json:**

```json
{
  "name": "zenv",
  "displayName": "zenv - Environment Validator",
  "description": "Validate .env files against JSON schemas",
  "publisher": "zorl-engine",
  "categories": ["Linters", "Other"],
  "activationEvents": [
    "onLanguage:dotenv",
    "workspaceContains:env.schema.json"
  ],
  "contributes": {
    "configuration": {
      "title": "zenv",
      "properties": {
        "zenv.schemaPath": {
          "type": "string",
          "default": "env.schema.json",
          "description": "Path to the schema file"
        }
      }
    }
  }
}
```

---

### `zenv example` Command

Generate `.env.example` from schema (reverse of `init`).

**Sample usage:**

```bash
# Generate .env.example from schema
zenv example

# Custom paths
zenv example --schema env.schema.json --output .env.example

# Include defaults
zenv example --include-defaults
```

**Sample output (.env.example):**

```env
# PostgreSQL connection string
# Type: url (required)
DATABASE_URL=

# Runtime environment
# Type: enum (required)
# Values: development, staging, production
# Default: development
NODE_ENV=development

# HTTP port
# Type: int
# Default: 3000
# Validation: min=1024, max=65535
PORT=3000

# External API key (must start with sk_)
# Type: string (required)
# Validation: min_length=32, pattern=^sk_
API_KEY=
```

---

## Nice to Have

| Feature | Why | Effort | Status |
|---------|-----|--------|--------|
| YAML schema format | Less verbose than JSON, some teams prefer it | Medium | Planned |
| Watch mode (`--watch`) | Auto-validate on file changes during dev | Low | Planned |
| Diff command | Compare two .env files against schema | Low | Planned |
| Config file (`.zenvrc`) | Default paths/settings per project | Low | Planned |

---

### YAML Schema Format

Support YAML as an alternative to JSON for schemas.

**Sample (env.schema.yaml):**

```yaml
DATABASE_URL:
  type: url
  required: true
  description: PostgreSQL connection string

NODE_ENV:
  type: enum
  values:
    - development
    - staging
    - production
  default: development
  required: true

PORT:
  type: int
  default: 3000
  validate:
    min: 1024
    max: 65535

API_KEY:
  type: string
  required: true
  validate:
    min_length: 32
    pattern: "^sk_"
```

**Usage:**

```bash
zenv check --schema env.schema.yaml
zenv docs --schema env.schema.yaml
```

---

### Watch Mode

Auto-validate on file changes during development.

**Sample usage:**

```bash
# Watch .env and schema for changes
zenv check --watch

# Watch specific files
zenv check --watch --env .env.local --schema env.schema.json

# With clear screen
zenv check --watch --clear
```

**Sample output:**

```
[watching] .env, env.schema.json
[14:32:01] zenv: OK
[14:32:15] .env changed
[14:32:15] zenv check failed:
           - PORT: expected int, got 'abc'
[14:32:20] .env changed
[14:32:20] zenv: OK
```

---

### Diff Command

Compare two .env files against schema.

**Sample usage:**

```bash
# Compare .env.development vs .env.production
zenv diff .env.development .env.production

# Compare against schema
zenv diff .env.development .env.production --schema env.schema.json
```

**Sample output:**

```
Comparing .env.development vs .env.production

Variables only in .env.development:
  - DEBUG=true

Variables only in .env.production:
  - SENTRY_DSN=https://...
  - REDIS_URL=redis://...

Variables with different values:
  - NODE_ENV: development -> production
  - LOG_LEVEL: debug -> error
  - DATABASE_URL: postgres://localhost/dev -> postgres://prod-host/prod

Schema compliance:
  .env.development: 2 warnings (DEBUG, LOG_LEVEL not in schema)
  .env.production: OK
```

---

### Config File (`.zenvrc`)

Project-level configuration for default paths and settings.

**Sample (.zenvrc):**

```json
{
  "schema": "config/env.schema.json",
  "env": ".env",
  "example": ".env.example",
  "allowMissingEnv": true,
  "strictMode": false,
  "ignore": ["DEBUG", "VERBOSE"]
}
```

**Or YAML (.zenvrc.yaml):**

```yaml
schema: config/env.schema.json
env: .env
example: .env.example
allowMissingEnv: true
strictMode: false
ignore:
  - DEBUG
  - VERBOSE
```

**Behavior:**
- Auto-detected in project root
- Override with CLI flags
- Simplifies commands: `zenv check` uses config defaults

---

## Completed

| Feature | Version | Date |
|---------|---------|------|
| `zenv version` command with `--check-update` | 0.2.2 | 2025-01-13 |
| Env file fallback (.env.local, etc.) | 0.2.2 | 2025-01-13 |
| Improved error messages | 0.2.2 | 2025-01-13 |
| JSON output for docs (`--format json`) | 0.2.1 | 2025-01-13 |
| Variable interpolation (`${VAR}`) | 0.2.0 | 2025-01-12 |
| Multiline quoted values | 0.2.0 | 2025-01-12 |
| Escape sequences | 0.2.0 | 2025-01-12 |
| Custom validation rules | 0.2.0 | 2025-01-12 |
| Schema inheritance (`extends`) | 0.2.0 | 2025-01-12 |
| `zenv check` command | 0.1.0 | 2025-01-08 |
| `zenv docs` command | 0.1.0 | 2025-01-08 |
| `zenv init` command | 0.1.0 | 2025-01-08 |
| Type system (string/int/float/bool/url/enum) | 0.1.0 | 2025-01-08 |

---

## Contributing

Want to help? Pick a feature and open a PR!

1. Fork the repo
2. Create a feature branch
3. Implement the feature
4. Add tests
5. Submit PR

See [CONTRIBUTING.md](CONTRIBUTING.md) for details.
