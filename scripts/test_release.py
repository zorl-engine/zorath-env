"""Run release-helper regressions in disposable copies; never bump this checkout."""
import importlib.util
from pathlib import Path
import shutil
import tempfile
import sys

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("release", ROOT / "scripts/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

with tempfile.TemporaryDirectory(prefix="zenv-release-test-") as folder:
    root = Path(folder)
    for path in [Path("Cargo.toml"), Path("Cargo.lock"), release.ACTION]:
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / path, root / path)
    action_before = (root / release.ACTION).read_text()
    release.prepare(root, "1.2.3-alpha-beta.1+build.001")
    assert release.versions(root) == "1.2.3-alpha-beta.1+build.001"
    action_after = (root / release.ACTION).read_text()
    assert action_before.count('VERSION="$ZENV_VERSION"') == action_after.count('VERSION="$ZENV_VERSION"') == 2
    assert '|| VERSION=""' in action_after
    assert 'VERSION=$(curl ' in action_after
    manifest_before = (root / "Cargo.toml").read_bytes()
    for invalid in ["01.2.3", "1.2.3-01", "1.2.3$(touch injected)"]:
        try:
            release.prepare(root, invalid)
        except ValueError:
            pass
        else:
            raise AssertionError(f"accepted {invalid}")
        assert (root / "Cargo.toml").read_bytes() == manifest_before
    (root / release.ACTION).write_text(action_after.replace('VERSION="1.2.3-alpha-beta.1+build.001"', 'VERSION="9.9.9"'))
    try:
        release.prepare(root, "2.0.0")
    except ValueError:
        pass
    else:
        raise AssertionError("accepted inconsistent release metadata")
    assert (root / "Cargo.toml").read_bytes() == manifest_before
print("Release helper checks passed: exact updates, valid SemVer, drift detection, no mutation on invalid input")
