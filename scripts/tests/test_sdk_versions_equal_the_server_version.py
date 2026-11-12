"""Both SDKs carry the server's version, everywhere they carry one, and a package ships the
repository's licence (scripts/check_sdk_versions.py; docs/contributing/releasing.md)."""
import io
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import check_sdk_versions as V  # noqa: E402
import sdk_names as N  # noqa: E402

REPO = V.DEFAULT_ROOT
FILES = [
    "Cargo.toml",
    "LICENSE",
    "sdks/python/pyproject.toml",
    "sdks/python/uv.lock",
    "sdks/python/LICENSE",
    "sdks/typescript/package.json",
    "sdks/typescript/package-lock.json",
    "sdks/typescript/src/client.ts",
    "sdks/typescript/LICENSE",
    "sdks/rust/Cargo.toml",
]


def copy_of_the_repository(tmp: str) -> Path:
    root = Path(tmp)
    for name in FILES:
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(REPO / name, root / name)
    return root


def edit(root: Path, name: str, old: str, new: str) -> None:
    path = root / name
    text = path.read_text()
    assert old in text, f"{name} does not contain {old!r}"
    path.write_text(text.replace(old, new, 1))


class SdkVersions(unittest.TestCase):
    def test_every_place_the_sdks_carry_a_version_is_the_workspace_version(self):
        self.assertEqual(V.disagreements(REPO), [])

    def test_the_tag_is_held_to_the_same_version(self):
        workspace = V.read_versions(REPO)["Cargo.toml [workspace.package] version"]
        self.assertEqual(V.disagreements(REPO, workspace), [])
        self.assertEqual(
            V.disagreements(REPO, "9.9.9"),
            [f"tag 9.9.9 is not the workspace version {workspace}"],
        )

    def test_a_server_bump_that_leaves_an_sdk_behind_names_each_place_it_left(self):
        workspace = V.read_versions(REPO)["Cargo.toml [workspace.package] version"]
        with tempfile.TemporaryDirectory() as tmp:
            root = copy_of_the_repository(tmp)
            edit(root, "Cargo.toml", f'version = "{workspace}"', 'version = "99.0.0"')
            problems = V.disagreements(root)
        self.assertEqual(len(problems), 6, problems)
        for place in (
            "sdks/python/pyproject.toml",
            "sdks/python/uv.lock",
            "sdks/typescript/package.json",
            "sdks/typescript/package-lock.json version",
            'sdks/typescript/package-lock.json packages[""]',
            "sdks/typescript/src/client.ts",
        ):
            self.assertTrue(any(p.startswith(place) for p in problems), (place, problems))

    def test_each_place_alone_is_caught_when_it_diverges(self):
        workspace = V.read_versions(REPO)["Cargo.toml [workspace.package] version"]
        cases = {
            "sdks/python/pyproject.toml": f'version = "{workspace}"',
            "sdks/python/uv.lock": f'name = "{N.python_name(REPO)}"\nversion = "{workspace}"',
            "sdks/typescript/package.json": f'"version": "{workspace}"',
            "sdks/typescript/package-lock.json": f'"version": "{workspace}"',
            "sdks/typescript/src/client.ts": f"oml-sdk-typescript/{workspace}",
        }
        for name, old in cases.items():
            with self.subTest(name), tempfile.TemporaryDirectory() as tmp:
                root = copy_of_the_repository(tmp)
                edit(root, name, old, old.replace(workspace, "0.0.1"))
                problems = V.disagreements(root)
            self.assertEqual(len(problems), 1, problems)
            self.assertTrue(problems[0].startswith(name), problems)

    def test_the_rust_sdk_follows_the_workspace_version_without_a_number_of_its_own(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = copy_of_the_repository(tmp)
            edit(root, "sdks/rust/Cargo.toml", "version.workspace = true", 'version = "0.2.0"')
            problems = V.disagreements(root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("not version.workspace = true", problems[0])

    def test_the_command_exits_non_zero_naming_the_bump_rule_on_a_divergence(self):
        err = io.StringIO()
        with tempfile.TemporaryDirectory() as tmp:
            root = copy_of_the_repository(tmp)
            manifest = json.loads((root / "sdks/typescript/package.json").read_text())
            edit(root, "sdks/typescript/package.json", manifest["version"], "0.0.1")
            with redirect_stderr(err):
                code = V.main(["--root", str(root)])
        self.assertEqual(code, 1)
        self.assertIn("sdks/typescript/package.json version is 0.0.1", err.getvalue())
        self.assertIn("bump every place in one change", err.getvalue())

    def test_the_sdk_scripts_run_under_python_isolated_mode(self):
        # -I leaves the script's own directory off the path, so a script that imports its
        # neighbour sdk_names has to add it itself.
        for script in ("check_sdk_versions.py", "check_sdk_registry_names.py", "build_sdk_packages.py", "sdk_names.py"):
            with self.subTest(script):
                done = subprocess.run(
                    [sys.executable, "-I", str(REPO / "scripts" / script), "--help"], capture_output=True, text=True
                )
                self.assertEqual(done.returncode, 0, done.stderr)

    def test_each_package_ships_the_repositorys_licence(self):
        for package in ("sdks/python", "sdks/typescript"):
            with self.subTest(package):
                self.assertEqual(
                    (REPO / package / "LICENSE").read_text(), (REPO / "LICENSE").read_text()
                )


if __name__ == "__main__":
    unittest.main()
