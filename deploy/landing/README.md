# Landing page

`index.html` is the public landing page of the maintainer's server, `https://memory.doublegate.io/`.
It is one self-contained file: inline CSS, the OML mark inline, no external assets and no
tracking. Caddy serves it for `/` alone and passes every other path to the lead.

```caddyfile
memory.doublegate.io {
	@landing path /
	handle @landing {
		root * /srv/data/www/memory
		rewrite * /index.html
		file_server
	}
	handle {
		reverse_proxy 127.0.0.1:8090
	}
}
```

Every claim on the page names a shipped release; planned work is linked to the release map,
never described here.
