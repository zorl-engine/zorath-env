"""Exercise the composite action's real shell script with a local mock CLI.

Requires bash and jq (both supplied by GitHub-hosted runners). No network calls.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bash", default="bash")
    parser.add_argument("--jq-dir")
    args = parser.parse_args()
    if args.jq_dir:
        args.jq_dir = str(Path(args.jq_dir).resolve())
    script = Path(__file__).resolve().parents[1] / ".github/actions/zenv-action/validate.sh"
    with tempfile.TemporaryDirectory(prefix="zenv-action-") as tmp:
        root = Path(tmp)
        binary = root / "zenv"
        output = root / "outputs"
        captured = root / "arguments"
        env = os.environ.copy()
        env.update({
            "PATH": os.pathsep.join(filter(None, [str(root), args.jq_dir, env["PATH"]])),
            "GITHUB_OUTPUT": output.as_posix(),
            "ZENV_CAPTURE": captured.as_posix(),
            "ZENV_SCHEMA": 'schema $(touch injected) `touch backtick` "quoted".json',
            "ZENV_ENV_FILE": "--leading-dash file.env",
            "ZENV_ALLOW_MISSING_ENV": "true",
            "ZENV_DETECT_SECRETS": "false",
            "ZENV_NO_CACHE": "true",
            "ZENV_FORMAT": "json",
            "ZENV_VERIFY_HASH": "a" * 64,
            "ZENV_CA_CERT": "certificate with spaces.pem",
        })
        if not shutil.which("jq", path=env["PATH"]):
            raise SystemExit("jq is required; add it to PATH or use --jq-dir")

        def run(body):
            binary.write_text("#!/usr/bin/env bash\n" + body + "\n", encoding="utf-8", newline="\n")
            binary.chmod(0o755)
            output.write_text("", encoding="utf-8")
            result = subprocess.run([args.bash, script.as_posix()], cwd=root,
                                    env=env, capture_output=True, text=True)
            outputs = dict(line.split("=", 1) for line in output.read_text().splitlines())
            return result, outputs

        result, outputs = run('printf "%s\\n" "$@" > "$ZENV_CAPTURE"\nprintf \'{"valid":true,"errors":[]}\\n\'')
        assert result.returncode == 0, result.stderr
        assert outputs == {"valid": "true", "errors": "[]"}, outputs
        argv = captured.read_text().splitlines()
        for flag, key in [("schema", "SCHEMA"), ("env", "ENV_FILE"),
                          ("ca-cert", "CA_CERT"), ("verify-hash", "VERIFY_HASH"),
                          ("no-cache", "NO_CACHE"), ("detect-secrets", "DETECT_SECRETS")]:
            assert f"--{flag}={env['ZENV_' + key]}" in argv, argv
        assert not (root / "injected").exists()
        assert not (root / "backtick").exists()

        result, outputs = run('printf \'{"valid":false,"errors":[{"key":"PORT","message":"expected int"}]}\\n\'\nprintf "diagnostic\\n" >&2\nexit 1')
        assert result.returncode == 1, result
        assert outputs["valid"] == "false"
        assert json.loads(outputs["errors"]) == ["PORT: expected int"], outputs

        result, outputs = run('printf "schema file missing\\n" >&2\nexit 3')
        assert result.returncode == 1
        assert json.loads(outputs["errors"]) == ["schema file missing"], outputs

        env["ZENV_FORMAT"] = "text"
        result, outputs = run('printf "validation failed\\n  - PORT: expected int\\n"\nexit 1')
        assert result.returncode == 1
        assert "PORT: expected int" in "\n".join(json.loads(outputs["errors"]))
    print("Action checks passed: literal arguments, flags, JSON errors, input errors, text errors")


if __name__ == "__main__":
    main()
