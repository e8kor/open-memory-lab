//! The frame that draws a Mermaid claim (ADR-0056 d9): one route answering
//! under a policy of its own, and the one bundle it runs.
//!
//! The pages stay without script. A page that shows a Mermaid claim embeds
//! `<iframe sandbox="allow-scripts" src="/web/render/<token>">`; the frame is
//! an opaque origin, so it has no cookie, no storage and no access to the
//! parent, and its policy forbids every network request. Its one input is
//! the source the session was already shown. The source stays in this
//! process's memory, keyed by the session and the memory (so more than one
//! `oml-web` replica needs session affinity for this route, ADR-0070 d4);
//! the token sealed into the
//! URL names the memory, the session and an expiry only, so a diagram near
//! the 64 KiB cap fits in a URL path and a token opens for no other
//! session. A rendering is not part of what is signed and journals nothing;
//! the reviewer always has the source beside it.
//!
//! Moved from the lead with the memory's own page (ADR-0070 d1, d7 (b)):
//! `oml-web` holds the source it read from the API for the session it
//! showed it to, so a reviewer's card draws a held diagram too.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::response::Response;
use http::{HeaderMap, HeaderValue, StatusCode, header};
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use serde_json::{Value, json};

use oml_page::digest::{base64_32, sha256};
use oml_page::escape;

use crate::session::{PageCalls, call, cookie};
use crate::{Capacity, SESSION_COOKIE, Web};

/// The pinned Mermaid release (MIT, `assets/mermaid.LICENSE`), vendored and
/// compiled in; nothing is fetched from a CDN.
pub const BUNDLE: &[u8] = include_bytes!(
    "../assets/mermaid-336df803d87a38b70a01e736e666c8f7601d2ce42f5e47c8fafe5d721074b9a3.js"
);

/// The bundle's SHA-256, the digest in its file name; a test holds the two
/// equal to the bytes.
pub const BUNDLE_SHA256: &str = "336df803d87a38b70a01e736e666c8f7601d2ce42f5e47c8fafe5d721074b9a3";

/// The frame's document, one token a path segment below it.
pub const RENDER_PREFIX: &str = "/web/render/";

/// The bundle the frame runs, its digest in its file name.
pub const ASSETS_PREFIX: &str = "/web/assets/";

/// What a render token opens for, so it never opens as another handle.
pub const RENDER_PURPOSE: &[u8] = b"oml render frame";

/// A token's life, seconds (ADR-0056 d9).
pub const RENDER_TTL_SECS: u64 = 300;

/// How many sources the process holds for frames at once; the oldest expiry
/// goes first. 256 sources of at most 64 KiB is 16 MiB, chosen, not
/// measured (#159).
pub const RENDER_SOURCES_MAX: usize = 256;

/// How many sources one session holds at once, so one session opening
/// many diagrams evicts only its own (#159; chosen, not measured).
pub const RENDER_SOURCES_PER_SESSION: usize = 32;

/// A held source's key (session, memory) and its value (expiry, source).
type Held = HashMap<(String, String), (u64, Arc<str>)>;

/// The sources the frame route draws from, keyed by session and memory, each
/// with its expiry. A restart or an eviction answers `not_found`; the card
/// still shows the source.
#[derive(Default)]
pub struct Sources {
    held: Mutex<Held>,
}

impl Sources {
    fn put(&self, session: &str, memory_id: &str, source: &str, expires: u64, now: u64) {
        let mut held = self.held.lock().expect("render sources lock");
        held.retain(|_, (e, _)| *e > now);
        held.remove(&(session.to_owned(), memory_id.to_owned()));
        // A session past its share gives up its own oldest first.
        while held.keys().filter(|(s, _)| s == session).count() >= RENDER_SOURCES_PER_SESSION {
            let Some(oldest) = held
                .iter()
                .filter(|((s, _), _)| s == session)
                .min_by_key(|(_, (e, _))| *e)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            held.remove(&oldest);
        }
        while held.len() >= RENDER_SOURCES_MAX {
            let Some(oldest) = held
                .iter()
                .min_by_key(|(_, (e, _))| *e)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            held.remove(&oldest);
        }
        held.insert(
            (session.to_owned(), memory_id.to_owned()),
            (expires, Arc::from(source)),
        );
    }

    fn get(&self, session: &str, memory_id: &str, now: u64) -> Option<Arc<str>> {
        let held = self.held.lock().expect("render sources lock");
        held.get(&(session.to_owned(), memory_id.to_owned()))
            .filter(|(e, _)| *e > now)
            .map(|(_, source)| Arc::clone(source))
    }

    /// How many sources are held, for the metric and the tests.
    #[must_use]
    pub fn len(&self) -> usize {
        self.held.lock().expect("render sources lock").len()
    }

    /// Whether none is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

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

/// The process's key for render tokens, drawn at start and never written
/// anywhere: a restart voids every token, and the card still shows the
/// source.
pub struct Sealer {
    key: LessSafeKey,
}

impl Default for Sealer {
    fn default() -> Self {
        let mut key = [0_u8; 32];
        getrandom::getrandom(&mut key).expect("the OS random source answers");
        Self {
            key: LessSafeKey::new(
                UnboundKey::new(&CHACHA20_POLY1305, &key).expect("a 32-byte key"),
            ),
        }
    }
}

impl Sealer {
    /// `plain` sealed under this process's key, as lower-case hex of the
    /// nonce, the ciphertext and the tag.
    #[must_use]
    pub fn seal(&self, plain: &[u8]) -> String {
        let mut nonce = [0_u8; NONCE_LEN];
        getrandom::getrandom(&mut nonce).expect("the OS random source answers");
        let mut sealed = plain.to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(RENDER_PURPOSE),
                &mut sealed,
            )
            .expect("a claim is far below the AEAD's length limit");
        nonce
            .iter()
            .chain(&sealed)
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// What `handle` seals under this process's key, or `None`.
    #[must_use]
    pub fn open(&self, handle: &str) -> Option<Vec<u8>> {
        if !handle.len().is_multiple_of(2) || handle.len() < 2 * NONCE_LEN {
            return None;
        }
        let bytes: Vec<u8> = (0..handle.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(handle.get(i..i + 2)?, 16).ok())
            .collect::<Option<_>>()?;
        let (nonce, sealed) = bytes.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
        let mut sealed = sealed.to_vec();
        let plain = self
            .key
            .open_in_place(nonce, Aad::from(RENDER_PURPOSE), &mut sealed)
            .ok()?;
        Some(plain.to_vec())
    }
}

/// The process's sealer and held sources.
#[derive(Default)]
pub struct Frames {
    sealer: Sealer,
    sources: Sources,
}

/// This process's frames: one key and one bounded store of sources.
pub static FRAMES: LazyLock<Frames> = LazyLock::new(Frames::default);

/// The token for `source`: it names the memory, the session's CSRF token as
/// the session binding and an expiry, sealed; the source is put in `sources`
/// under the same session and memory, never in the token.
#[must_use]
pub fn token(
    sealer: &Sealer,
    sources: &Sources,
    session: &str,
    memory_id: &str,
    source: &str,
    now: u64,
) -> String {
    let expires = now + RENDER_TTL_SECS;
    sources.put(session, memory_id, source, expires, now);
    let claim = json!({ "e": expires, "s": session, "i": memory_id });
    sealer.seal(claim.to_string().as_bytes())
}

/// The source a token opens for `session` at `now`, or `None` for a token
/// that is forged, expired, another session's or not a render token, or
/// whose source the process no longer holds.
#[must_use]
pub fn open(
    sealer: &Sealer,
    sources: &Sources,
    handle: &str,
    session: &str,
    now: u64,
) -> Option<Arc<str>> {
    let bytes = sealer.open(handle)?;
    let claim: Value = serde_json::from_slice(&bytes).ok()?;
    let expires = claim["e"].as_u64()?;
    if now >= expires || claim["s"].as_str()? != session {
        return None;
    }
    sources.get(session, claim["i"].as_str()?, now)
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
        .extensions_mut()
        .insert(crate::middleware::OwnHeaders);
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
        .extensions_mut()
        .insert(crate::middleware::OwnHeaders);
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

/// The media type of a Mermaid claim (ADR-0056 d1).
pub const MERMAID: &str = "text/vnd.oml.mermaid";

/// Seconds since the epoch, for a token's expiry.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The drawing beside a Mermaid claim's source, bound to the session by its
/// CSRF token: `None` for any other format, or where the public origin the
/// frame's policy names is not known yet.
#[must_use]
pub fn drawing(
    public_origin: Option<&str>,
    format: Option<&str>,
    csrf: &str,
    memory_id: &str,
    source: &str,
) -> Option<String> {
    public_origin?;
    (format == Some(MERMAID)).then(|| iframe(&mint(csrf, memory_id, source, unix_now())))
}

/// A token for `source` minted at `now` under this process's key and held
/// sources, bound to the session's CSRF token.
#[must_use]
pub fn mint(csrf: &str, memory_id: &str, source: &str, now: u64) -> String {
    token(
        &FRAMES.sealer,
        &FRAMES.sources,
        csrf,
        memory_id,
        source,
        now,
    )
}

/// The pages' policy with the one frame source a drawing needs: the render
/// route under the public origin, and nothing else.
#[must_use]
pub fn framing_policy(public_origin: &str) -> String {
    oml_page::web::content_security_policy(&[], Some(&frame_src(public_origin)))
}

/// `page` under [`framing_policy`] where it holds a drawing; the headers
/// layer keeps a policy a handler set.
pub fn allow_frames(page: &mut Response, public_origin: Option<&str>) {
    if let Some(origin) = public_origin
        && let Ok(value) = HeaderValue::from_str(&framing_policy(origin))
    {
        page.headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, value);
        page.extensions_mut().insert(crate::middleware::OwnHeaders);
    }
}

fn not_found() -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}

/// `GET /web/render/{token}`: the frame's document for a token this session
/// holds, else `404`: a forged, expired or another session's token, no
/// session, or one the API no longer resolves, answer alike (ADR-0056 d9).
/// The session is the API's to judge (ADR-0070 d3 layer 3): one `whoami`.
pub async fn frame(
    State(web): State<Web>,
    calls: Option<axum::Extension<Arc<PageCalls>>>,
    headers: HeaderMap,
    Path(handle): Path<String>,
) -> Response {
    let (Some(origin), Some(session_token)) = (
        web.layers.public_origin.clone(),
        cookie(&headers, SESSION_COOKIE),
    ) else {
        return not_found();
    };
    let calls = calls.map_or_else(|| Arc::new(PageCalls::new("frame")), |c| c.0);
    let credential =
        oml_web_credential::capacity_credential(&session_token, Capacity::Web.as_str());
    let ids = calls.next_ids();
    if web.api.whoami(&call(&credential, &ids)).await.is_err() {
        return not_found();
    }
    let csrf = oml_web_credential::csrf_token(&session_token);
    match open(&FRAMES.sealer, &FRAMES.sources, &handle, &csrf, unix_now()) {
        Some(source) => frame_response(&origin, &source),
        None => not_found(),
    }
}

/// `GET /web/assets/{file}`: the pinned bundle, and no other file.
pub async fn asset(Path(file): Path<String>) -> Response {
    if file == format!("mermaid-{BUNDLE_SHA256}.js") {
        bundle_response()
    } else {
        not_found()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pinned_bundle_is_the_one_its_file_name_says() {
        let digest = sha256(BUNDLE);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, BUNDLE_SHA256);
    }

    #[test]
    fn a_token_opens_for_its_session_before_it_expires_and_never_after() {
        let sealer = Sealer::default();
        let sources = Sources::default();
        let t = token(
            &sealer,
            &sources,
            "csrf-a",
            "id",
            "flowchart LR\n A-->B",
            1_000,
        );
        assert_eq!(
            open(&sealer, &sources, &t, "csrf-a", 1_000 + RENDER_TTL_SECS - 1).as_deref(),
            Some("flowchart LR\n A-->B")
        );
        assert_eq!(
            open(&sealer, &sources, &t, "csrf-a", 1_000 + RENDER_TTL_SECS),
            None
        );
        assert_eq!(
            open(&sealer, &sources, &t, "csrf-b", 1_000),
            None,
            "another session"
        );
        let other = Sealer::default();
        assert_eq!(
            open(&other, &sources, &t, "csrf-a", 1_000),
            None,
            "another process's key"
        );
        assert!(!t.contains("flowchart"), "the source is never in the token");
    }

    #[test]
    fn the_held_sources_are_bounded_and_the_oldest_expiry_goes_first() {
        let sealer = Sealer::default();
        let sources = Sources::default();
        // Sixteen sessions, each under its share, fill the whole pool.
        for i in 0..RENDER_SOURCES_MAX + 10 {
            let _ = token(
                &sealer,
                &sources,
                &format!("s{}", i % 16),
                &format!("m{i}"),
                "flowchart LR",
                1_000 + i as u64,
            );
        }
        assert_eq!(sources.len(), RENDER_SOURCES_MAX);
        let first = token(&sealer, &sources, "s0", "m0", "flowchart LR", 5_000);
        assert!(open(&sealer, &sources, &first, "s0", 5_001).is_some());
    }

    #[test]
    fn one_session_holds_at_most_its_share_and_evicts_only_its_own_sources() {
        let sealer = Sealer::default();
        let sources = Sources::default();
        let other = token(&sealer, &sources, "other", "m", "flowchart LR", 1_000);
        for i in 0..RENDER_SOURCES_MAX {
            let _ = token(
                &sealer,
                &sources,
                "busy",
                &format!("m{i}"),
                "flowchart LR",
                1_000 + i as u64,
            );
        }
        assert_eq!(sources.len(), RENDER_SOURCES_PER_SESSION + 1);
        assert!(
            open(&sealer, &sources, &other, "other", 1_001).is_some(),
            "another session's source stays"
        );
    }

    /// The shared pool's bound is still one pool (#159): past it, the
    /// source with the oldest expiry goes, whichever session holds it, so
    /// a person with many sessions under their share each can still evict
    /// another session's; that session's frame answers `404` and its card
    /// still shows the source.
    #[test]
    fn a_full_pool_gives_up_its_oldest_source_whichever_session_holds_it() {
        let sealer = Sealer::default();
        let sources = Sources::default();
        let sessions = RENDER_SOURCES_MAX / RENDER_SOURCES_PER_SESSION;
        let oldest = token(&sealer, &sources, "s0", "m-oldest", "flowchart LR", 1_000);
        for i in 1..RENDER_SOURCES_MAX {
            let _ = token(
                &sealer,
                &sources,
                &format!("s{}", i % sessions),
                &format!("m{i}"),
                "flowchart LR",
                1_000 + i as u64,
            );
        }
        assert_eq!(sources.len(), RENDER_SOURCES_MAX, "the pool is full");
        let now = 1_000 + RENDER_SOURCES_MAX as u64;
        let newcomer = token(&sealer, &sources, "newcomer", "m", "flowchart LR", now);
        assert_eq!(sources.len(), RENDER_SOURCES_MAX);
        assert!(open(&sealer, &sources, &newcomer, "newcomer", now + 1).is_some());
        assert!(
            open(&sealer, &sources, &oldest, "s0", now + 1).is_none(),
            "another session's oldest source was evicted"
        );
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
