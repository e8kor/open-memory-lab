"""Put one visible line under every page's title and refuse a page that lacks its facts.

Every published page declares `status` and `verified` in its front matter. The line reads
"Applies to OML v0.6.x · Status: Stable · Last verified 2026-11-06"; the series comes from
`extra.oml_series` in mkdocs.yml, so a release changes it in one place.
"""

import re

from mkdocs.exceptions import PluginError

STATUSES = {"Stable", "Beta", "Measured", "Generated"}
H1 = re.compile(r"^# .+\n", re.M)


def on_page_markdown(markdown, page, config, files):
    meta = page.meta or {}
    status, verified = meta.get("status"), meta.get("verified")
    if status not in STATUSES or not verified:
        raise PluginError(
            f"{page.file.src_uri}: front matter needs status ({', '.join(sorted(STATUSES))}) and verified (a date)"
        )
    series = config.get("extra", {}).get("oml_series", "")
    line = (
        f'\n<p class="oml-status">Applies to OML v{series}.x · Status: {status} '
        f"· Last verified {verified}</p>\n"
    )
    match = H1.search(markdown)
    if not match:
        return line.lstrip() + markdown
    return markdown[: match.end()] + line + markdown[match.end() :]
