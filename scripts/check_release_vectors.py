#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "rfc8785==0.1.4",
# ]
# ///
"""The vocabulary release vectors reproduce outside Rust, byte for byte.

Run by .github/workflows/m0-spikes.yml (job release-vectors) as

    uv run --locked --script scripts/check_release_vectors.py

docs/engineering/release-vectors.md declares the vectors: each release vector's input
(term tips and edges, deliberately not in canonical order), its canonical document and its
SHA-256 digest, and each diff vector's two releases, its canonical diff and digest. This
script reads every one of them from the page and rebuilds each from its input with code
of its own, not a port of crates/oml-domain/src/vocab_release.rs:

- the release keeps the oml:is_a and oml:replaced_by edges once each, drops every other
  label, and materializes the is_a closure at each pair's fewest edges (all-pairs
  shortest paths here, where the Rust code walks breadth first);
- a consider tip's successors are sorted and kept once each;
- the document and the diff are canonicalized by rfc8785 (RFC 8785) and hashed by
  hashlib (SHA-256), and the bytes, the byte count and the digest must equal the page's;
- the page's canonical bytes must parse back to the value rebuilt, so a disagreement is
  named by member, not only by digest.

It fails with every disagreement found. The Rust test
crates/oml-domain/tests/release_vectors.rs reads the same page and reproduces the same
bytes from build_release and diff_releases.

Usage: check_release_vectors.py [--page PATH]
       (default: docs/engineering/release-vectors.md)
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

try:
    import rfc8785
except ImportError as missing:  # run without uv
    sys.exit(
        f"check_release_vectors: {missing.name} is not importable; run "
        "`uv run --locked --script scripts/check_release_vectors.py`"
    )

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_PAGE = ROOT / "docs/engineering/release-vectors.md"

IS_A = "oml:is_a"
REPLACED_BY = "oml:replaced_by"
RELEASED_LABELS = {IS_A, REPLACED_BY}

# A marker names the fenced block that follows it: <!-- vector: R2 input -->.
BLOCK = re.compile(
    r"<!-- vector: (\S+) (input|canonical|digest) -->\s*```[a-z]*\n(.*?)\n```",
    re.DOTALL,
)
DIGEST = re.compile(r"\Abytes  = (\d+)\nsha256 = sha256:([0-9a-f]{64})\Z")


class PageError(Exception):
    pass


def read_page(page: Path) -> dict[str, dict[str, str]]:
    """Every vector's blocks, by vector id then block name; each vector has all three."""
    vectors: dict[str, dict[str, str]] = {}
    for vector, block, body in BLOCK.findall(page.read_text(encoding="utf-8")):
        if block in vectors.setdefault(vector, {}):
            raise PageError(f"vector {vector} has two {block} blocks")
        vectors[vector][block] = body
    if not vectors:
        raise PageError("no <!-- vector: … --> blocks found")
    for vector, blocks in vectors.items():
        missing = {"input", "canonical", "digest"} - set(blocks)
        if missing:
            raise PageError(
                f"vector {vector} lacks its {', '.join(sorted(missing))} block"
            )
    return vectors


def successors(tip: dict) -> list[dict]:
    if tip["term_status"] not in {"active", "deprecated"}:
        raise PageError(f"{tip['curie']} has term_status {tip['term_status']!r}")
    if tip["term_status"] == "active":
        if "successors" in tip:
            raise PageError(f"{tip['curie']} is active and names successors")
        return []
    relation = tip["successors"]["relation"]
    curies = tip["successors"]["curies"]
    if relation not in {"replaced_by", "consider"} or not curies:
        raise PageError(f"{tip['curie']} has successors {tip['successors']!r}")
    if relation == "replaced_by" and len(curies) != 1:
        raise PageError(f"{tip['curie']} is replaced_by {len(curies)} successors")
    return [{"curie": c, "relation": relation} for c in sorted(set(curies))]


def closure(edges: list[tuple[str, str, str]]) -> list[dict]:
    """The is_a closure by Floyd-Warshall: one row per (ancestor, descendant), at its
    fewest edges, refusing a cycle."""
    up = {(f, t) for label, f, t in edges if label == IS_A}
    nodes = sorted({n for pair in up for n in pair})
    infinity = len(nodes) + 1
    depth = {(a, b): (1 if (a, b) in up else infinity) for a in nodes for b in nodes}
    for k in nodes:
        for a in nodes:
            for b in nodes:
                depth[a, b] = min(depth[a, b], depth[a, k] + depth[k, b])
    if any(depth[n, n] < infinity for n in nodes):
        raise PageError("the is_a edges close a cycle")
    rows = [
        {"ancestor": ancestor, "depth": d, "descendant": descendant}
        for (descendant, ancestor), d in depth.items()
        if d < infinity
    ]
    return sorted(rows, key=lambda r: (r["ancestor"], r["descendant"], r["depth"]))


def release(spec: dict) -> dict:
    """The release document a vector's input materializes."""
    curies = [t["curie"] for t in spec["tips"]]
    if len(curies) != len(set(curies)):
        raise PageError("two tips name one CURIE")
    terms = sorted(
        (
            {
                "curie": t["curie"],
                "successors": successors(t),
                "term_status": t["term_status"],
            }
            for t in spec["tips"]
        ),
        key=lambda t: t["curie"],
    )
    edges = sorted(
        {
            (e["label"], e["from"], e["to"])
            for e in spec["edges"]
            if e["label"] in RELEASED_LABELS
        }
    )
    replaced = {
        (t["curie"], s["curie"])
        for t in terms
        for s in t["successors"]
        if s["relation"] == "replaced_by"
    }
    if replaced != {(f, t) for label, f, t in edges if label == REPLACED_BY}:
        raise PageError("the replaced_by tips and the replaced_by edges disagree")
    return {
        "closure": closure(edges),
        "edges": [{"from": f, "label": label, "to": t} for label, f, t in edges],
        "terms": terms,
    }


def edge_key(edge: dict) -> tuple:
    return edge["label"], edge["from"], edge["to"]


def row_key(row: dict) -> tuple:
    return row["ancestor"], row["descendant"], row["depth"]


def minus(these: list[dict], those: list[dict], key) -> list[dict]:
    gone = {key(x) for x in those}
    return sorted((x for x in these if key(x) not in gone), key=key)


def diff(previous: dict, next_: dict) -> dict:
    """The diff from previous to next, refusing a term dropped, reinstated or deprecated
    again with other successors."""
    before = {t["curie"]: t for t in previous["terms"]}
    after = {t["curie"]: t for t in next_["terms"]}
    for curie, old in before.items():
        new = after.get(curie)
        if new is None:
            raise PageError(f"{curie} is dropped")
        if old["term_status"] == "deprecated" and new != old:
            raise PageError(f"{curie} is reinstated or deprecated again")
    return {
        "closure_added": minus(next_["closure"], previous["closure"], row_key),
        "closure_removed": minus(previous["closure"], next_["closure"], row_key),
        "deprecations": [
            {"curie": c, "successors": t["successors"]}
            for c, t in sorted(after.items())
            if c in before
            and before[c]["term_status"] == "active"
            and t["term_status"] == "deprecated"
        ],
        "edges_added": minus(next_["edges"], previous["edges"], edge_key),
        "edges_removed": minus(previous["edges"], next_["edges"], edge_key),
        "terms_added": [t for c, t in sorted(after.items()) if c not in before],
    }


def disagreements(path: str, want, got) -> list[str]:
    """Where two JSON values differ, by member path."""
    if isinstance(want, dict) and isinstance(got, dict):
        out = []
        for key in sorted(set(want) | set(got)):
            if key not in got:
                out.append(f"{path}.{key}: on the page, not rebuilt")
            elif key not in want:
                out.append(f"{path}.{key}: rebuilt, not on the page")
            else:
                out += disagreements(f"{path}.{key}", want[key], got[key])
        return out
    if isinstance(want, list) and isinstance(got, list) and len(want) == len(got):
        return [
            d
            for i, (w, g) in enumerate(zip(want, got))
            for d in disagreements(f"{path}[{i}]", w, g)
        ]
    return (
        []
        if want == got
        else [f"{path}: page {json.dumps(got)}, rebuilt {json.dumps(want)}"]
    )


def main(argv: list[str]) -> int:
    options = dict(zip(argv[::2], argv[1::2]))
    if len(argv) % 2 or set(options) - {"--page"}:
        print(__doc__[__doc__.index("Usage:") :].strip(), file=sys.stderr)
        return 2
    page_path = Path(options.get("--page", DEFAULT_PAGE))
    try:
        vectors = read_page(page_path)
    except (OSError, PageError) as e:
        print(f"check_release_vectors: FAIL: {page_path}: {e}", file=sys.stderr)
        return 1

    errors: list[str] = []
    documents: dict[str, dict] = {}
    sizes: list[str] = []
    # Release vectors first: a diff vector names two of them.
    for vector in sorted(vectors, key=lambda v: (not v.startswith("R"), v)):
        blocks = vectors[vector]
        try:
            spec = json.loads(blocks["input"])
            if vector.startswith("R"):
                value = release(spec)
                documents[vector] = value
            else:
                named = [spec["previous"], spec["next"]]
                if not set(named) <= set(documents):
                    raise PageError(
                        f"names {' and '.join(named)}, and not both rebuilt"
                    )
                value = diff(*(documents[n] for n in named))
        except (KeyError, ValueError, PageError) as e:
            errors.append(
                f"vector {vector}: its input does not rebuild: {type(e).__name__}: {e}"
            )
            continue
        rebuilt = rfc8785.dumps(value)
        stated = blocks["canonical"].encode("utf-8")
        stated_digest = DIGEST.match(blocks["digest"])
        if stated_digest is None:
            errors.append(
                f"vector {vector}: the digest block is not `bytes  = N` / `sha256 = sha256:<hex>`"
            )
            continue
        count, digest = int(stated_digest[1]), stated_digest[2]
        if rebuilt != stated:
            errors.append(f"vector {vector}: rfc8785's bytes differ from the page's")
            try:
                errors += [
                    f"vector {vector}: {d}"
                    for d in disagreements("$", value, json.loads(stated))
                ]
            except ValueError:
                errors.append(
                    f"vector {vector}: the page's canonical block is not JSON"
                )
        if len(stated) != count:
            errors.append(
                f"vector {vector}: the page states {count} bytes and its block holds {len(stated)}"
            )
        if hashlib.sha256(rebuilt).hexdigest() != digest:
            errors.append(
                f"vector {vector}: SHA-256 of the rebuilt bytes is not the stated sha256:{digest}"
            )
        if hashlib.sha256(stated).hexdigest() != digest:
            errors.append(
                f"vector {vector}: SHA-256 of the page's bytes is not the stated sha256:{digest}"
            )
        sizes.append(f"{vector} {len(rebuilt)}")

    if errors:
        for error in errors:
            print(f"check_release_vectors: FAIL: {error}", file=sys.stderr)
        return 1
    print(
        f"check_release_vectors: OK: {page_path.relative_to(ROOT) if page_path.is_relative_to(ROOT) else page_path}: "
        f"{len(sizes)} vectors reproduce byte for byte ({', '.join(sizes)} bytes)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
