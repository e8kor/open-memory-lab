//! The frame that draws a Mermaid claim (ADR-0056 d9): one route answering
//! under a policy of its own, and the one bundle it runs.
//!
//! The pages stay without script. A page that shows a Mermaid claim embeds
//! `<iframe sandbox="allow-scripts" src="/web/render/<token>">`; the frame is
//! an opaque origin, so it has no cookie, no storage and no access to the
//! parent, and its policy forbids every network request. Its one input is
//! the source the session was already shown, sealed into the token with the
//! session and an expiry, so what is drawn is what was served and a token
//! opens for no other session. A rendering is not part of what is signed
//! and journals nothing; the reviewer always has the source beside it.

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use serde_json::{Value, json};

use crate::oauth::page::{base64_32, escape, sha256};

/// The pinned Mermaid release (MIT, `assets/mermaid.LICENSE`), vendored and
/// compiled in; nothing is fetched from a CDN.
pub const BUNDLE: &[u8] = include_bytes!(
    "../../assets/mermaid-336df803d87a38b70a01e736e666c8f7601d2ce42f5e47c8fafe5d721074b9a3.js"
);

/// The bundle's SHA-256, the digest in its file name; a test holds the two
/// equal to the bytes.
pub const BUNDLE_SHA256: &str = "336df803d87a38b70a01e736e666c8f7601d2ce42f5e47c8fafe5d721074b9a3";

/// The routes of the frame, outside [`super::WEB_ROUTES`]: the frame's
/// document and the bundle it runs.
pub const RENDER_ROUTES: &[(&str, &str)] = &[
    ("GET", "/web/render/{token}"),
    ("GET", "/web/assets/{file}"),
];

/// What a render token opens for, so it never opens as another handle.
pub const RENDER_PURPOSE: &[u8] = b"oml render frame";

/// A token's life, seconds (ADR-0056 d9).
pub const RENDER_TTL_SECS: u64 = 300;

/// The one script the frame runs besides the bundle: Mermaid at
/// `securityLevel: "strict"` with no HTML labels, and Mermaid's own message
/// in the frame when a diagram does not draw.
const INIT: &str = "(async function(){try{var m=(globalThis.__esbuild_esm_mermaid_nm||{}).mermaid;\
m=m&&m.default?m.default:m;\
m.initialize({startOnLoad:false,securityLevel:'strict',htmlLabels:false,suppressErrorRendering:true,flowchart:{htmlLabels:false}});\
var n=document.querySelectorAll('pre.mermaid');\
for(var i=0;i<n.length;i++){try{var r=await m.render('d'+i,n[i].textContent);n[i].innerHTML=r.svg;}\
catch(e){n[i].textContent=String(e&&e.message?e.message:e);n[i].className='error';}}\
document.title='drawn';}catch(e){document.body.textContent=String(e);}})();";

/// The path of the bundle, its digest in the name.
#[must_use]
pub fn bundle_path() -> String {
    format!("/web/assets/mermaid-{BUNDLE_SHA256}.js")
}

/// What a parent page may frame: the render route under the issuer.
#[must_use]
pub fn frame_src(issuer: &str) -> String {
    format!("{}/web/render/", issuer.trim_end_matches('/'))
}

/// The frame route's own policy (ADR-0056 d9): scripts from the pinned
/// bundle and the hashed initialiser alone, no network, no form, and only
/// the issuer's pages may frame it.
#[must_use]
pub fn frame_policy(issuer: &str) -> String {
    let issuer = issuer.trim_end_matches('/');
    let digest = base64_32(&sha256(INIT.as_bytes()));
    let digest = std::str::from_utf8(&digest).expect("base64 is ASCII");
    format!(
        "sandbox allow-scripts; default-src 'none'; script-src {issuer}{} 'sha256-{digest}'; \
         style-src 'unsafe-inline'; img-src 'none'; font-src 'none'; connect-src 'none'; \
         form-action 'none'; base-uri 'none'; frame-ancestors {issuer}",
        bundle_path()
    )
}

/// The frame's document: the source escaped in a `pre`, the bundle and the
/// initialiser.
#[must_use]
pub fn frame_document(source: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>diagram</title>\
         <style>body{{margin:0;font:14px sans-serif;background:#fff;color:#111}}\
         pre.mermaid{{margin:8px;white-space:pre-wrap}}pre.error{{color:#a00}}\
         svg{{max-width:100%;height:auto}}</style></head><body>\
         <pre class=\"mermaid\">{}</pre><script src=\"{}\"></script><script>{INIT}</script>\
         </body></html>",
        escape(source),
        bundle_path()
    )
}

/// The token for `source`: the memory it belongs to, the session's CSRF
/// token as the session binding, and its expiry, sealed.
#[must_use]
pub fn token(
    sealer: &impl oml_app::ports::HandleSealer,
    session: &str,
    memory_id: &str,
    source: &str,
    now: u64,
) -> String {
    let claim = json!({
        "e": now + RENDER_TTL_SECS,
        "s": session,
        "i": memory_id,
        "x": source,
    });
    sealer.seal(claim.to_string().as_bytes())
}

/// The source a token opens for `session` at `now`, or `None` for a token
/// that is forged, expired, another session's or not a render token.
#[must_use]
pub fn open(
    sealer: &impl oml_app::ports::HandleSealer,
    handle: &str,
    session: &str,
    now: u64,
) -> Option<String> {
    let bytes = sealer.open(handle)?;
    let claim: Value = serde_json::from_slice(&bytes).ok()?;
    let expires = claim["e"].as_u64()?;
    if now >= expires || claim["s"].as_str()? != session {
        return None;
    }
    claim["x"].as_str().map(str::to_owned)
}

/// The frame's answer: the document under its own policy, never cached.
#[must_use]
pub fn frame_response(issuer: &str, source: &str) -> Response {
    let mut response = Response::new(Body::from(frame_document(source)));
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&frame_policy(issuer)).expect("the policy is ASCII"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// The bundle's answer, immutable: its digest is in its name.
#[must_use]
pub fn bundle_response() -> Response {
    let mut response = Response::new(Body::from(BUNDLE));
    *response.status_mut() = StatusCode::OK;
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/javascript; charset=utf-8"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// The iframe a card embeds for `token`; the sandbox grants scripts and
/// nothing else, never the parent's origin.
#[must_use]
pub fn iframe(token: &str) -> String {
    format!(
        "<p class=\"label\">drawn from the source above in a sandbox, never signed</p>\
         <iframe sandbox=\"allow-scripts\" src=\"/web/render/{token}\" title=\"diagram\" \
         referrerpolicy=\"no-referrer\" loading=\"lazy\" width=\"100%\" height=\"360\"></iframe>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::oauth::seal::AeadSealer;

    #[test]
    fn the_pinned_bundle_is_the_one_its_file_name_says() {
        let digest = sha256(BUNDLE);
        assert_eq!(hex::encode(digest), BUNDLE_SHA256);
    }

    #[test]
    fn a_token_opens_for_its_session_before_it_expires_and_never_after() {
        let sealer = AeadSealer::generate_for(RENDER_PURPOSE);
        let t = token(&sealer, "csrf-a", "id", "flowchart LR\n A-->B", 1_000);
        assert_eq!(
            open(&sealer, &t, "csrf-a", 1_000 + RENDER_TTL_SECS - 1).as_deref(),
            Some("flowchart LR\n A-->B")
        );
        assert_eq!(open(&sealer, &t, "csrf-a", 1_000 + RENDER_TTL_SECS), None);
        assert_eq!(open(&sealer, &t, "csrf-b", 1_000), None, "another session");
        let other = AeadSealer::generate_for(b"oml web sign-in");
        assert_eq!(open(&other, &t, "csrf-a", 1_000), None, "another purpose");
    }

    #[test]
    fn the_frame_policy_names_the_bundle_and_the_hashed_initialiser_alone() {
        let policy = frame_policy("https://oml.example.com/");
        assert!(policy.contains(&format!(
            "script-src https://oml.example.com{} 'sha256-",
            bundle_path()
        )));
        assert_eq!(policy.matches("script-src").count(), 1);
        assert!(!policy.contains("unsafe-eval"));
        let script = policy
            .split("; ")
            .find(|d| d.starts_with("script-src"))
            .expect("a script-src");
        assert!(!script.contains("unsafe"), "{script}");
        assert!(policy.starts_with("sandbox allow-scripts; default-src 'none';"));
    }
}
