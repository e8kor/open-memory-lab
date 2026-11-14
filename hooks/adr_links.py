"""Point every relative link to a design record at the record on GitHub.

The records in docs/adr/ are excluded from the site (mkdocs.yml `exclude_docs`), so a relative link
to one renders to a page that does not exist. This hook rewrites each inline or reference link whose
target resolves to `adr/ADR-*.md` to the blob URL the design records page uses
(`scripts/gen_design_records.py` BLOB), keeping its fragment. Other links are left as written.

GitHub does not redirect a renamed file, so a link to a record that does not exist fails the strict
build. When the target is a former file name in the register's "Renamed records" list
(docs/adr/index.md), the failure names the current one (ADR-0091 d10).
"""

import logging
import posixpath
import re
from pathlib import Path

# Read, not imported: the generator's f-strings need Python 3.12, and the script tests run on 3.11.
_GEN = (Path(__file__).resolve().parent.parent / "scripts" / "gen_design_records.py").read_text()
BLOB = re.search(r'^BLOB = "([^"]+)"$', _GEN, re.M).group(1)

INLINE = re.compile(r"(\]\()([^)\s]+)(\))")
REFERENCE = re.compile(r"^(\s*\[[^\]]+\]:\s*)(\S+)", re.M)
RECORD = re.compile(r"adr/(ADR-\d{4}[^/]*\.md)")
ADR_DIR = Path(__file__).resolve().parent.parent / "docs" / "adr"
RENAMED_ROW = re.compile(r"^\|\s*`(ADR-\d{4}[^`]*\.md)`\s*\|\s*\[`(ADR-\d{4}[^`]*\.md)`\]", re.M)

log = logging.getLogger("mkdocs.plugins.adr_links")


def renamed() -> dict[str, str]:
    """Former file name to current file name, from the register's "Renamed records" list."""
    text = (ADR_DIR / "index.md").read_text()
    section = text.partition("## Renamed records")[2].partition("\n## ")[0]
    return dict(RENAMED_ROW.findall(section))


def missing(name: str) -> str | None:
    """Why a link to the record `name` is broken, or None when the file exists."""
    if (ADR_DIR / name).is_file():
        return None
    current = renamed().get(name)
    if current:
        return f"{name} was renamed to {current} (docs/adr/index.md, Renamed records)"
    return f"{name} does not exist in docs/adr/"


def to_blob(target: str, page: str) -> str:
    path, hashmark, fragment = target.partition("#")
    if not path or "://" in path or path.startswith(("/", "mailto:")):
        return target
    m = RECORD.fullmatch(posixpath.normpath(posixpath.join(posixpath.dirname(page), path)))
    if not m:
        return target
    why = missing(m.group(1))
    if why:
        log.warning("%s links to a design record that is not there: %s", page, why)
    return f"{BLOB}{m.group(1)}{hashmark}{fragment}"


def rewrite(markdown: str, page: str) -> str:
    """`page` is the page's path inside docs/, such as architecture/overview.md."""
    markdown = INLINE.sub(lambda m: m.group(1) + to_blob(m.group(2), page) + m.group(3), markdown)
    return REFERENCE.sub(lambda m: m.group(1) + to_blob(m.group(2), page), markdown)


def on_page_markdown(markdown, page, config, files):
    return rewrite(markdown, page.file.src_uri)
