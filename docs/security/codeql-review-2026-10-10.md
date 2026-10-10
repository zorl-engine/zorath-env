# CodeQL logging review — October 10, 2026

Reviewed all ten open `rust/cleartext-logging` findings from CodeQL 2.27.2 analysis
`1927778139`, on main commit `606e0a6386625ce111f98288b45ef17d87cbba0d`.
The SARIF data-flow paths and each producing function were inspected. These
findings are false positives; no logging rule is disabled or globally suppressed.
This review applies to these specific paths, not every possible secret-handling
path in the application.

| Alerts | Logged information | Evidence |
| --- | --- | --- |
| [17](https://github.com/zorl-engine/zorath-env/security/code-scanning/17), [18](https://github.com/zorl-engine/zorath-env/security/code-scanning/18), [20](https://github.com/zorl-engine/zorath-env/security/code-scanning/20), [21](https://github.com/zorl-engine/zorath-env/security/code-scanning/21), [25](https://github.com/zorl-engine/zorath-env/security/code-scanning/25) | Environment variable name, optional line number, detection reason | `SecretWarning` in `src/secrets.rs` has only `key`, `line`, and `reason`. `detect_secrets` fills reasons exclusively from literal messages or static pattern names. It never copies the matched value into a warning. Check and watch print only those metadata fields. The source is inferred from the `secret_warnings` identifier. |
| [14](https://github.com/zorl-engine/zorath-env/security/code-scanning/14), [24](https://github.com/zorl-engine/zorath-env/security/code-scanning/24) | Number of warnings | The trace goes through `secret_warnings.len()` into an integer count. Neither warning contents nor environment values are printed at these locations. |
| [16](https://github.com/zorl-engine/zorath-env/security/code-scanning/16) | CA certificate file path and number of certificates | `build_tls_config` passes `certs.len()` to the log. Certificate bytes and PEM private-key items are not logged. The certificate vector remains an input to the TLS root store. |
| [22](https://github.com/zorl-engine/zorath-env/security/code-scanning/22) | Synthetic MCP test fixtures on assertion failure | The test uses fixed strings `first-secret`, `continuation-secret`, and `private` written to its own temporary directory. It does not obtain real credentials. The assertion verifies those fixture values are absent from MCP output. |
| [23](https://github.com/zorl-engine/zorath-env/security/code-scanning/23) | Synthetic Kubernetes serialization fixtures on assertion failure | The test calls `export_to_string` with `ExportFormat::K8s` and a literal list of scalar/control-character fixtures. CodeQL merges the dispatcher with the separate GitHub-secrets export arm. This test does not read environment files or credentials. |

Additional executable coverage in `tests/workflows.rs` verifies that synthetic
token-pattern matches and URL passwords are absent from text and JSON check
output, and that watch startup and changed-value diagnostics do not echo either
password. The tests assert that detection messages remain visible, so a disabled
detector cannot produce a false pass. Existing MCP and Kubernetes regression
tests exercise the two flagged fixture paths.

Alerts are resolved individually as **false positive**, with the above evidence
in their dismissal comments. CodeQL remains enabled. Any future change that
copies values into warning fields or expands these logs needs a fresh review.
