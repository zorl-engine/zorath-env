# zenv and related tools

Reviewed October 10, 2026 against the projects' own documentation. This is a workflow comparison, not a claim that zenv has exclusive features.

| Tool | Main role | How it relates to zenv |
|---|---|---|
| zenv | Validate `.env` files against JSON/YAML contracts before deployment; generate docs/examples; inspect code references; export values; expose local MCP tools | One Rust CLI for the existing build-time workflow. It does not launch applications, inject defaults, encrypt files, or retrieve secrets from a vault. |
| [dotenv-linter](https://github.com/dotenv-linter/dotenv-linter) | Lint and fix dotenv syntax/style | Useful alongside semantic schema validation. zenv's parser remains permissive about malformed lines and unclosed quotes. |
| [envalid](https://github.com/af/envalid) | Validate and type environment configuration inside JavaScript runtimes | Application startup validation and type inference complement a language-independent CI check. |
| [dotenvx](https://dotenvx.com/docs/install/) | Environment loading and encrypted dotenv workflows | Its installation options include standalone distribution; the former comparison's mandatory Node dependency claim was inaccurate. |
| [direnv](https://direnv.net/) | Load/unload environment settings when entering directories | Shell activation is separate from validating a file's values. |
| [SOPS](https://github.com/getsops/sops) | Encrypt configuration files with supported key systems | Supports age/PGP as well as cloud KMS; cloud KMS is not mandatory. zenv can validate decrypted files but does not manage encryption keys. |
| [Varlock](https://varlock.dev/) | Schema-driven environment resolution, runtime loading, secret-provider integrations, type generation, and agent-oriented protections | There is substantial overlap in validation and AI workflows. These are useful design references, not evidence that zenv needs every platform feature to complete its existing CLI. |

## What completion means for zenv

The existing promise is a reliable build-time validator and its supporting CLI/MCP workflows. A release should have consistent schema validation and masking, dependable watch behavior, explicit failure codes, safe output generation, working CI integrations, and installable tested artifacts. See [TESTED.md](../TESTED.md) for the current audit and release gates.

Useful future work, in order:

1. Specify a strict dotenv dialect and add an opt-in syntax-error mode; distinguish literal single-quoted values from interpolation. Test against the parsers users actually deploy.
2. Add an explicit CI policy to fail on detected secrets. Current secret findings are warnings; `--detect-secrets` alone is not a prevention gate.
3. Publish a versioned schema specification and machine-readable schema for editor completion, with compatibility rules for future fields.
4. Add conditional requirements and mutual exclusions only after defining how they interact with defaults, interpolation, inheritance, and severity.
5. Consider type generation, environment overlays, `run`/process injection, vault integrations, or editor support as separate product extensions. None is currently implemented merely because it appears in the local roadmap.

These priorities are recommendations from the audit. They are not claims that the proposed features have been delivered.
