# Landing page

`index.html` is the public landing page of the maintainer's server, `https://memory.doublegate.io/`;
`teams.html` and `personal.html` are its two audience pages. Each is one self-contained file:
inline CSS, the OML mark inline, no external assets and no tracking. They are generated: edit
`index.src.html`, `teams.src.html`, `personal.src.html` or the shared `landing.src.css` and run
`python scripts/gen_design_tokens.py`, which fills in the design tokens, the header and footer,
the wordmark, the glyphs, the review-gate diagram, the display font, and the workspace version
from `Cargo.toml` for the footer and the install command (ADR-0069).

`privacy.html` and `terms.html` are drafts. Until the operator publishes them, the footers do not
link them: `gen_design_tokens.py --legal-links` adds the Privacy and Terms links, and
`LEGAL_LINKS` in the script is the default that `--check` compares against. `CONTACT_URL` and
`CONTACT_EMAIL` in the script set where "Ask about access" goes and the address the footers show;
change them there to point at a form or another address. Deploying it is a copy to `/srv/data/www/memory/`, an operator step. Caddy serves `index.html`, `teams.html`, `personal.html`, `privacy.html` and `terms.html` for `/`, `/teams`, `/personal`, `/privacy` and `/terms` alone and passes every other path to the lead.

```caddyfile
memory.doublegate.io {
	@landing path / /teams /personal /privacy /terms
	handle @landing {
		root * /srv/data/www/memory
		try_files {path}.html /index.html
		file_server
	}
	handle {
		reverse_proxy 127.0.0.1:8090
	}
}
```

Every claim on the page names a shipped release; planned work is linked to the release map,
never described here.
