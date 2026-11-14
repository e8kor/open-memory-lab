"""Put one visible line under every page's title and refuse a page that lacks its facts.

Every published page declares `status` and `verified` in its front matter. The line reads
"Applies to OML v0.7.x · Status: Stable · Last verified 2026-11-06"; the series is the major and
minor of the workspace version in Cargo.toml, so the release that bumps that version changes it.
"""

import pathlib
import re
import tomllib

from mkdocs.exceptions import PluginError

STATUSES = {"Stable", "Beta", "Measured", "Generated"}
H1 = re.compile(r"^# .+\n", re.M)
CARGO = pathlib.Path(__file__).resolve().parent.parent / "Cargo.toml"


def series() -> str:
    version = tomllib.loads(CARGO.read_text())["workspace"]["package"]["version"]
    return ".".join(version.split(".")[:2])


SERIES = series()


def on_page_markdown(markdown, page, config, files):
    meta = page.meta or {}
    status, verified = meta.get("status"), meta.get("verified")
    if status not in STATUSES or not verified:
        raise PluginError(
            f"{page.file.src_uri}: front matter needs status ({', '.join(sorted(STATUSES))}) and verified (a date)"
        )
    line = (
        f'\n<p class="oml-status">Applies to OML v{SERIES}.x · Status: {status} '
        f"· Last verified {verified}</p>\n"
    )
    match = H1.search(markdown)
    if not match:
        return line.lstrip() + markdown
    return markdown[: match.end()] + line + markdown[match.end() :]
