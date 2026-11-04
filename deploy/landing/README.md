# Landing page

`index.html` is the public landing page of the maintainer's server, `https://memory.doublegate.io/`.
It is one self-contained file: inline CSS, the OML mark inline, no external assets and no
tracking. Caddy serves `index.html`, `privacy.html` and `terms.html` for `/`, `/privacy` and `/terms` alone and passes every other path to the lead.

```caddyfile
memory.doublegate.io {
	@landing path / /privacy /terms
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
