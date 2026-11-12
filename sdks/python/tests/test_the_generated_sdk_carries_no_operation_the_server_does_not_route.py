"""The generated layer offers exactly the operations openapi.yaml declares (ADR-0039 d9).

openapi.yaml declares only what the server routes, which the server's own test
holds against its route table, so an operation here that the document does not
declare would be a call answered with 404. Each generated module names its
method and path in `_get_kwargs`; the document's paths are read in its own
layout: a path item at two spaces, its operations at four.
"""

import re
from pathlib import Path

import oml_sdk.generated.api as generated_api

SPEC = Path(__file__).resolve().parents[3] / "docs" / "api" / "openapi.yaml"
METHOD_RE = re.compile(r"^    (get|put|post|delete|patch):$")
KWARGS_RE = re.compile(r'"method": "([a-z]+)",\s*"url": "([^"]+)"')


def declared_operations() -> set[tuple[str, str]]:
    operations: set[tuple[str, str]] = set()
    in_paths = False
    path = None
    for line in SPEC.read_text(encoding="utf-8").splitlines():
        if line and not line.startswith(" "):
            in_paths = line == "paths:"
            path = None
        elif in_paths and line.startswith("  /"):
            path = line.strip().removesuffix(":")
        elif in_paths and path and (method := METHOD_RE.match(line)):
            operations.add((method.group(1).upper(), path))
    return operations


def generated_operations() -> set[tuple[str, str]]:
    operations: set[tuple[str, str]] = set()
    for module in Path(generated_api.__file__).parent.rglob("*.py"):
        if module.name == "__init__.py":
            continue
        found = KWARGS_RE.findall(module.read_text(encoding="utf-8"))
        assert len(found) == 1, f"{module} names one operation"
        method, url = found[0]
        operations.add((method.upper(), url))
    return operations


def test_the_generated_sdk_carries_no_operation_the_server_does_not_route() -> None:
    declared = declared_operations()
    assert ("GET", "/v2/whoami") in declared
    assert len(declared) == 80
    assert generated_operations() == declared
