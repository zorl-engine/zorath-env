# Changelog

All notable changes to this project will be documented in this file.

## [0.3.9] - 2026-04-13

### Fixed
- **CI template URLs**: Fixed GitHub/GitLab/CircleCI templates pointing to old org `zorath-net` instead of `zorl-engine`
- **GitHub Action fallback version**: Updated from hardcoded `0.3.7` to `0.3.8`
- **`--ca-cert` now works**: Custom CA certificates are actually applied to TLS connections instead of being validated and discarded
- **Doctor config check**: Removed false `.zenv.json` lookup (config only uses `.zenvrc`)

### Added
- **`--config` global flag**: Load custom `.zenvrc` from any path (`zenv --config path/to/.zenvrc check`)
- **`--verbose`/`--quiet` global flags**: Control diagnostic output across all commands (mutually exclusive)
- **`--no-color` global flag**: Disable colored output via CLI flag (also respects `NO_COLOR` env var and `.zenvrc`)
- **`format` in `.zenvrc`**: Set default output format in config (`{"format": "json"}`) -- CLI `--format` still overrides
- **`CliError` structured error enum**: Replaced string-based exit code detection with typed error variants (`Validation`/`Input`/`Schema` -> exit 1/2/3). Removed brittle `determine_exit_code()` string matching.
- **`#[doc(hidden)]` on CLI internals**: All command `run()` functions marked as internal, keeping `cargo doc` output focused on the stable library API
- **16 CLI-level integration tests**: End-to-end binary tests via `std::process::Command` covering exit codes, JSON output, global flags, config merge, and all major commands
- 686 total tests (545 unit + 141 integration)

### Fixed
- **`--allow-missing-env` default**: Changed from `true` to `false` -- missing `.env` now fails by default (safe default)
- **Secret detection line numbers**: Removed duplicate line-number parser from `secrets.rs`; now uses envfile parser's `line_numbers` directly (single source of truth)

### Changed
- Removed unused direct dependencies: `rustls`, `rustls-pemfile`, `webpki-roots` (provided transitively by `ureq`)
- **Dead code cleanup**: Removed `config_exists()`, `config_path()`, `load_schema()` dead code wrappers; unwired `#[allow(dead_code)]` from `no_color`/`rate_limit_seconds`
- **`SchemaError::Write`**: `save_schema()` now uses proper `Write` error variant instead of misusing `Read`
- **Short flags**: Added `-f` (format) to `docs`, `-o` (output) to `example` for consistency with `export`
- **`--list-presets` moved**: Logic moved from `main.rs` to `init.rs` where it belongs
- **`SecurityOptions::default()`**: Now uses `DEFAULT_RATE_LIMIT_SECS` (60s) matching `::new()` -- no more silent mismatch
- **Doctor remote flags**: Added `--no-cache`, `--verify-hash`, `--ca-cert` to `doctor` command for remote schema support
- **Removed redundant `list_presets()`**: Callers use `AVAILABLE_PRESETS` constant directly
- **`detect_secrets` signature**: Takes `&HashMap<String, usize>` (line numbers) instead of raw `&str` content

## [0.3.8] - 2026-01-25

### Added
- **`zenv cache stats`**: New subcommand showing cache statistics
  - Total schemas, size, TTL, age range, expired count
  - Quick overview of cache health
- **GitHub Secrets export format**: `zenv export --format github-secrets`
  - Generates shell script with `gh secret set` commands
  - Handles multiline values and special characters
  - Ready-to-run with GitHub CLI
- **Structured exit codes** for CI/CD integration
  - Exit 1: Validation failures (check failed)
  - Exit 2: Input/file errors (not found, failed to read)
  - Exit 3: Schema errors (invalid JSON, parse failures)
- **Typo detection in diff**: "Did you mean?" suggestions
  - Detects possible typos between compared files
  - Uses Levenshtein distance for smart matching
- **Config key validation**: Warns about unknown keys in `.zenvrc`
  - Lists valid configuration options
  - Helps catch typos in config files
- **Actionable fix suggestions**: Check command now suggests `zenv fix`
  - Shows when validation fails with auto-fixable issues
  - Includes hint about `--remove-unknown` flag
- 250 new tests (630 total)

### Changed
- **Secret masking in fix --dry-run**: Sensitive values now display as `***MASKED***`
  - Prevents accidental exposure in logs/screenshots
  - Uses same detection as `--detect-secrets`
- **Improved CLI help**: Added examples for security flags
  - `--verify-hash` and `--ca-cert` usage examples
  - Clearer documentation for cache, diff, fix, export commands
- **Performance**: Regex patterns in scan command now use OnceLock caching
  - Patterns compiled once and reused across calls
  - Faster repeated scans in watch mode or library usage

## [0.3.7] - 2026-01-20

### Added
- **`zenv template` command**: Generate CI/CD configuration templates
  - `github` - GitHub Actions workflow for env validation
  - `gitlab` - GitLab CI configuration
  - `circleci` - CircleCI configuration
  - Supports aliases (gh, gl, circle) and `--output` flag
- **Duplicate key detection**: Warns when .env files contain duplicate keys
  - Shows line numbers for both original and duplicate definitions
  - Included in JSON output (`duplicate_warnings` array)
  - Prevents silent value overwrites from copy-paste errors
- **Library APIs for embedding**: zenv is now both a library and CLI
  - `check::validate_files()` - convenience wrapper for file validation
  - `export::export_to_string()` - export to string without file I/O
  - `docs::generate()` - unified docs generation (markdown/json)
  - `example::generate()` - generate .env.example content to string
- **29 integration tests** in `tests/integration_tests.rs`
  - End-to-end testing with real files via tempfile
  - Coverage for check, docs, fix, init, and edge cases
- **Version-aware build system** (`build.rs`)
  - Detects stale cache and warns on version mismatch
  - Forces rebuild when Cargo.toml, Cargo.lock, or src files change
  - Writes version stamp to target/ for comparison
- **Cargo aliases** (`.cargo/config.toml`)
  - `cargo fresh` - clean build
  - `cargo rel` - release build
  - `cargo t` - run tests
  - `cargo lint` - run clippy
- 380 total tests (351 unit + 29 integration)

### Changed
- Refactored completions module to accept Command from caller for testability

## [0.3.6] - 2026-01-19

### Fixed
- GitHub Action quoting bug causing test failures
- `--allow-missing-env` flag now correctly skips validation when env file is missing
- Clippy warnings with allow annotations

## [0.3.5] - 2026-01-19

### Added
- **YAML schema format support**: Use `.yaml` or `.yml` schemas alongside JSON
  - Auto-detection by file extension
  - Mixed format inheritance (YAML can extend JSON and vice versa)
  - YAML supports comments for better documentation
- **4 new validation types**:
  - `port`: Validates port numbers (1-65535)
  - `ipv6`: Validates IPv6 addresses
  - `date`: Validates ISO 8601 dates (YYYY-MM-DD)
  - `hostname`: Validates RFC 1123 hostnames
- **`zenv check --format json`**: JSON output for CI/CD pipelines
  - Structured errors, warnings, and secret warnings
  - Machine-readable validation results
- **`zenv export` command**: Export .env to multiple formats
  - `--format shell` (export FOO="bar")
  - `--format docker` (ENV FOO=bar)
  - `--format k8s` (Kubernetes ConfigMap YAML)
  - `--format json` (JSON object)
  - `--format systemd` (Environment=FOO=bar)
  - `--format dotenv` (standard .env format)
  - `--schema` flag to filter to schema-defined variables
- **`zenv doctor` command**: Health check and diagnostics
  - Checks schema file exists and parses
  - Checks .env file exists and parses
  - Checks config file (.zenvrc) validity
  - Checks remote schema cache
  - Runs validation test if both files exist
  - Actionable suggestions for each issue
- **Severity levels**: `"severity": "warning"` in schema
  - Warnings don't cause exit code 1
  - Separate warnings from errors in output
  - JSON output includes errors and warnings arrays
- `zenv fix` command: Auto-fix common .env issues
  - Creates backup before modifying
  - `--remove-unknown` flag to remove undefined keys
  - `--dry-run` to preview changes
- `zenv scan` command: Scan source code for env var usage
  - Supports 9 languages (JS/TS, Python, Go, Rust, PHP, Ruby, Java, C#, Kotlin)
  - `--show-unused` to find vars in schema but not in code
  - `--show-paths` to display file:line for all found variables
  - `--format json` for CI integration
- `zenv cache` command: Manage remote schema cache
  - `cache list` to show cached schemas
  - `cache clear` to remove cached entries
  - `cache path` to show cache directory
- `no_color` config option: Disable colored output (respects `NO_COLOR` env var)
- **Remote schema security features**:
  - `--verify-hash` flag for SHA-256 content verification
  - `--ca-cert` flag for custom CA certificates (PEM format)
  - Rate limiting (60s default, configurable via `.zenvrc`)
  - Hash prefix matching (16+ chars) for convenience
- 10 new tests (341 total)

### Changed
- Schema error messages now indicate format (JSON vs YAML)
- `save_schema` auto-detects output format from file extension
- Check command now separates errors (exit 1) from warnings (no exit)

## [0.3.4] - 2026-01-18

### Added
- **Watch mode**: `zenv check --watch` for continuous validation
  - Delta detection: only shows changed variables
  - Schema change detection: revalidates on schema updates
  - Timestamped output with terminal bell on errors
- `--format json` for diff command: machine-readable output
- Smart description inference in `zenv init`
  - Infers descriptions from key names (DATABASE_URL, API_KEY, etc.)
  - Service name extraction (STRIPE_API_KEY -> "Stripe API key")
- Type-aware placeholders in `zenv example`
  - PORT -> 3000, DATABASE_URL -> postgres://..., API_KEY -> your_api_key_here
- Duplicate key warnings with line numbers in .env parser
- Validation rules shown in `zenv docs` output (min, max, pattern, etc.)
- Actionable tips for unknown keys ("To add them: zenv init...")
- Changelog and releases links shown on version update available
- 16 new tests (205 total)

### Changed
- Improved watch mode schema error display with context-aware tips

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
