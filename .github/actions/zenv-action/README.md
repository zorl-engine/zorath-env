# zenv GitHub Action

Validate `.env` files against JSON schemas in your CI/CD pipeline.

## Usage

```yaml
- name: Validate environment
  uses: zorl-engine/zorath-env/.github/actions/zenv-action@main
  with:
    schema: env.schema.json
    env-file: .env.example
```

## Inputs

| Input | Description | Required | Default |
|-------|-------------|----------|---------|
| `schema` | Path to the JSON schema file | No | `env.schema.json` |
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

Linux and Windows prebuilt downloads target x86-64. Other runner architectures use Cargo to build the requested version. `skip-install: 'true'` skips version lookup, download, and cache restoration, and validates with the binary already on PATH.

## Outputs

| Output | Description |
|--------|-------------|
| `valid` | Whether validation passed (`true`/`false`) |
| `errors` | JSON array of error messages |

## Examples

### Basic validation

```yaml
name: Validate Environment

on: [push, pull_request]

jobs:
  validate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6

      - name: Validate .env
        uses: zorl-engine/zorath-env/.github/actions/zenv-action@main
        with:
          schema: env.schema.json
          env-file: .env.example
```

### Validate multiple environments

```yaml
jobs:
  validate:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        env: [development, staging, production]
    steps:
      - uses: actions/checkout@v6

      - name: Validate ${{ matrix.env }}
        uses: zorl-engine/zorath-env/.github/actions/zenv-action@main
        with:
          schema: env.schema.json
          env-file: .env.${{ matrix.env }}.example
```

### Use validation output

```yaml
- name: Validate .env
  id: validate
  uses: zorl-engine/zorath-env/.github/actions/zenv-action@main
  continue-on-error: true
  with:
    schema: env.schema.json
    env-file: .env

- name: Report errors
  if: steps.validate.outputs.valid == 'false'
  run: |
    echo "Validation failed with errors:"
    echo '${{ steps.validate.outputs.errors }}' | jq -r '.[]'
```

### Pin to specific version

```yaml
- name: Validate .env
  uses: zorl-engine/zorath-env/.github/actions/zenv-action@main
  with:
    schema: env.schema.json
    env-file: .env
    version: '0.3.9'
```
