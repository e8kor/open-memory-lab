"""The Memories list and the Atlas in a real Chromium, driven by Playwright
(ADR-0045 d22, d25, d27, d32).

Run by the Rust test `the_atlas_treemap_draws_and_zooms_with_no_script_in_chromium`,
which serves the pages on a loopback port and passes it here. The browser
believes it is on the issuer's https origin: every request to the issuer is
answered by the loopback server, so the policy the server sends is the one
the browser enforces. Any request elsewhere is aborted and reported.

Prints one JSON object on stdout; `did_not_run` names why when no browser
could start.
"""

import argparse
import json
import sys

try:
    from playwright.sync_api import sync_playwright
except ImportError as e:  # the Rust test reports it by name
    print(json.dumps({"did_not_run": f"playwright is not importable: {e}"}))
    sys.exit(0)


def cached_chromium() -> str | None:
    """A Chromium from the Playwright cache, the headless shell first."""
    import os
    import pathlib

    cache = pathlib.Path(os.environ.get("HOME", "/")) / ".cache/ms-playwright"
    found = sorted(
        cache.glob("chromium_headless_shell-*/chrome-headless-shell-linux64/chrome-headless-shell")
    )
    found += sorted(cache.glob("chromium-*/chrome-linux64/chrome"))
    return str(found[-1]) if found else None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", required=True)
    ap.add_argument("--issuer", required=True)
    ap.add_argument("--cookie", required=True)
    ap.add_argument("--admin-cookie", required=True)
    ap.add_argument("--shots", default="")
    a = ap.parse_args()
    local = f"http://127.0.0.1:{a.port}"
    issuer = a.issuer.rstrip("/")
    requests: list[str] = []
    foreign: list[str] = []
    console: list[str] = []
    out: dict = {}

    def route(r):
        url = r.request.url
        requests.append(url)
        if not url.startswith(issuer + "/"):
            foreign.append(url)
            r.abort()
            return
        response = r.fetch(url=local + url[len(issuer):], headers=r.request.all_headers())
        r.fulfill(response=response)

    with sync_playwright() as p:
        try:
            browser = p.chromium.launch()
        except Exception as first:  # noqa: BLE001 - reported, not hidden
            found = cached_chromium()
            if found is None:
                print(json.dumps({"did_not_run": f"Chromium would not start: {first}"}))
                return 0
            browser = p.chromium.launch(executable_path=found)
        for who, cookie in (("person", a.cookie), ("admin", a.admin_cookie)):
            base = "/web" if who == "person" else "/web/admin"
            for theme in ("dark", "light"):
                for width in (1280, 375):
                    ctx = browser.new_context(
                        viewport={"width": width, "height": 900}, color_scheme=theme
                    )
                    ctx.add_cookies(
                        [
                            {
                                "name": "__Host-oml_session",
                                "value": cookie,
                                "url": issuer + "/",
                                "secure": True,
                                "httpOnly": True,
                                "sameSite": "Lax",
                            }
                        ]
                    )
                    ctx.route("**/*", route)
                    page = ctx.new_page()
                    page.on("console", lambda m: console.append(f"{m.type}: {m.text}"))
                    page.on("pageerror", lambda e: console.append(f"pageerror: {e}"))
                    tag = f"{who}-{theme}-{width}"

                    def shot(name: str) -> None:
                        if a.shots:
                            page.screenshot(path=f"{a.shots}/{tag}-{name}.png", full_page=True)

                    page.goto(f"{issuer}{base}?view=memories")
                    out[f"{tag}_cards"] = page.locator("article.card.memory").count()
                    # The provenance is folded by default; one card opened for the picture.
                    out[f"{tag}_folded"] = page.locator("details.more[open]").count() == 0
                    shot("memories")
                    details = page.locator("details.more summary").first
                    if details.count():
                        details.click()
                        shot("memories-details")

                    page.goto(f"{issuer}{base}?view=memories&display=atlas")
                    svg = "svg.tm.narrow" if width < 600 else "svg.tm.wide"
                    out[f"{tag}_regions"] = page.locator(f"{svg} a").count()
                    out[f"{tag}_rows"] = page.locator("#atlas-table tbody tr").count()
                    out[f"{tag}_scripts"] = page.locator("script").count()
                    out[f"{tag}_hscroll"] = page.evaluate(
                        "document.documentElement.scrollWidth > window.innerWidth + 1"
                    )
                    shot("atlas")
                    first = page.locator(f"{svg} a").first
                    if first.count():
                        first.click()
                        page.wait_for_load_state()
                        out[f"{tag}_zoomed_url"] = page.url
                        shot("atlas-zoomed")

                    if theme == "dark":
                        page.goto(f"{issuer}{base}?view=workarea")
                        out[f"{tag}_selects"] = page.locator("select[name=op]").count()
                        shot("workareas")
                    ctx.close()
        browser.close()
    out["requests"] = requests
    out["foreign"] = foreign
    out["console"] = console
    print(json.dumps(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
