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
| `allow-missing-env` | Allow missing .env file | No | `true` |
| `version` | Version of zenv to use | No | `latest` |

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
      - uses: actions/checkout@v4

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
      - uses: actions/checkout@v4

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
    version: '0.3.5'
```
