<p align="center">
  <img src="assets/logo.png" alt="Zorath" width="200">
</p>

# zorath-env

[![Crates.io](https://img.shields.io/crates/v/zorath-env.svg)](https://crates.io/crates/zorath-env)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![More from zorl-engine](https://img.shields.io/badge/zorl--engine-more-blue)](https://edgeurl.io/p/zorl-engine)

**Package:** `zorath-env` | **Binary:** `zenv`

**Built by Zorath -- infrastructure for builders.**

A tiny, fast CLI that makes `.env` sane.

`zenv` validates environment variables from a schema, generates docs, and helps keep config consistent across dev/staging/prod.

## Why

`.env` files drift. Teams copy/paste secrets. CI fails late. Docs go stale.

`zenv` makes your schema the source of truth.

> **Schema is the source of truth.** Docs and examples should be generated from it.

## Privacy

zenv runs locally. No uploads, no secrets fetching, no phoning home.

## Works with any stack

`zenv` is language-agnostic. Use it with Node.js, Python, Go, Ruby, Rust, Java, PHP, or any project that uses `.env` files. It's a standalone binary with zero runtime dependencies.

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

## Quick start

1. Create a schema:

```bash
zenv init
```

2. Validate your `.env`:

```bash
zenv check
```

3. Generate docs:

```bash
zenv docs > ENVIRONMENT.md
```

## Commands

### `zenv check`

Validates `.env` against `env.schema.json`.

* exits `0` if valid
* exits `1` if invalid (CI-friendly)

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

## Schema format (v0.2)

`env.schema.json` is a JSON object where each key is an env var name.

Example:

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
    "type": "int",
    "default": 3000,
    "required": false,
    "description": "HTTP port",
    "validate": {
      "min": 1024,
      "max": 65535
    }
  }
}
```

Supported types:

* `string`
* `int`
* `float`
* `bool`
* `url`
* `enum`

### Validation rules

Add constraints with the `validate` field:

```json
{
  "PORT": { "type": "int", "validate": { "min": 1024, "max": 65535 } },
  "RATE": { "type": "float", "validate": { "min_value": 0.0, "max_value": 1.0 } },
  "API_KEY": { "type": "string", "validate": { "min_length": 32, "pattern": "^sk_" } }
}
```

### Schema inheritance

Schemas can extend other schemas:

```json
{
  "extends": "base.schema.json",
  "EXTRA_VAR": { "type": "string" }
}
```

## .env features

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

## Connect

More from Zorath: [edgeurl.io/p/zorl-engine](https://edgeurl.io/p/zorl-engine)

## License

MIT

