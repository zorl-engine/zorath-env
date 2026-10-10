---
name: zenv
description: Validate, scan, and fix .env files using zenv -- the single-binary, language-agnostic .env validator. Use whenever the user works with .env files, environment variable schemas, secrets in env files, or wants to add/audit env vars in any project (Node, Python, Rust, Go, Ruby, PHP, anywhere). zenv runs locally with no telemetry; explicit remote schemas and update checks use HTTPS. The binary's structured exit codes + JSON output formats make it the right tool to drive from an AI agent.
---

# zenv -- .env validator for AI agents

zenv is a single Rust binary CLI that validates .env files against JSON/YAML schemas, detects leaked secrets, scans source code for env-var usage, exports .env to deployment-shaped formats, and runs entirely on the local machine. This skill teaches you when to invoke each subcommand, which flags matter for agent use, how to read the structured output, and what NOT to do.

## When to trigger this skill

Reach for zenv any time you see:

- A `.env`, `.env.local`, `.env.example`, `.env.production` etc. file in the workspace
- A `env.schema.json` or `env.schema.yaml` file
- The user asks to "add an env var", "check my .env", "validate environment", "find leaked secrets", "audit secrets", "what env vars does my code use", "generate a schema for my .env", "is this env var safe to commit", or any similar phrasing
- The agent is about to commit a change that adds/removes/modifies env-var declarations and you want to verify nothing leaked
- The user is setting up a new project and asks how to enforce env-var conventions in CI

Do NOT trigger if the user is working with shell env exports unrelated to .env files (e.g. setting PATH), or for tooling like `direnv` that switches per-directory shell env -- those are different problems.

## Confirm zenv is available

Before invoking any zenv command, verify the binary is on PATH:

```bash
command -v zenv >/dev/null 2>&1 || echo "zenv not installed"
```

If absent, tell the user how to install (do NOT install it yourself unless they ask):

```
zenv is not installed. Install with one of:
  cargo install zorath-env --version 0.3.13 --locked  # Rust 1.85+
  Download from https://github.com/zorl-engine/zorath-env/releases
```

After install confirmation, proceed with the task.

## Core commands -- when to use each

zenv has 14 commands. The five you reach for most as an agent are `check`, `scan`, `init`, `docs`, `doctor`. The rest matter in narrower situations.

### `zenv check` -- validate .env against a schema

Default invocation:

```bash
zenv check --format json
```

The `--format json` flag is critical for agent use -- it returns parseable structured output: a `valid` boolean, `errors`, `warnings`, `duplicate_warnings`, and `secret_warnings` arrays, and a `stats` object with the per-category counts. Reading the text output is fine for echoing back to a human but never parse it.

Use the `--detect-secrets` flag whenever the user asks about secrets or you're about to commit code that touches .env:

```bash
zenv check --detect-secrets --format json
```

Exit codes are structured -- read them, don't rely on the text:
- `0` = No blocking validation errors (warnings, including secret findings, may still be present)
- `1` = Validation failed (env doesn't satisfy schema)
- `2` = Usage / input error (bad flag, missing file)
- `3` = Schema error (schema file malformed or fetch failed)

For watch mode (live re-validation as files change):

```bash
zenv check --watch
```

Use watch only when the user explicitly asks for live validation -- it's a long-running process and blocks the agent.

### `zenv scan` -- find env-var usage in source code

Tells you which env vars the codebase actually reads, vs which are declared in the schema. Use this to catch drift between code and schema.

```bash
zenv scan --path src --format json
```

JSON includes `scanned_path`, `files_scanned`, `total_vars_found`, `matched_in_schema`, `missing_from_schema_count`, `missing_from_schema` (objects with `var_name` and `usages`), and `success`. Add `--show-paths` for `found_vars`; add `--show-unused` for `unused_in_code` and its count. Scan exits 1 when code uses variables absent from the schema. Scanning is pattern-based and cannot resolve every dynamic lookup.

When the user adds a new env var in code, scan to confirm it shows up:

```bash
zenv scan --path . --show-paths --format json
```

`--show-paths` adds `file:line` for each find -- use this to point the user to the exact reference.

### `zenv init` -- generate a schema from an existing .env or framework preset

Two modes. From an existing `.env.example`:

```bash
zenv init --example .env.example --schema env.schema.json
```

From a known framework (Next.js, Rails, Django, FastAPI, Express, Laravel):

```bash
zenv init --preset nextjs --schema env.schema.json
zenv init --list-presets        # see all available
```

After init, ALWAYS open the generated schema and confirm types with the user -- zenv infers conservatively, but framework-specific knowledge (like which Next.js public vars are exposed to the client) is something they should review.

### `zenv docs` -- generate human + machine docs from the schema

```bash
zenv docs > ENVIRONMENT.md       # markdown for humans
zenv docs --format json          # JSON for programmatic consumers
```

When a project doesn't have an `ENVIRONMENT.md`, suggest generating one. The schema is the source of truth; the doc is derived.

### `zenv doctor` -- health check across the env setup

```bash
zenv doctor
```

Checks the selected schema, env file, configuration, remote cache, interpolation, and validation. It returns human-readable text; use `scan` separately for code/schema drift and check Git ignore rules separately. Good first step when the user says "something is wrong with my env setup but I don't know what."

### `zenv fix` -- auto-fix common .env issues

**Always dry-run first.** Never run `zenv fix` without `--dry-run` first. This is the most important rule in this skill.

```bash
zenv fix --dry-run               # preview only
# show diff to user, get confirmation
zenv fix                         # apply
```

`fix` can add missing required vars (using schema defaults) and remove unknown keys (with `--remove-unknown`). It writes atomically with mode preservation, but it modifies the user's .env -- never apply without confirmation. The command creates a local backup without overwriting an existing backup. Keep real `.env` values and backups out of Git.

### `zenv diff` -- compare two .env files

```bash
zenv diff .env.local .env.production --format json
```

Useful when the user asks "what's different between staging and prod" or "why does my deploy fail when local works." JSON output includes `file_a`, `file_b`, `only_in_a`, `only_in_b`, `different_values`, and `identical`; `schema_compliance` is included when a schema is supplied.

Diff masks recognized sensitive keys and values. Pass `--schema env.schema.json` to honor `secret: true` declarations; an invalid supplied schema fails rather than silently removing that protection. Masking is heuristic: review output before sharing, and never treat an unmasked value as proof that it is public.

### `zenv export` -- convert .env to deployment formats

Seven output formats:

```bash
zenv export --format shell           # shell `export FOO="bar"` lines
zenv export --format docker          # Dockerfile ENV directives
zenv export --format k8s             # Kubernetes ConfigMap YAML
zenv export --format json            # JSON object
zenv export --format systemd         # systemd Environment= directives
zenv export --format dotenv          # standard .env format (round-trip)
zenv export --format github-secrets  # `gh secret set` command list
```

Exports contain actual, unmasked values. `--schema` filters keys but does not make values public or run validation. Docker output is Dockerfile `ENV` syntax, not a Docker `--env-file`. GitHub Secrets output is a shell script that changes secrets only when executed; generating it does not upload anything.

Use `export` when the user asks "how do I deploy this to Kubernetes" / "how do I get these vars into my Dockerfile" / "how do I sync these to GitHub Actions secrets."

### `zenv template` -- generate CI config

```bash
zenv template github                 # GitHub Actions workflow
zenv template gitlab                 # .gitlab-ci.yml
zenv template circleci               # CircleCI config
zenv template --list                 # available templates
```

Use when the user wants to enforce env validation in their CI but hasn't set it up.

### `zenv example` -- generate `.env.example` from the schema

```bash
zenv example > .env.example
zenv example --include-defaults > .env.example
```

Inverse of `init`: takes a schema and produces an example .env with typed placeholders. `--include-defaults` copies schema defaults, which may contain sensitive values. Review before publication. Arbitrary regex constraints can require manual edits to placeholders.

### `zenv cache` -- manage the remote schema cache

When the user uses a remote schema (`https://...`), zenv caches it locally. Use `--verify-hash` with a trusted digest to pin both cached and fresh content. Subcommands:

```bash
zenv cache list           # show cached schemas + age
zenv cache stats          # cache size, expiry summary
zenv cache clear          # nuke all
zenv cache clear <url>    # nuke one
zenv cache path           # print the cache dir
```

Rarely needed in agent flows; useful when the user asks "why is my remote schema not updating" (TTL is 3600s -- they can force a refetch with `clear` or `--no-cache`).

### `zenv version` and `zenv completions`

```bash
zenv version --check-update         # check crates.io for newer version
zenv completions bash               # generate shell completions
```

Standard CLI ergonomics -- mention when relevant.

## Schema authoring guidance

Schemas declare what env vars exist, their types, and validation rules. Minimal example:

```json
{
  "DATABASE_URL": {
    "type": "url",
    "required": true,
    "description": "Postgres connection URL",
    "secret": true
  },
  "PORT": {
    "type": "port",
    "default": "3000"
  },
  "LOG_LEVEL": {
    "type": "enum",
    "values": ["debug", "info", "warn", "error"],
    "default": "info"
  }
}
```

Available types: `string`, `int`, `float`, `bool`, `url`, `enum`, `uuid`, `email`, `ipv4`, `ipv6`, `semver`, `port`, `date`, `hostname`.

Per-var validation rules include `min`, `max`, `min_length`, `max_length`, `pattern` (regex), `min_value`, `max_value`. Severity is per-variable (`error` or `warning`). Unknown fields, incompatible rules, and invalid defaults are rejected.

Schemas can `extends` other schemas (local relative path or HTTPS URL). Inheritance is depth-capped at 10. Local parents must remain within the child schema directory after path resolution. Remote parents use the same HTTPS protections. Each parent needs its own `extends_hash` when pinning the root; a root hash alone does not pin parent content. A valid default allows a required key to be absent during `check`; it is also used by generation/repair. It is not injected into the application, which must implement its own fallback.

## Remote schemas + security

If the user fetches a schema from HTTPS, recommend pinning:

```bash
zenv check --schema https://example.com/env.schema.json --verify-hash "$SCHEMA_SHA256"
```

`--verify-hash` takes the full 64-char SHA-256 OR a prefix of at least 32 hex chars (128 bits). Shorter prefixes are rejected.

For custom trust roots on public HTTPS endpoints (private/internal destinations remain blocked):

```bash
zenv check --schema https://schemas.example.com/schema.json --ca-cert /etc/ssl/corp-ca.pem
```

Rate limiting is built in (default 60s between fetches per URL); bypass with `--no-cache` if the user knows they need a fresh fetch.

## Output handling for agents

- Use `--format json` for check, scan, diff, docs, and export when consuming structured output. Other commands, including doctor, do not provide a JSON format flag. Four MCP tools serialize JSON in text content; `zenv_doctor` returns plain text. Inspect the returned error status too.
- For log capture (CI), use `--no-color` so ANSI codes don't pollute logs.
- For quiet operation in scripts, use `--quiet` (or set `ZENV_QUIET=1`).
- For verbose debugging, use `--verbose` (or `ZENV_VERBOSE=1`).
- Read exit codes. Don't grep the text output for "OK" / "FAIL" -- the text format may change between releases; the exit codes are part of the stable API.

## Anti-patterns -- never do these

- **Never run `zenv fix` without `--dry-run` first.** Modifying the user's .env without preview is the #1 way to lose their work. Even after dry-run, get explicit confirmation before applying.
- **Never include actual secret values in commits, PRs, or chat messages.** When the user shares a `.env` excerpt with you, mask values yourself before echoing them back. zenv masks values in its own output; preserve that masking when summarizing.
- **Never run `zenv check --schema https://...` without `--verify-hash`** in a CI or production context. Obtain the expected digest from a separately trusted source and review each inherited parent pin.
- **Never trust the text output of `zenv` commands to be stable across versions.** Use `--format json` and parse it. Text output is for humans.
- **Never auto-install zenv for the user.** Suggest the install command and let them choose their package manager.
- **Never commit `.env` files** themselves, even after validation. `.env` files almost always contain secrets. Confirm `.env` is in `.gitignore`; if not, fix that before doing anything else with env vars.
- **Never silently overwrite an existing schema** with `zenv init`. If `env.schema.json` already exists, ask the user before regenerating.
- **Never use `zenv check` as a substitute for code review of new env vars.** zenv validates shape; humans validate intent. A var with a valid URL type can still point to the wrong service.

## Useful invocation recipes

**Audit a fresh codebase for env hygiene:**

```bash
zenv doctor                                      # high-level status
zenv check --detect-secrets --format json        # validate + scan for leaked secrets
zenv scan --path . --format json                 # code-vs-schema drift
```

**Add a new env var to a Node project:**

```bash
# 1. Add the var to .env locally
# 2. Add the declaration to env.schema.json
# 3. Verify nothing else broke
zenv check --format json
# 4. Confirm it shows up in code scan once code uses it
zenv scan --path src --format json
```

**Pre-deploy sanity check:**

```bash
zenv check --env .env.production --detect-secrets --format json
zenv diff .env.local .env.production --format json   # what changed between envs
zenv export --env .env.production --format k8s       # build k8s ConfigMap
```

**Set up CI validation for a new repo:**

```bash
zenv init --example .env.example
zenv template github > .github/workflows/env-validation.yml
zenv example > .env.example                          # public example
zenv docs > docs/ENVIRONMENT.md                      # human docs
```

**Diagnose a "missing env var" production incident:**

```bash
zenv doctor                                          # high-level
zenv scan --path . --show-paths --format json        # where is the var used?
zenv diff .env.local .env.production --format json   # what's different?
```

## Edge cases worth knowing

- Prefer structured output where supported; terminal presentation can vary by platform.
- The default schema search order is: explicit `--schema` flag, then `.zenvrc` config, then `env.schema.json`, then `env.schema.yaml`, then `env.schema.yml`.
- Env selection uses `--env`, then `.zenvrc`, then `.env`. When the selected name is the default `.env` and it is absent, fallback discovery tries `.env.local`, `.env.development`, then `.env.development.local`. A different explicit missing path is an error.
- `.zenvrc` is a JSON config file at the repo root that pins defaults (schema path, env path, output format, rate limit, etc.). Discovery stops at project markers such as `.git` or `Cargo.toml`; relative config paths are resolved from the config directory. Invalid config fails visibly. Check the selected file before suggesting command lines.
- Watch mode (`--watch`) uses filesystem events and polling; local inherited schemas and repeated atomic saves are covered by regression tests. Verify behavior on the actual mount used by the project.
- On Windows, paths in command output use `\`. The CLI accepts both `/` and `\` in path arguments.

## When the user asks "should I use zenv or X?"

zenv is a **CI / agent-time validator** that runs locally. It does NOT inject env vars into your runtime, it does NOT encrypt .env files, and it does NOT replace your secrets-management vendor (Vault / Doppler / Infisical). It validates that whatever .env hits CI conforms to a schema, scans for leaked secrets, and exits with a structured code.

If the user needs runtime injection of secrets into their app, that's a different category of tool. If they need encrypted-at-rest .env files committed to git, that's also different. If they need hosted org-wide secret rotation with audit logs, that's a SaaS category. zenv is the validator layer that sits in front of any of those.

For polyglot codebases, zenv provides one schema and CLI across languages. Compare requirements and current alternatives using `docs/comparison.md`; avoid unsupported exclusivity claims.

## Updating this skill

This skill ships in the zenv repo at `integrations/claude-code/SKILL.md`. To update your local copy:

```bash
curl -o ~/.claude/skills/zenv/SKILL.md \
  https://raw.githubusercontent.com/zorl-engine/zorath-env/main/integrations/claude-code/SKILL.md
```

If you fix a bug or improve a workflow here, contribute the patch back via PR -- the skill is MIT-licensed, same as zenv itself.
