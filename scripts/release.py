#!/usr/bin/env python3
"""Prepare a version bump, or check release metadata without publishing anything.

Requires Python 3.11+. All replacement targets are checked before writing.
"""
import argparse
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[1]
SEMVER = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(0|[1-9][0-9]*|[0-9]*[a-zA-Z-][0-9a-zA-Z-]*)(\.(0|[1-9][0-9]*|[0-9]*[a-zA-Z-][0-9a-zA-Z-]*))*)?(\+[0-9a-zA-Z-]+(\.[0-9a-zA-Z-]+)*)?")
ACTION = Path(".github/actions/zenv-action/action.yml")
FALLBACK = r'(?m)^(\s*VERSION=")[0-9][^"\r\n]*("\s*)$'


def replace_once(content, pattern, version):
    result, count = re.subn(pattern, lambda m: m[1] + version + m[2], content)
    if count != 1:
        raise ValueError(f"expected exactly one version field, found {count}")
    return result


def versions(root):
    manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    package = manifest["package"]
    locked = [p["version"] for p in lock["package"] if p["name"] == package["name"] and "source" not in p]
    fallback = re.findall(r'(?m)^\s*VERSION="([0-9][^"\r\n]*)"\s*$', (root / ACTION).read_text(encoding="utf-8"))
    if locked != [package["version"]] or fallback != [package["version"]]:
        raise ValueError("Cargo.toml, Cargo.lock, and Action fallback versions differ")
    if not SEMVER.fullmatch(package["version"]):
        raise ValueError("invalid package version")
    return package["version"]


def prepare(root, version):
    if not SEMVER.fullmatch(version):
        raise ValueError("version must be valid SemVer (without a v prefix)")
    versions(root)
    files = {
        Path("Cargo.toml"): r'(?m)^(version = ")[^"\r\n]*(")$',
        Path("Cargo.lock"): r'(?m)(^name = "zorath-env"\nversion = ")[^"\r\n]*(")$',
        ACTION: FALLBACK,
    }
    updates = {path: replace_once((root / path).read_text(encoding="utf-8"), pattern, version)
               for path, pattern in files.items()}
    for path, content in updates.items():
        (root / path).write_text(content, encoding="utf-8", newline="\n")
    versions(root)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", nargs="?")
    parser.add_argument("description", nargs="?")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--tag")
    args = parser.parse_args()
    try:
        if args.check:
            version = versions(ROOT)
            if args.tag and args.tag != "v" + version:
                raise ValueError(f"tag must match Cargo.toml: v{version}")
            print(f"Release metadata agrees: v{version}")
        elif args.version:
            prepare(ROOT, args.version)
            print(f"Prepared v{args.version}. Update CHANGELOG.md, review the diff, and run CI before tagging.")
        else:
            parser.error("provide a version, or --check")
    except (ValueError, OSError, KeyError) as error:
        parser.exit(1, f"Release metadata error: {error}\n")


if __name__ == "__main__":
    main()
