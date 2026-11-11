//! Every path router the repository ships sends `oml-web` exactly the paths
//! of [`oml_web::PAGES`] and the prefixes of [`oml_web::PREFIXES`] and every other path to the lead, by path alone
//! (ADR-0070 d4, d7 (b) as amended): the reference Caddyfile, the docker
//! profile's, `oml quickstart`'s, and the Helm chart's default Ingress paths.

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

/// Each path of [`oml_web::PAGES`] once, in the table's order: a path
/// serves every method its rows name.
fn paths() -> String {
    let mut paths: Vec<&str> = Vec::new();
    for (_, path) in oml_web::PAGES {
        if !paths.contains(path) {
            paths.push(path);
        }
    }
    for prefix in oml_web::PREFIXES {
        paths.push(Box::leak(format!("{prefix}*").into_boxed_str()));
    }
    paths.join(" ")
}

#[test]
fn every_shipped_router_sends_oml_web_its_paths_and_the_lead_the_rest() {
    for relative in ROUTERS {
        let text = read(relative);
        let matcher: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("@oml_web "))
            .collect();
        assert_eq!(
            matcher,
            [format!("@oml_web path {}", paths())],
            "{relative}: one path matcher naming exactly oml_web::PAGES"
        );
        assert!(!text.contains("query "), "{relative}: no query routing");
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

#[test]
fn the_charts_default_ingress_paths_are_oml_webs_pages() {
    let values = read("deploy/helm/oml/values.yaml");
    for (_, path) in oml_web::PAGES {
        assert!(
            values.contains(&format!("- path: {path}\n      pathType: Exact")),
            "values.yaml web.ingressPaths routes {path}"
        );
    }
    for prefix in oml_web::PREFIXES {
        let path = prefix.trim_end_matches('/');
        assert!(
            values.contains(&format!("- path: {path}\n      pathType: Prefix")),
            "values.yaml web.ingressPaths routes {path} as a prefix"
        );
    }
}
