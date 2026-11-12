#!/usr/bin/env python3
"""Build the two SDK packages as they would be published, check what they ship, and install them.

    python scripts/build_sdk_packages.py --out dist-sdks

writes dist-sdks/python/ (a wheel and an sdist) and dist-sdks/typescript/ (an npm tarball), named
by scripts/sdk_names.py from the manifests, at the workspace version, then for each
package:

- reads the published metadata back out of the built file and fails on a name, version,
  licence, readme, repository URL or file list that is not the one a registry page should show;
- lists the files it ships and fails on a test, a fixture, a lock file, a key or an environment file;
- installs the built file into a fresh virtual environment or a fresh node project, away from the
  checkout, and runs one offline smoke: import it, build a client, send one request through a
  stand-in transport and check what went out.

Nothing is uploaded. Publishing is release.yml's `publish-sdk-pypi` and `publish-sdk-npm`
jobs, which stay off until the repository variable PUBLISH_SDKS is `true`
(docs/contributing/releasing.md, "Publishing the SDKs"). The build and these checks run on
every release and, through ci.yml job sdk-packages, on every pull request that touches sdks/.
Needs uv and npm, and the network for the build backend and for openapi-fetch.
"""

from __future__ import annotations

import argparse
import email
import json
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import sdk_names  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
PY = ROOT / "sdks" / "python"
TS = ROOT / "sdks" / "typescript"
REPO_URL = "https://github.com/e8kor/open-memory-lab"

# A file a published package must never carry, by path.
FORBIDDEN = re.compile(
    r"(^|/)(tests?|fixtures?|node_modules|\.venv|__pycache__|\.pytest_cache|\.git)(/|$)"
    r"|(^|/)(uv\.lock|package-lock\.json|\.env[^/]*|\.npmrc|\.pypirc|\.netrc)$"
    r"|\.(pem|key|p12|pfx|log)$|(^|/)(credentials|secrets?)(\.[a-z]+)?$",
    re.IGNORECASE,
)

errors: list[str] = []


def fail(message: str) -> None:
    errors.append(message)
    print(f"FAIL {message}", file=sys.stderr)


def run(cmd: list[str], cwd: Path, **kw) -> subprocess.CompletedProcess:
    print("+", " ".join(cmd), f"(in {cwd})", flush=True)
    return subprocess.run(cmd, cwd=cwd, check=True, text=True, **kw)


def workspace_version() -> str:
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    return cargo["workspace"]["package"]["version"]


def forbidden(names: list[str], what: str) -> None:
    for name in names:
        if FORBIDDEN.search(name):
            fail(f"{what} ships {name}, which a published package must not carry")


def metadata_checks(meta: email.message.Message, version: str, what: str) -> None:
    if meta["Name"] != sdk_names.python_name():
        fail(f"{what}: Name is {meta['Name']!r}, the manifest says {sdk_names.python_name()!r}")
    if meta["Version"] != version:
        fail(f"{what}: Version is {meta['Version']!r}, the workspace is {version!r}")
    if meta["License-Expression"] != "Apache-2.0":
        fail(f"{what}: License-Expression is {meta['License-Expression']!r}, not 'Apache-2.0'")
    # The README's heading up to its first non-ASCII character: the email parser hands the body back decoded as latin-1.
    heading = re.match(r"[ -~]+", (PY / "README.md").read_text(encoding="utf-8").splitlines()[0]).group(0).strip()
    if not (meta.get_payload() or "").strip().startswith(heading):
        fail(f"{what}: the long description is not the README")
    if meta["Description-Content-Type"] != "text/markdown":
        fail(f"{what}: Description-Content-Type is {meta['Description-Content-Type']!r}")
    urls = {u.split(", ", 1)[0]: u.split(", ", 1)[1] for u in meta.get_all("Project-URL") or []}
    if urls.get("Repository") != REPO_URL:
        fail(f"{what}: Project-URL Repository is {urls.get('Repository')!r}, not {REPO_URL}")
    if not meta["Requires-Python"]:
        fail(f"{what}: no Requires-Python")
    if re.search(r"\.\./\.\./", meta.get_payload() or ""):
        fail(f"{what}: the README links a repository-relative path, which no registry page resolves")


def check_licence(text: str, what: str) -> None:
    if text != (ROOT / "LICENSE").read_text():
        fail(f"{what}: the shipped LICENSE is not the repository's")


def smoke_python(artifact: Path, label: str, scratch: Path, version: str) -> None:
    venv = scratch / f"venv-{label}"
    run(["uv", "venv", "--quiet", "--python", sys.executable, str(venv)], scratch)
    py = str(venv / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python"))
    run(["uv", "pip", "install", "--quiet", "--python", py, str(artifact)], scratch)
    program = f"""
import json, pathlib, httpx, oml_sdk
from oml_sdk import OmlClient

assert "site-packages" in oml_sdk.__file__, oml_sdk.__file__
seen = []
def handler(request):
    seen.append(request)
    return httpx.Response(200, json={{"items": [], "next_cursor": None}})
with OmlClient(url="http://oml.invalid", api_key="smoke-key", transport=httpx.MockTransport(handler)) as oml:
    assert "smoke-key" not in repr(oml)
    oml.read(limit=1)
assert len(seen) == 1, seen
assert seen[0].headers["user-agent"] == "oml-sdk-python/{version}", seen[0].headers["user-agent"]
assert seen[0].headers["authorization"] == "Bearer smoke-key"
print("python smoke ok:", oml_sdk.__file__)
"""
    run([py, "-I", "-c", program], scratch)


def python_package(out: Path, scratch: Path, version: str) -> None:
    out.mkdir(parents=True)
    run(["uv", "build", "--quiet", "--out-dir", str(out)], PY)
    (out / ".gitignore").unlink(missing_ok=True)  # uv writes one beside what it builds
    wheels = sorted(out.glob("*.whl"))
    sdists = sorted(out.glob("*.tar.gz"))
    if len(wheels) != 1 or len(sdists) != 1:
        fail(f"python: expected one wheel and one sdist, found {[p.name for p in out.iterdir()]}")
        return
    wheel, sdist = wheels[0], sdists[0]
    want_wheel, want_sdist = sdk_names.wheel_filename(ROOT, version), sdk_names.sdist_filename(ROOT, version)
    if wheel.name != want_wheel or sdist.name != want_sdist:
        fail(f"python: built {wheel.name} and {sdist.name}, not {want_wheel} and {want_sdist}")

    with zipfile.ZipFile(wheel) as z:
        names = z.namelist()
        forbidden(names, wheel.name)
        stem = sdk_names.python_stem(sdk_names.python_name())
        info = f"{stem}-{version}.dist-info"
        if f"{info}/METADATA" in names:
            metadata_checks(email.message_from_bytes(z.read(f"{info}/METADATA")), version, wheel.name)
        else:
            fail(f"{wheel.name}: no {info}/METADATA")
        licences = [n for n in names if n.startswith(f"{info}/licenses/")]
        if not licences:
            fail(f"{wheel.name}: no licence file under {info}/licenses/")
        for name in licences:
            check_licence(z.read(name).decode(), f"{wheel.name} {name}")
        if not any(n.startswith("oml_sdk/generated/") for n in names) or "oml_sdk/client.py" not in names:
            fail(f"{wheel.name}: the generated layer or the wrapper is missing")
        if [n for n in names if not (n.startswith("oml_sdk/") or n.startswith(info + "/"))]:
            fail(f"{wheel.name}: files outside oml_sdk/ and {info}/")

    with tarfile.open(sdist) as t:
        members = {m.name: m for m in t.getmembers() if m.isfile()}
        stem = f"{sdk_names.python_stem(sdk_names.python_name())}-{version}"
        forbidden(list(members), sdist.name)
        for needed in ("PKG-INFO", "pyproject.toml", "README.md", "LICENSE"):
            if f"{stem}/{needed}" not in members:
                fail(f"{sdist.name}: no {needed}")
        if f"{stem}/PKG-INFO" in members:
            meta = email.message_from_bytes(t.extractfile(members[f"{stem}/PKG-INFO"]).read())
            metadata_checks(meta, version, sdist.name)
        if f"{stem}/LICENSE" in members:
            check_licence(t.extractfile(members[f"{stem}/LICENSE"]).read().decode(), f"{sdist.name} LICENSE")

    smoke_python(wheel, "wheel", scratch, version)
    smoke_python(sdist, "sdist", scratch, version)


def typescript_package(out: Path, scratch: Path, version: str) -> None:
    out.mkdir(parents=True)
    run(["npm", "ci", "--no-audit", "--no-fund", "--fetch-retries", "5"], TS)
    run(["npm", "run", "build"], TS)
    packed = run(
        ["npm", "pack", "--json", "--pack-destination", str(out)], TS, stdout=subprocess.PIPE
    )
    result = json.loads(packed.stdout)[0]
    tgz = out / result["filename"]
    if tgz.name != sdk_names.tgz_filename(ROOT, version):
        fail(f"typescript: packed {tgz.name}, not {sdk_names.tgz_filename(ROOT, version)}")
    names = [f["path"] for f in result["files"]]
    forbidden(names, tgz.name)
    for needed in ("package.json", "README.md", "LICENSE", "dist/index.js", "dist/index.d.ts"):
        if needed not in names:
            fail(f"{tgz.name}: no {needed}")
    if [n for n in names if not (n in {"package.json", "README.md", "LICENSE"} or n.startswith("dist/"))]:
        fail(f"{tgz.name}: files outside dist/ and the three npm always ships")

    with tarfile.open(tgz) as t:
        manifest = json.loads(t.extractfile("package/package.json").read())
        licence = t.extractfile("package/LICENSE").read().decode()
        readme = t.extractfile("package/README.md").read().decode()
    check_licence(licence, f"{tgz.name} LICENSE")
    if "../../" in readme:
        fail(f"{tgz.name}: the README links a repository-relative path, which no registry page resolves")
    expected = {
        "name": sdk_names.npm_name(),
        "version": version,
        "license": "Apache-2.0",
    }
    for key, want in expected.items():
        if manifest.get(key) != want:
            fail(f"{tgz.name}: package.json {key} is {manifest.get(key)!r}, not {want!r}")
    repository = manifest.get("repository") or {}
    if repository.get("url") != f"git+{REPO_URL}.git" or repository.get("directory") != "sdks/typescript":
        fail(f"{tgz.name}: package.json repository is {repository!r}; npm provenance needs the GitHub URL")
    if (manifest.get("publishConfig") or {}).get("access") != "public":
        fail(f"{tgz.name}: package.json publishConfig.access is not 'public'")

    project = scratch / "node-project"
    project.mkdir()
    (project / "package.json").write_text('{"name": "smoke", "private": true, "type": "module"}\n')
    run(["npm", "install", "--no-audit", "--no-fund", "--fetch-retries", "5", str(tgz)], project)
    program = f"""
import {{ OmlClient, USER_AGENT }} from "{sdk_names.npm_name()}";
import {{ realpathSync }} from "node:fs";
const seen = [];
const fetcher = async (request) => {{
  seen.push(request);
  return new Response(JSON.stringify({{ items: [], next_cursor: null }}), {{
    status: 200, headers: {{ "content-type": "application/json" }},
  }});
}};
const oml = new OmlClient({{ url: "http://oml.invalid", apiKey: "smoke-key", fetch: fetcher }});
await oml.read({{ limit: 1 }});
if (seen.length !== 1) throw new Error("expected one request, saw " + seen.length);
if (seen[0].headers.get("user-agent") !== "oml-sdk-typescript/{version}") throw new Error("user-agent " + seen[0].headers.get("user-agent"));
if (USER_AGENT !== "oml-sdk-typescript/{version}") throw new Error("USER_AGENT " + USER_AGENT);
if (seen[0].headers.get("authorization") !== "Bearer smoke-key") throw new Error("authorization");
const where = realpathSync(new URL(import.meta.resolve("{sdk_names.npm_name()}")).pathname);
if (!where.includes("node_modules")) throw new Error("not imported from node_modules: " + where);
console.log("typescript smoke ok:", where);
"""
    (project / "smoke.mjs").write_text(program)
    run(["node", "smoke.mjs"], project)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--out", type=Path, required=True, help="directory to write python/ and typescript/ into")
    args = parser.parse_args()
    out = args.out.resolve()
    if out.exists():
        print(f"{out} exists; name a new directory", file=sys.stderr)
        return 2
    version = workspace_version()
    with tempfile.TemporaryDirectory(prefix="oml-sdk-smoke-") as tmp:
        scratch = Path(tmp)
        python_package(out / "python", scratch, version)
        typescript_package(out / "typescript", scratch, version)
    if errors:
        print(f"\n{len(errors)} problem(s); nothing was uploaded", file=sys.stderr)
        shutil.rmtree(out, ignore_errors=True)
        return 1
    print(f"\nbuilt and checked at {version}:")
    for path in sorted(out.rglob("*")):
        if path.is_file():
            print(f"  {path.relative_to(out)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
