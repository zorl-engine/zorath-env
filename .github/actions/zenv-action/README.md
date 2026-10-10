# zenv GitHub Action

Validate `.env` files against JSON or YAML schemas (local files or HTTPS URLs) in your CI/CD pipeline.

## Usage

```yaml
- name: Validate environment
  uses: zorl-engine/zorath-env/.github/actions/zenv-action@v0.3.13
  with:
    version: '0.3.13'
    schema: env.schema.json
    env-file: .env.example
```

## Inputs

| Input | Description | Required | Default |
|-------|-------------|----------|---------|
| `schema` | Local JSON/YAML schema path or HTTPS URL | No | `env.schema.json` |
| `env-file` | Path to the .env file to validate | No | `.env` |
| `allow-missing-env` | Allow missing .env file | No | `false` |
| `version` | Version of zenv to use | No | `latest` |
| `skip-install` | Use an existing zenv on PATH, useful when testing a source build | No | `false` |
| `detect-secrets` | Scan for embedded credentials | No | `false` |
| `verify-hash` | Remote schema SHA-256 digest or at least 32 hex characters | No | Empty |
| `ca-cert` | Custom CA certificate path | No | Empty |
| `no-cache` | Bypass remote schema cache | No | `false` |
| `format` | Validation output: `text` or `json` | No | `text` |

The action requires Bash, curl, and jq, available on GitHub-hosted runners. Input paths are passed as literal arguments, including paths containing spaces or shell metacharacters. JSON validation errors are returned as an array of messages; input/schema failures include the CLI diagnostics.

Linux and Windows prebuilt downloads target x86-64; macOS has Intel and Apple Silicon assets. The Linux asset is built on Ubuntu 24.04 with glibc; use `ubuntu-24.04` or a compatible runner, not Alpine. Unsupported Linux/Windows architectures or unavailable downloads fall back to Cargo (Rust 1.85+ for v0.3.13). Downloads do not yet have automatic consumer-side provenance verification. For a verified preinstalled binary, follow the [installation guide](https://github.com/zorl-engine/zorath-env/wiki/Installation) and set `skip-install`. `skip-install: 'true'` skips version lookup, download, and cache restoration, and validates with the binary already on PATH.

## Outputs

| Output | Description |
|--------|-------------|
| `valid` | Whether validation passed (`true`/`false`) |
| `errors` | JSON array of error messages |

Secret findings and warning-severity validation do not fail the Action by themselves. Inspect `secret_warnings` in a CLI JSON report if your policy requires rejection; see [CI Integration](https://github.com/zorl-engine/zorath-env/wiki/CI-Integration#fail-ci-on-secret-findings).

Pin both the Action reference and the `version` input for reproducibility: the reference selects Action code, while `version` selects the installed executable.

## Examples

### Basic validation

```yaml
name: Validate Environment

on: [push, pull_request]

jobs:
  validate:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v6

      - name: Validate .env
        uses: zorl-engine/zorath-env/.github/actions/zenv-action@v0.3.13
        with:
          version: '0.3.13'
          schema: env.schema.json
          env-file: .env.example
```

### Validate multiple environments

```yaml
jobs:
  validate:
    runs-on: ubuntu-24.04
    strategy:
      matrix:
        env: [development, staging, production]
    steps:
      - uses: actions/checkout@v6

      - name: Validate ${{ matrix.env }}
        uses: zorl-engine/zorath-env/.github/actions/zenv-action@v0.3.13
        with:
          version: '0.3.13'
          schema: env.schema.json
          env-file: .env.${{ matrix.env }}.example
```

### Use validation output

```yaml
- name: Validate .env
  id: validate
  uses: zorl-engine/zorath-env/.github/actions/zenv-action@v0.3.13
  continue-on-error: true
  with:
    version: '0.3.13'
    schema: env.schema.json
    env-file: .env

- name: Report errors
  if: steps.validate.outcome == 'failure'
  shell: bash
  env:
    ZENV_ERRORS: ${{ steps.validate.outputs.errors }}
  run: |
    echo "Validation failed with errors:"
    printf '%s\n' "$ZENV_ERRORS" | jq -r '.[]'
    exit 1
```

### Pin to specific version

```yaml
- name: Validate .env
  uses: zorl-engine/zorath-env/.github/actions/zenv-action@v0.3.13
  with:
    version: '0.3.13'
    schema: env.schema.json
    env-file: .env
```
