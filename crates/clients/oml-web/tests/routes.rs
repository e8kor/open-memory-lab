//! Every path router the repository ships sends `oml-web` exactly the pages
//! of [`oml_web::PAGES`] and every other path to the lead (ADR-0070 d4): the
//! reference Caddyfile, the docker profile's and `oml quickstart`'s.

use std::path::Path;

const ROUTERS: [&str; 3] = [
    "deploy/caddy/Caddyfile",
    "docker-compose.yml",
    "crates/bins/oml-cli/src/quickstart-compose.yaml",
];

fn read(relative: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    std::fs::read_to_string(root.join(relative))
        .unwrap_or_else(|e| panic!("{relative}: {e}"))
        .replace("\r\n", "\n")
}

#[test]
fn every_shipped_router_sends_oml_web_its_pages_and_the_lead_the_rest() {
    for relative in ROUTERS {
        let text = read(relative);
        let matchers = text.matches("@oml_web {").count();
        assert_eq!(matchers, 1, "{relative}: one oml-web matcher");
        for (method, path, view) in oml_web::PAGES {
            for line in [
                format!("method {method}"),
                format!("path {path}"),
                format!("query view={view}"),
            ] {
                assert!(text.contains(&line), "{relative}: {line}");
            }
        }
        // One matcher line per page, and nothing else matched to oml-web.
        assert_eq!(
            text.matches("query view=").count(),
            oml_web::PAGES.len(),
            "{relative}"
        );
        assert_eq!(
            text.matches("reverse_proxy @oml_web ").count(),
            1,
            "{relative}"
        );
        // The fall-through, after the matcher, is the lead.
        let routes: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("reverse_proxy "))
            .collect();
        assert_eq!(routes.len(), 2, "{relative}: {routes:?}");
        assert!(
            routes[0].starts_with("reverse_proxy @oml_web "),
            "{relative}"
        );
        assert!(
            routes[1].ends_with(":8090") || routes[1].ends_with("oml-server:8080"),
            "{relative}: {routes:?}"
        );
    }
}
