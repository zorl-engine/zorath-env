# Changelog

All notable changes to this project will be documented in this file.

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
