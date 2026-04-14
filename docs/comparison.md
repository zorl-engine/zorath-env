# zenv vs Alternatives

How does zenv compare to other .env and environment variable tools?

## At a Glance

| Feature | zenv | dotenv-linter | envalid | dotenvx | direnv | sops |
|---------|------|---------------|---------|---------|--------|------|
| **Language** | Rust | Rust | TypeScript | JavaScript | Go | Go |
| **Standalone binary** | Yes | Yes | No (Node lib) | Yes (npm) | Yes | Yes |
| **Zero runtime deps** | Yes | Yes | No (Node) | No (Node) | Yes | No (KMS) |
| **Schema validation** | Yes (14 types) | No (syntax only) | Yes (8 types) | No | No | No |
| **Custom validation rules** | Yes (min, max, pattern, length) | No | Yes (custom fns) | No | No | No |
| **Secret detection** | Yes (15 patterns + entropy) | No | No | Yes (scan) | No | N/A |
| **Auto-fix** | Yes (with backup) | Yes | No | No | No | No |
| **Watch mode** | Yes (delta detection) | No | No | No (cd trigger) | No | No |
| **Docs generation** | Yes (Markdown/JSON) | No | No | No | No | No |
| **Export formats** | 7 | 0 | 0 | 0 | 0 | 5 |
| **Code scanning** | Yes (9 languages) | No | No | No | No | No |
| **CI/CD templates** | Yes (3 platforms) | Yes (GitHub) | No | Yes (GitHub) | No | No |
| **Encryption** | No | No | No | Yes (AES-256) | No | Yes (multi-KMS) |
| **Runtime injection** | No | No | Yes (in-process) | Yes | Yes (shell) | No |
| **Schema inheritance** | Yes (remote + local) | No | No | No | No | No |
| **Remote schemas** | Yes (HTTPS + SHA-256 hash) | No | No | No | No | No |
| **YAML schema support** | Yes | No | No | No | No | Yes |
| **Shell completions** | 4 shells | No | No | bash | many | No |
| **Config file** | Yes (.zenvrc) | No | No | No | Yes (.envrc) | Yes (.sops.yml) |
| **JSON output (CI)** | Yes | No | No | No | Yes | Yes |
| **Severity levels** | Yes (warning/error) | No | No | No | No | No |
| **Library API** | Yes (Rust crate) | Yes | Yes | Yes (npm) | No | No |
| **Env file diff** | Yes (with typo detection) | Yes | No | No | No | No |

## Detailed Comparison

### zenv vs dotenv-linter

[dotenv-linter](https://github.com/dotenv-linter/dotenv-linter) is the closest competitor -- both are Rust binaries that work with `.env` files.

**Where dotenv-linter fits:** Fast syntax linting. Catches formatting issues (extra spaces, incorrect key names, duplicate keys, unordered keys). Good for enforcing `.env` file hygiene.

**Where zenv fits:** Semantic validation. Catches type errors, missing required vars, invalid values, leaked secrets, and unused schema vars. Generates docs, exports to deployment formats, scans code for env var usage.

**Key differences:**
- dotenv-linter checks **syntax** (is this a well-formed `.env` file?)
- zenv checks **semantics** (does this `.env` satisfy the schema's type and value requirements?)
- dotenv-linter has no schema -- it can't validate that `PORT=abc` is wrong because it doesn't know `PORT` should be an integer
- zenv has 14 typed validators, custom rules, severity levels, and schema inheritance
- dotenv-linter doesn't generate docs, export formats, scan code, or detect secrets

**Can you use both?** Yes. dotenv-linter catches formatting issues. zenv catches logic issues. They're complementary.

### zenv vs envalid

[envalid](https://github.com/af/envalid) is a TypeScript library for validating environment variables at runtime inside Node.js applications.

**Where envalid fits:** In-process validation with TypeScript type inference. Ensures your app starts with valid config. Deep integration with Node.js ecosystem. Custom validator functions.

**Where zenv fits:** Build-time/CI validation. Language-agnostic. No runtime dependency. Works with any stack, not just Node.js.

**Key differences:**
- envalid runs inside your app at startup; zenv runs before your app in CI/CD
- envalid has 8 types; zenv has 14
- envalid requires Node.js; zenv is a standalone binary
- zenv adds export formats, docs generation, code scanning, auto-fix, watch mode, and secret detection -- features outside envalid's scope

**Can you use both?** Yes. Use zenv in CI to catch issues early. Use envalid in your Node.js app for runtime safety.

### zenv vs dotenvx

[dotenvx](https://github.com/dotenvx/dotenvx) is the next-generation dotenv from the original dotenv creator. Its focus is encryption and multi-environment management.

**Where dotenvx fits:** Encrypting `.env` files so they can be safely committed to git. Managing multiple environments (dev, staging, prod) with encrypted values. Runtime injection across languages.

**Where zenv fits:** Validating that `.env` values are correct according to a typed schema. Catching misconfigurations before deployment.

**Key differences:**
- dotenvx solves "how do I safely store secrets in git?" (encryption)
- zenv solves "how do I know my config is correct?" (validation)
- dotenvx has no schema validation or type checking
- zenv has no encryption

**Can you use both?** Yes, and you probably should. Encrypt with dotenvx, validate with zenv.

### zenv vs direnv

[direnv](https://github.com/direnv/direnv) automatically loads and unloads environment variables when you enter/leave a directory.

**Where direnv fits:** Developer workflow. Automatically activates project-specific env vars when you `cd` into a directory. Shell integration.

**Where zenv fits:** Validation and documentation. Ensures the values in your `.env` are correct, generates docs, exports to deployment formats.

**Key differences:**
- direnv manages **loading** env vars; zenv manages **validating** them
- direnv has zero validation capabilities
- zenv doesn't load env vars into your shell

**Can you use both?** Yes. Load with direnv, validate with zenv.

### zenv vs sops

[sops](https://github.com/getsops/sops) (CNCF project) encrypts secrets in config files using AWS KMS, GCP KMS, Azure Key Vault, age, or PGP.

**Where sops fits:** Encrypting secrets at rest. GitOps workflows where encrypted config files are committed. Multi-cloud KMS integration.

**Where zenv fits:** Validating that config values are correct. Checking types, required fields, and schema compliance.

**Key differences:**
- sops encrypts values; zenv validates them
- sops requires KMS infrastructure; zenv requires nothing
- They solve completely different problems

**Can you use both?** Yes. Encrypt secrets with sops, validate the decrypted result with zenv.

## Features Only zenv Has

These features are not available in any of the tools compared above:

1. **Code scanning across 9 languages** -- find env vars used in your source code (JS/TS, Python, Go, Rust, PHP, Ruby, Java, C#, Kotlin) that aren't covered by your schema
2. **Schema inheritance with remote schemas** -- extend base schemas from HTTPS URLs with SHA-256 hash verification
3. **14 typed validators** in a standalone binary -- the most comprehensive type system for `.env` validation
4. **Documentation generation** -- generate Markdown or JSON docs from your schema
5. **7 export formats** -- shell, Docker, Kubernetes, systemd, GitHub Secrets, JSON, dotenv
6. **Watch mode with delta detection** -- re-validates only changed variables, shows exactly what changed
7. **Doctor diagnostics** -- full health check of your schema, env file, config, cache, and validation
8. **Severity levels** -- mark validations as warnings (don't fail CI) or errors (fail CI)

## Summary

| If you need... | Use |
|----------------|-----|
| Schema-driven validation with typed checking | **zenv** |
| Syntax linting for `.env` formatting | dotenv-linter |
| Runtime validation inside Node.js | envalid |
| Encrypted `.env` files in git | dotenvx |
| Auto-load env vars per directory | direnv |
| Encrypt secrets with cloud KMS | sops |
| All of the above (validation + encryption + loading) | **zenv** + dotenvx + direnv |
