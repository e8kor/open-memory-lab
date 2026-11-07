//! Skeleton checks for `oml-web`. The parity suite moves in phase (b).

#[test]
fn web_crate_exposes_routes_and_capacities() {
    assert!(!oml_web::WEB_ROUTES.is_empty());
    let _ = oml_web::Capacity::Web;
    let _ = oml_web::WebApiClientMode::InProcessLoopback;
}
