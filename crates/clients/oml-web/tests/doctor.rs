//! `oml-web --doctor` (ADR-0070 d4): the lead's `oml doctor` cannot read
//! `oml-web`'s settings, so `oml-web` prints its own lines. `web.api_url` is
//! `ok`, `warn` (plain http allowed by the opt-in) or `fail` (refused), and
//! every other setting the start refuses is a `fail` naming it.

use std::collections::HashMap;

use oml_web::doctor::{Level, config_lines};

const SECRET: &str = "0123456789abcdef0123456789abcdef";

fn lines(pairs: &[(&str, &str)]) -> Vec<(Level, String)> {
    let env: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    config_lines(|name| env.get(name).cloned())
}

fn api_url_line(lines: &[(Level, String)]) -> (Level, String) {
    lines
        .iter()
        .find(|(_, text)| text.starts_with("web.api_url"))
        .cloned()
        .expect("a web.api_url line")
}

#[test]
fn the_doctor_prints_web_api_url_as_ok_warn_or_fail() {
    let ok = lines(&[
        ("OML_WEB__API_URL", "http://127.0.0.1:8090"),
        ("OML_WEB__CLIENT_SECRET", SECRET),
    ]);
    assert_eq!(api_url_line(&ok).0, Level::Ok, "{ok:?}");
    assert!(ok.iter().all(|(level, _)| *level == Level::Ok), "{ok:?}");

    let allowed = lines(&[
        ("OML_WEB__API_URL", "http://oml-server:8080"),
        ("OML_WEB__ALLOW_PLAIN_HTTP_API", "true"),
        ("OML_WEB__CLIENT_SECRET", SECRET),
    ]);
    let (level, text) = api_url_line(&allowed);
    assert_eq!(level, Level::Warn, "{text}");
    assert!(text.contains("oml-server:8080"), "{text}");
    assert!(text.contains("web.allow_plain_http_api"), "{text}");

    let refused = lines(&[
        ("OML_WEB__API_URL", "http://oml-server:8080"),
        ("OML_WEB__CLIENT_SECRET", SECRET),
    ]);
    let (level, text) = api_url_line(&refused);
    assert_eq!(level, Level::Fail, "{text}");
    assert!(text.contains("plain http to a non-loopback host"), "{text}");
}

#[test]
fn the_doctor_fails_a_missing_setting_by_name_and_never_prints_the_secret() {
    let missing = lines(&[("OML_WEB__CLIENT_SECRET", SECRET)]);
    let (level, text) = api_url_line(&missing);
    assert_eq!(level, Level::Fail);
    assert!(text.contains("OML_WEB__API_URL"), "{text}");

    let no_secret = lines(&[("OML_WEB__API_URL", "https://memory.example")]);
    assert_eq!(api_url_line(&no_secret).0, Level::Ok);
    assert!(
        no_secret
            .iter()
            .any(|(level, text)| *level == Level::Fail && text.starts_with("web.client_secret")),
        "{no_secret:?}"
    );

    let all = lines(&[
        ("OML_WEB__API_URL", "https://memory.example"),
        ("OML_WEB__CLIENT_SECRET", SECRET),
    ]);
    assert!(
        all.iter().all(|(_, text)| !text.contains(SECRET)),
        "{all:?}"
    );
}
