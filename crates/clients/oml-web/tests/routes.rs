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

/// An API no call reaches: a redirect reads no session.
struct Nowhere;

impl oml_sdk::Transport for Nowhere {
    fn send(&self, _: http::Request<Vec<u8>>) -> oml_sdk::Sending<'_> {
        Box::pin(async {
            Err(oml_sdk::TransportError::Unreachable(
                "no API in this test".to_owned(),
            ))
        })
    }
}

/// `path`'s answer from `oml-web` alone, with no session cookie: its
/// status and `Location`.
async fn moved(path: &str) -> (http::StatusCode, Option<String>) {
    use tower::ServiceExt as _;
    let web = oml_web::router(oml_web::Web {
        layers: oml_web::Layers {
            public_origin: Some("https://memory.example".to_owned()),
            log: std::sync::Arc::new(|_| {}),
        },
        api: oml_web::api(
            std::sync::Arc::new(Nowhere),
            "0123456789abcdef0123456789abcdef",
        )
        .unwrap(),
    });
    let response = web
        .oneshot(
            http::Request::get(path)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let location = response
        .headers()
        .get(http::header::LOCATION)
        .map(|v| v.to_str().unwrap().to_owned());
    (response.status(), location)
}

/// The Review view's old path answers `308` to To do for one release,
/// keeping the workspace and the linked key it judged as, and dropping
/// every other field; it reads no session (ADR-0083 d6).
#[tokio::test]
async fn the_old_review_path_answers_308_to_todo_keeping_the_workspace() {
    for (path, location) in [
        ("/web/review", "/web/todo"),
        (
            "/web/review?workspace=alice.proj",
            "/web/todo?workspace=alice.proj",
        ),
        (
            "/web/review?held=1&workspace=team&author_key=k&as=0f0e8a52-5d1c-4f43-9a5e-2f7c1f9b8f10",
            "/web/todo?workspace=team&as=0f0e8a52-5d1c-4f43-9a5e-2f7c1f9b8f10",
        ),
    ] {
        assert_eq!(
            moved(path).await,
            (
                http::StatusCode::PERMANENT_REDIRECT,
                Some(location.to_owned())
            ),
            "{path}"
        );
    }
    // A POST is not oml-web's any more: the proxy sends it to the lead.
    assert!(!oml_web::serves("POST", "/web/review"));
}

/// The admin Review view's old path answers `308` to the admin Memories
/// page's Held view, keeping the workspace (ADR-0083 d6).
#[tokio::test]
async fn the_old_admin_review_path_answers_308_to_the_held_view() {
    for (path, location) in [
        ("/web/admin/review", "/web/admin/memories?view=held"),
        (
            "/web/admin/review?workspace=team",
            "/web/admin/memories?view=held&workspace=team",
        ),
    ] {
        assert_eq!(
            moved(path).await,
            (
                http::StatusCode::PERMANENT_REDIRECT,
                Some(location.to_owned())
            ),
            "{path}"
        );
    }
    assert!(!oml_web::serves("POST", "/web/admin/review"));
}
