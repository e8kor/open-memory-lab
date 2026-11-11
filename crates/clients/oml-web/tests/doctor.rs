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

/// A secret the start refuses as weak, or set both inline and as a file,
/// is a `fail` naming the setting, and `web.api_url` still gets its own
/// line (#230).
#[test]
fn the_doctor_fails_a_weak_or_doubly_set_client_secret_by_name() {
    for pairs in [
        vec![
            ("OML_WEB__API_URL", "https://memory.example"),
            (
                "OML_WEB__CLIENT_SECRET",
                "ssssssssssssssssssssssssssssssssssss",
            ),
        ],
        vec![
            ("OML_WEB__API_URL", "https://memory.example"),
            ("OML_WEB__CLIENT_SECRET", SECRET),
            (
                "OML_WEB__CLIENT_SECRET_FILE",
                "/run/credentials/oml-web/client",
            ),
        ],
    ] {
        let got = lines(&pairs);
        assert_eq!(api_url_line(&got).0, Level::Ok, "{got:?}");
        assert!(
            got.iter()
                .any(|(level, text)| *level == Level::Fail && text.starts_with("web.client_secret")),
            "{got:?}"
        );
        assert!(
            got.iter().all(|(_, text)| !text.contains(SECRET)),
            "{got:?}"
        );
    }
}

/// The lead's answer to the doctor's probe: a status and a body, and the
/// requests it saw.
struct Lead(
    u16,
    serde_json::Value,
    std::sync::Arc<std::sync::Mutex<Vec<http::Request<Vec<u8>>>>>,
);

impl oml_sdk::Transport for Lead {
    fn send(&self, request: http::Request<Vec<u8>>) -> oml_sdk::Sending<'_> {
        self.2.lock().unwrap().push(request);
        let (status, body) = (self.0, self.1.clone());
        Box::pin(async move {
            Ok(http::Response::builder()
                .status(status)
                .body(body.to_string().into_bytes())
                .unwrap())
        })
    }
}

fn refusal(reason: &str) -> serde_json::Value {
    serde_json::json!({
        "error": {"code": "auth.invalid", "message": "refused", "details": {"reason": reason}},
        "request_id": "req_1",
    })
}

/// Whether the lead accepts the secret oml-web sends, which `/readyz`
/// cannot say: the doctor presents a capacity credential no session
/// holds, and the lead names `session_outside_web` only when the secret
/// did not prove oml-web, as after a rotation done out of order or a
/// closed window (#230). The probe carries the secret as the pages do and
/// no line prints it.
#[tokio::test]
async fn the_doctor_fails_when_the_lead_refuses_oml_webs_secret_and_passes_when_it_accepts_it() {
    use oml_web::doctor::secret_line;
    for (reason, want) in [("session_outside_web", Level::Fail), ("unknown", Level::Ok)] {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let api = oml_web::api(
            std::sync::Arc::new(Lead(401, refusal(reason), seen.clone())),
            SECRET,
        )
        .unwrap();
        let (level, text) = secret_line(&api, "http://127.0.0.1:8090").await;
        assert_eq!(level, want, "{reason}: {text}");
        assert!(text.starts_with("api: "), "{text}");
        assert!(!text.contains(SECRET), "{text}");
        if want == Level::Fail {
            assert!(text.contains("web.client_secret"), "{text}");
        }
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].uri().path(), "/v2/whoami");
        assert_eq!(seen[0].headers()["oml-web-client"], SECRET);
        let bearer = seen[0].headers()["authorization"].to_str().unwrap();
        assert!(bearer.starts_with("Bearer omls_"), "{bearer}");
    }
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let api = oml_web::api(
        std::sync::Arc::new(Lead(500, serde_json::json!({"oops": true}), seen)),
        SECRET,
    )
    .unwrap();
    assert_eq!(
        secret_line(&api, "http://127.0.0.1:8090").await.0,
        Level::Fail
    );
}
