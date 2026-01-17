# Changelog

All notable changes to this project will be documented in this file.

## [0.3.3] - 2026-01-17

### Added
- Remote schema support: fetch schemas from HTTPS URLs
  - `zenv check --schema https://example.com/env.schema.json`
  - Automatic caching with 1-hour TTL
  - `--no-cache` flag to force fresh fetch
  - HTTPS only (HTTP rejected for security)
- Schema inheritance works with remote URLs
  - Remote schemas can extend other remote schemas
  - Relative URLs resolved against parent schema URL
- 4 new tests for remote schema functionality (189 total)

### Changed
- `load_schema` now accepts both local paths and HTTPS URLs

## [0.3.2] - 2026-01-16

### Added
- `zenv diff` command to compare two .env files
  - Shows variables only in first file, only in second file, and with different values
  - Optional `--schema` flag for compliance checking both files
- `--detect-secrets` flag for `zenv check` command
  - Detects AWS keys, Stripe/GitHub/GitLab/Slack tokens
  - Detects private key headers (RSA, SSH, PGP)
  - Detects JWT tokens and URLs with embedded passwords
  - Detects high-entropy strings (potential secrets)
- 17 new tests (185 total)

## [0.3.1] - 2026-01-15

### Added
- Windows support in GitHub Action (downloads `zenv.exe` for Windows runners)
- Sidebar navigation for GitHub wiki

### Changed
- Updated crates.io metadata: homepage now points to zorl.cloud/zenv, documentation to GitHub wiki
- Removed unused `parse_env_file_interpolated` function (dead code cleanup)
- Improved GitHub Action reliability with `jq` for version parsing

### Fixed
- GitHub Action test workflow syntax errors

## [0.3.0] - 2025-01-15

### Added
- `zenv completions` command for shell auto-completion (bash, zsh, fish, powershell)
- `zenv example` command to generate `.env.example` from schema
  - `--include-defaults` flag to populate default values
  - `--output` flag to write to file instead of stdout
- GitHub Action for CI/CD validation (`.github/actions/zenv-action`)
  - Inputs: `schema`, `env-file`, `allow-missing-env`, `version`
  - Outputs: `valid`, `errors`
- 19 new tests (168 total)

## [0.2.2] - 2025-01-13

### Added
- `zenv version` command with optional `--check-update` flag to query crates.io
- Auto-detection of `.env.local`, `.env.development`, `.env.development.local` when `.env` is missing
- Helpful error messages showing which env files were checked
- Tip about unknown keys count when validation fails

### Changed
- Improved error output formatting for missing env files

## [0.2.1] - 2025-01-13

### Added
- `--format` flag for `docs` command
- JSON output format (`zenv docs --format json`)
- 3 new tests for JSON output

## [0.2.0] - 2025-01-12

### Added
- Variable interpolation (`${VAR}` and `$VAR` syntax)
- Multiline quoted values
- Escape sequences in double-quoted strings (`\n`, `\t`, `\r`, `\\`, `\"`)
- Custom validation rules (`min`, `max`, `min_value`, `max_value`, `min_length`, `max_length`, `pattern`)
- Schema inheritance via `extends` field
- Circular reference detection for both interpolation and inheritance

### Changed
- Expanded test coverage to 141 tests

## [0.1.3] - 2025-01-11

### Added
- 90 unit tests
- Improved SEO metadata

## [0.1.2] - 2025-01-10

### Changed
- Simplified binary names
- Updated Cargo.lock

## [0.1.1] - 2025-01-09

### Fixed
- Release workflow permissions

## [0.1.0] - 2025-01-08

### Added
- Initial release
- `zenv check` command for .env validation
- `zenv docs` command for Markdown documentation generation
- `zenv init` command for schema creation from .env.example
- Support for types: string, int, float, bool, url, enum
- Required/optional field validation
- Unknown key detection
- CI-friendly exit codes
