# Landing site

The public landing site of the maintainer's server, `https://memory.doublegate.io/`: `index.html`,
and `teams.html` and `personal.html`, its two audience pages. It is a deployable of its own, apart
from the server and the web console: static files that a web server such as Caddy serves, built
here and shipped by each release as `oml-site-<version>.tar.gz`. Each page is one self-contained
file: inline CSS, the OML mark inline, no external assets and no tracking.

## Layout

| Path | What | Edit |
|---|---|---|
| `src/*.src.html`, `src/landing.src.css` | the page sources and their shared stylesheet | yes |
| `src/privacy.html`, `src/terms.html` | the legal drafts, copied as they are | yes |
| `design/` | the design system's tokens, display font, marks, glyphs and review-gate diagram | no: `python scripts/gen_design_tokens.py` writes it from `design/` at the root (ADR-0069) |
| `build.py` | assembles `dist/` from `src/`, `design/` and the workspace version in `Cargo.toml` | yes |
| `dist/` | what is served and what the release archive holds | no: `python site/build.py` writes it |

## Build

```sh
python scripts/gen_design_tokens.py   # after a change under design/
python site/build.py                  # after a change under site/src/, site/design/ or the version
python site/build.py --check          # what CI runs: exit 1 when dist/ is stale
```

`build.py` fills in the header and footer, the wordmark, the glyphs, the review-gate diagram, the
display font, and the workspace version for the footer and the install command. A version bump
therefore needs `python site/build.py` in the same commit, or CI fails.

`privacy.html` and `terms.html` are drafts. Until the operator publishes them, the footers do not
link them: `python site/build.py --legal-links` adds the Privacy and Terms links, and
`LEGAL_LINKS` in `build.py` is the default that `--check` compares against. `CONTACT_URL` and
`CONTACT_EMAIL` in `build.py` set where "Ask about access" goes and the address the footers show;
change them there to point at a form or another address.

## Deploy

The release archive `oml-site-<version>.tar.gz` holds `oml-site-<version>/` with the pages of
`dist/` and the licences; it is summed in `SHA256SUMS` and signed like every other release file.
Deploying it is an operator step: verify it, extract it, and point the web server at the
directory.

Caddy serves the five pages for `/`, `/teams`, `/personal`, `/privacy` and `/terms`, and routes
the rest by path. The web console runs as its own process, `oml-web`
([ADR-0070](../docs/adr/ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md)),
on port 8091 here. Its pages move from the lead one at a time, so the proxy sends `oml-web` only
the pages it serves, matched by method, path and query, and every other path to the lead, here on
port 8090: the API, MCP, OAuth, discovery, sign-in (`/web/sign-in`, `/web/callback/*`,
`/web/sign-up`) and the pages that have not moved. `deploy/caddy/Caddyfile` is the reference, and
it grows as pages move.

```caddyfile
memory.doublegate.io {
	@landing path / /teams /personal /privacy /terms
	handle @landing {
		root * /srv/data/www/memory
		try_files {path}.html /index.html
		file_server
	}
	# The pages oml-web serves (oml_web::PAGES); the list grows as pages move.
	@oml_web path /web/guardrails /web/workareas /web/admin/workareas /web/memories /web/atlas /web/admin/memories /web/admin/atlas /web/data/memories /web/admin/data/memories
	reverse_proxy @oml_web 127.0.0.1:8091
	reverse_proxy 127.0.0.1:8090
}
```

Every claim on the page names a shipped release; planned work is linked to the release map,
never described here.
