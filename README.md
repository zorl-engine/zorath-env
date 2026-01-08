<p align="center">
  <img src="assets/logo.png" alt="Zorath" width="200">
</p>

# zorath-env

**Built by Zorath -- infrastructure for builders.**

A tiny, fast CLI that makes `.env` sane.

`zenv` validates environment variables from a schema, generates docs, and helps keep config consistent across dev/staging/prod.

## Why

`.env` files drift. Teams copy/paste secrets. CI fails late. Docs go stale.

`zenv` makes your schema the source of truth.

> **Schema is the source of truth.** Docs and examples should be generated from it.

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

Prints Markdown documentation for all env vars in the schema.

### `zenv init`

Creates `env.schema.json` from `.env.example` (best-effort inference, you refine types after).

## Files

By default, `zenv` looks for:

* `.env` (optional)
* `.env.example` (optional)
* `env.schema.json` (preferred)

You can override paths:

```bash
zenv check --env .env --schema env.schema.json
zenv docs  --schema env.schema.json
zenv init  --example .env.example --schema env.schema.json
```

## Schema format (v0.1)

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
    "description": "HTTP port"
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

## License

MIT
