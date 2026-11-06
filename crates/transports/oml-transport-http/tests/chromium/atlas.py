"""The Atlas view in a real Chromium, driven by Playwright (ADR-0045 d25-d27).

Run by the Rust test `the_atlas_page_runs_its_scripts_from_the_same_origin_in_chromium`,
which serves the pages on a loopback port and passes it here. The browser
believes it is on the issuer's https origin: every request to the issuer is
answered by the loopback server, so the policy the server sends is the one
the browser enforces, by path, under the issuer. Any request elsewhere is
aborted and reported.

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
    found = sorted(cache.glob("chromium_headless_shell-*/chrome-headless-shell-linux64/chrome-headless-shell"))
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
        headers = r.request.all_headers()
        response = r.fetch(url=local + url[len(issuer):], headers=headers)
        r.fulfill(response=response)

    with sync_playwright() as p:
        args = ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"]
        try:
            browser = p.chromium.launch(args=args)
        except Exception as first:  # noqa: BLE001 - reported, not hidden
            # A Playwright newer than the cached browser: any cached Chromium.
            found = cached_chromium()
            if found is None:
                print(json.dumps({"did_not_run": f"Chromium would not start: {first}"}))
                return 0
            browser = p.chromium.launch(executable_path=found, args=args)
        for who, cookie in (("person", a.cookie), ("admin", a.admin_cookie)):
            ctx = browser.new_context(
                viewport={"width": 1280, "height": 900}, color_scheme="dark"
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
            base = "/web" if who == "person" else "/web/admin"

            page.goto(f"{issuer}{base}?view=memories")
            out[f"{who}_list_cards"] = page.locator("article.memory").count()
            if a.shots:
                page.screenshot(path=f"{a.shots}/{who}-memories-list.png", full_page=True)

            page.goto(f"{issuer}{base}?view=memories&display=atlas")
            page.wait_for_function(
                "document.documentElement.dataset.atlas !== undefined", timeout=20000
            )
            page.wait_for_timeout(1500)
            out[f"{who}_atlas"] = page.evaluate("document.documentElement.dataset.atlas")
            out[f"{who}_atlas_items"] = int(
                page.evaluate("document.documentElement.dataset.atlasItems || '-1'")
            )
            out[f"{who}_atlas_list"] = page.locator("#atlas-list li").count()
            out[f"{who}_regions_drawn"] = int(
                page.evaluate("document.documentElement.dataset.atlasRegions || '-1'")
            )
            out[f"{who}_regions_listed"] = page.locator("#atlas-regions li").count()
            if a.shots:
                page.screenshot(path=f"{a.shots}/{who}-memories-atlas.png")
                first = page.locator("#atlas-list li button").first
                if first.count():
                    first.click()
                    page.evaluate("window.scrollTo(0, 0)")
                    page.wait_for_timeout(500)
                    page.screenshot(path=f"{a.shots}/{who}-memories-atlas-selected.png")
            # An injected inline handler runs nothing under the policy.
            page.evaluate(
                "document.body.insertAdjacentHTML('beforeend',"
                "'<img src=\"data:,x\" onerror=\"window.__injected=1\">')"
            )
            page.wait_for_timeout(300)
            out[f"{who}_inline_ran"] = page.evaluate("window.__injected === 1")

            # The words each view shows, for the copy pass's report.
            views = (
                ["", "setup", "memories", "workarea", "guardrails", "review"]
                if who == "person"
                else ["", "queue", "memories", "workarea", "review", "held", "curate"]
            )
            for view in views:
                page.goto(f"{issuer}{base}" + (f"?view={view}" if view else ""))
                words = page.evaluate("document.querySelector('main').innerText")
                out.setdefault(f"{who}_words", {})[view or "home"] = len(words.split())

            if a.shots:
                page.goto(f"{issuer}{base}?view=memories&display=atlas")
                page.wait_for_function(
                    "document.documentElement.dataset.atlas !== undefined", timeout=20000
                )
                page.wait_for_timeout(1200)
                regions = page.locator("#atlas-regions li button")
                if regions.count() > 1:
                    regions.nth(1).click()
                    page.evaluate("window.scrollTo(0, 0)")
                    page.wait_for_timeout(400)
                    page.screenshot(path=f"{a.shots}/{who}-atlas-region.png")
                    zoom = page.get_by_text("Show only this workarea")
                    if zoom.count():
                        zoom.click()
                        page.wait_for_load_state()
                        page.wait_for_function(
                            "document.documentElement.dataset.atlas !== undefined",
                            timeout=20000,
                        )
                        out[f"{who}_zoomed"] = page.evaluate(
                            "document.documentElement.dataset.atlasRegions"
                        )
                        page.wait_for_timeout(1200)
                        page.screenshot(path=f"{a.shots}/{who}-atlas-zoomed.png")

            page.goto(f"{issuer}{base}?view=workarea")
            out[f"{who}_workarea_selects"] = page.locator("select[name=op]").count()
            if a.shots:
                page.screenshot(path=f"{a.shots}/{who}-workareas.png", full_page=True)
                select = page.locator("form.act").first
                if select.count():
                    select.locator("button").click()
                    page.wait_for_load_state()
                    page.screenshot(
                        path=f"{a.shots}/{who}-workareas-confirm.png", full_page=True
                    )
            ctx.close()
        browser.close()
    out["requests"] = requests
    out["foreign"] = foreign
    out["console"] = console
    print(json.dumps(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
