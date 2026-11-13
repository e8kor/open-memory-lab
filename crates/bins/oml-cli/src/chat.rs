//! Chat memory in the agent plugin (ADR-0063): the turn counter and turn
//! hashes at `UserPromptSubmit`, the capture request at `Stop`, the
//! compaction mark at `PreCompact`, the opt-in per-turn recall, and the
//! pseudonymous session a chat anchor names.
//!
//! No hook here sends a turn's text anywhere, and none calls a model: the
//! agent running in the harness extracts the facts and writes them with
//! `oml_write` (d6). The session cache beside ADR-0038's holds turn numbers
//! and digests, never a prompt.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::client::{CallOpts, Client};
use crate::events::{self, Event, Reply};
use crate::hook::{
    FRAMING, KEY_CAP, Pair, Quiet, cache_path, cut, cut_bytes, describe, is_secret, one_line,
    resolve_pair_for,
};

/// Asks the agent to capture chat facts at `Stop` (ADR-0063 d6).
pub const CAPTURE_SWITCH: &str = "OML_CHAT_CAPTURE";
/// Lets the capture skill quote one excerpt under a fact (d5).
pub const EXCERPT_SWITCH: &str = "OML_CHAT_EXCERPT";
/// Recall on every prompt (d10).
pub const TURN_RECALL_SWITCH: &str = "OML_TURN_RECALL";
/// The chat workarea, by name or id (d8).
pub const WORKAREA_VAR: &str = "OML_CHAT_WORKAREA";
/// The plugin option Claude Code exports for the chat workarea.
pub const OPTION_WORKAREA: &str = "CLAUDE_PLUGIN_OPTION_CHAT_WORKAREA";
/// Turns between two capture requests (d6).
pub const EVERY_VAR: &str = "OML_CHAT_CAPTURE_EVERY";
/// Per-turn recall's latency budget in milliseconds (d10).
pub const BUDGET_VAR: &str = "OML_TURN_RECALL_BUDGET_MS";

/// The chat workarea's name under a user root (ADR-0063 d8).
pub const CHAT_WORKAREA: &str = "chat";
/// The harness a chat anchor names.
pub const HARNESS: &str = "claude-code";
/// The capture skill the `Stop` reason asks for.
pub const SKILL: &str = "oml:oml-chat-capture";
/// The default turns between captures.
pub const EVERY_DEFAULT: u64 = 5;
const EVERY_MAX: u64 = 50;
/// Capture requests per session.
pub const CAPTURES_MAX: u32 = 4;
/// Facts per capture request.
pub const FACTS_PER_CAPTURE: u32 = 6;
/// Facts per session.
pub const FACTS_PER_SESSION: u32 = 24;
/// The default per-turn recall budget, and its ceiling.
pub const BUDGET_DEFAULT_MS: u64 = 300;
pub const BUDGET_MAX_MS: u64 = 1000;
/// Missed deadlines after which per-turn recall stops for the session.
pub const MISSES_MAX: u32 = 3;
/// Hits per scope, tokens per recall, and the injected text's caps.
const TURN_K: u64 = 3;
const TURN_MAX_TOKENS: u64 = 150;
const TURN_LINE_CAP: usize = 400;
pub const TURN_CONTEXT_CAP: usize = 1600;
/// Distinctive words of a prompt sent as the query.
pub const PROMPT_WORDS_MAX: usize = 8;
/// The turns the reason lists by number and hash; older ones are a span.
const TURNS_LISTED: usize = 12;

/// The line under the frame on every per-turn injection (d10).
pub const TURN_LINE: &str =
    "Recalled from OML for this prompt; it may be irrelevant — ignore it if so.";

/// What the chat hooks keep per session: one `oml-turn/1` digest per turn,
/// the last turn a capture was asked over, the captures asked, whether the
/// session compacted, missed recall deadlines, and the ids injected
/// (ADR-0063 d6, d10). Never a prompt.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ChatCache {
    /// `sha256:<hex>` of turn `i + 1`'s prompt.
    pub turns: Vec<String>,
    /// The last turn a capture request covered.
    pub captured_through: u64,
    /// Capture requests made this session.
    pub captures: u32,
    /// A compaction was recorded since the last capture request.
    pub compacted: bool,
    /// Per-turn recalls that missed their deadline.
    pub misses: u32,
    /// Artifact ids per-turn recall injected.
    pub served: std::collections::BTreeSet<String>,
    /// The turns this session numbered before a resume (ADR-0063 d6).
    pub base: u64,
    /// Facts the capture requests have asked for so far (ADR-0063 d6).
    pub facts_asked: u32,
}

/// The turn counter a session keeps past `SessionEnd`, beside the cache:
/// `<id>.chat-turns`, the number of turns it numbered, so a resumed session
/// numbers on and its anchors and idempotency keys never repeat (ADR-0063
/// d6). It holds one number; files idle for 30 days are pruned.
fn counter_path(cache: &Path) -> PathBuf {
    cache.with_extension("chat-turns")
}

/// How long an idle turn counter is kept.
const COUNTER_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

impl ChatCache {
    /// The stored cache, or for a session that ended and resumed, an empty
    /// one numbering on from the turns it already numbered.
    #[must_use]
    pub fn load_or_resume(path: &Path) -> Self {
        if path.exists() {
            return Self::load(path);
        }
        let base = std::fs::read_to_string(counter_path(path))
            .ok()
            .and_then(|t| t.trim().parse::<u64>().ok())
            .unwrap_or(0);
        Self {
            base,
            captured_through: base,
            ..Self::default()
        }
    }

    /// The number of the session's latest turn.
    #[must_use]
    pub fn last_turn(&self) -> u64 {
        self.base + self.turns.len() as u64
    }

    /// The stored cache, empty when there is none; one that does not parse
    /// reads as spent, so a damaged file stops requests, never repeats them.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            Err(_) => Self::default(),
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(Self {
                captures: CAPTURES_MAX,
                misses: MISSES_MAX,
                ..Self::default()
            }),
        }
    }

    /// Writes the cache 0600 in a 0700 directory, by a rename.
    ///
    /// # Errors
    ///
    /// The directory or the file could not be written.
    pub fn store(&self, path: &Path) -> std::io::Result<()> {
        write_owner_only(path, &serde_json::to_vec(self)?)
    }
}

fn write_owner_only(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("the cache path has no directory"))?;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(dir)?;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    std::io::Write::write_all(&mut file, bytes)?;
    drop(file);
    std::fs::rename(&tmp, path)
}

/// The chat cache beside ADR-0038's: `oml/sessions/<id>.chat.json`.
#[must_use]
pub fn chat_cache_path(var: &dyn Fn(&str) -> Option<String>, session_id: &str) -> Option<PathBuf> {
    cache_path(var, session_id).map(|p| p.with_extension("chat.json"))
}

/// The salt file: `oml/chat-salt` under `XDG_CONFIG_HOME`, or
/// `HOME/.config` (ADR-0063 d5).
#[must_use]
pub fn salt_path(var: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let absolute = |v: String| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
    let base = var("XDG_CONFIG_HOME")
        .and_then(absolute)
        .or_else(|| var("HOME").and_then(absolute).map(|h| h.join(".config")))?;
    Some(base.join("oml").join("chat-salt"))
}

/// The salt, written once as 32 random bytes, owner-only.
///
/// # Errors
///
/// The file could not be read or written, or holds no 32 bytes.
pub fn salt(var: &dyn Fn(&str) -> Option<String>) -> Result<Vec<u8>, String> {
    let path = salt_path(var).ok_or("neither XDG_CONFIG_HOME nor HOME is set")?;
    match std::fs::read(&path) {
        Ok(bytes) if bytes.len() == 32 => Ok(bytes),
        Ok(_) => Err(format!("{} does not hold 32 bytes", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut bytes = vec![0_u8; 32];
            getrandom::getrandom(&mut bytes)
                .map_err(|e| format!("no randomness for the salt: {e}"))?;
            write_owner_only(&path, &bytes)
                .map_err(|e| format!("the chat salt could not be written: {e}"))?;
            Ok(bytes)
        }
        Err(e) => Err(format!("the chat salt could not be read: {e}")),
    }
}

/// The pseudonymous session a chat anchor names: the first 32 hex digits
/// of SHA-256(salt ‖ the harness's session id) (ADR-0063 d5).
#[must_use]
pub fn pseudonym(salt: &[u8], session_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(session_id.as_bytes());
    hex::encode(hasher.finalize())[..32].to_owned()
}

/// `oml-turn/1`: HMAC-SHA-256 of the prompt, UTF-8 after NFC, keyed by the
/// person's chat salt, which stays owner-only on this machine and is never
/// sent (ADR-0063 d4, d5).
#[must_use]
pub fn turn_hash(salt: &[u8], prompt: &str) -> String {
    let nfc: String = prompt.nfc().collect();
    format!("sha256:{}", hex::encode(hmac_sha256(salt, nfc.as_bytes())))
}

/// HMAC-SHA-256 (RFC 2104), so a turn's hash is keyed by the person's
/// salt and nobody without it can confirm or search a guessed prompt
/// against a signed anchor (ADR-0063 d4).
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut block = [0_u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let (mut inner_pad, mut outer_pad) = ([0x36_u8; 64], [0x5c_u8; 64]);
    for i in 0..64 {
        inner_pad[i] ^= block[i];
        outer_pad[i] ^= block[i];
    }
    let inner = Sha256::new()
        .chain_update(inner_pad)
        .chain_update(data)
        .finalize();
    Sha256::new()
        .chain_update(outer_pad)
        .chain_update(inner)
        .finalize()
        .into()
}

/// A switch: unset, empty or `0` is off; `1` is on; anything else is off
/// and named.
fn switch(var: &dyn Fn(&str) -> Option<String>, name: &str) -> Result<bool, String> {
    match var(name).filter(|v| !v.is_empty()).as_deref() {
        None | Some("0") => Ok(false),
        Some("1") => Ok(true),
        Some(other) => Err(format!("{name} is {other:?}; it takes 1 or 0")),
    }
}

/// The chat workarea: the plugin option, else `OML_CHAT_WORKAREA`.
#[must_use]
pub fn chat_workarea(var: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let set = |name: &str| var(name).filter(|v| !v.trim().is_empty());
    set(OPTION_WORKAREA).or_else(|| set(WORKAREA_VAR))
}

fn bounded(
    var: &dyn Fn(&str) -> Option<String>,
    name: &str,
    default: u64,
    max: u64,
) -> Result<u64, String> {
    match var(name).filter(|v| !v.is_empty()) {
        None => Ok(default),
        Some(v) => v
            .parse::<u64>()
            .ok()
            .filter(|n| (1..=max).contains(n))
            .ok_or_else(|| format!("{name} is {v:?}; it takes 1 to {max}")),
    }
}

type Object = serde_json::Map<String, Value>;

/// `oml hook user-prompt`, through the one dispatcher (ADR-0063 d6).
///
/// # Errors
///
/// As [`events::dispatch`].
pub fn user_prompt(
    var: &dyn Fn(&str) -> Option<String>,
    stdin: &mut dyn Read,
) -> Result<String, Quiet> {
    events::dispatch(Event::UserPromptSubmit, var, stdin)
}

/// `oml hook stop`, through the one dispatcher (ADR-0063 d6).
///
/// # Errors
///
/// As [`events::dispatch`].
pub fn stop(var: &dyn Fn(&str) -> Option<String>, stdin: &mut dyn Read) -> Result<String, Quiet> {
    events::dispatch(Event::Stop, var, stdin)
}

/// `oml hook pre-compact`, through the one dispatcher (ADR-0063 d6).
///
/// # Errors
///
/// As [`events::dispatch`].
pub fn pre_compact(
    var: &dyn Fn(&str) -> Option<String>,
    stdin: &mut dyn Read,
) -> Result<String, Quiet> {
    events::dispatch(Event::PreCompact, var, stdin)
}

/// The chat handler of `UserPromptSubmit`: counts the turn and records its hash when
/// capture or per-turn recall is on; with per-turn recall, injects what the
/// prompt's words recall, within the budget (ADR-0063 d6, d10).
///
/// # Errors
///
/// `Err(None)` when there is nothing to do; `Err(Some(why))` for a reason
/// the user may read on stderr. The turn goes on either way.
pub fn on_user_prompt(
    var: &dyn Fn(&str) -> Option<String>,
    object: &Object,
) -> Result<Reply, Quiet> {
    let capture = switch(var, CAPTURE_SWITCH).map_err(Some)?;
    let recall = switch(var, TURN_RECALL_SWITCH).map_err(Some)?;
    if !capture && !recall {
        return Err(None);
    }
    let (Some(Value::String(session_id)), Some(Value::String(prompt))) =
        (object.get("session_id"), object.get("prompt"))
    else {
        return Err(Some(
            "stdin carries no session_id and prompt, so the turn was not counted".to_owned(),
        ));
    };
    let path = chat_cache_path(var, session_id).ok_or_else(|| {
        Some("the session id is not a file name, or no cache directory is set".to_owned())
    })?;
    let salt = salt(var).map_err(Some)?;
    let mut cache = ChatCache::load_or_resume(&path);
    cache.turns.push(turn_hash(&salt, prompt));
    cache
        .store(&path)
        .map_err(|e| Some(format!("the chat cache could not be written: {e}")))?;
    if !recall {
        return Err(None);
    }
    turn_recall(var, prompt, &path, &mut cache).map(Reply::Context)
}

/// The per-turn recall (d10): two recalls under one deadline, nothing
/// injected when it passes, and three misses end it for the session.
fn turn_recall(
    var: &dyn Fn(&str) -> Option<String>,
    prompt: &str,
    path: &Path,
    cache: &mut ChatCache,
) -> Result<String, Quiet> {
    if cache.misses >= MISSES_MAX {
        return Err(None);
    }
    let budget = Duration::from_millis(
        bounded(var, BUDGET_VAR, BUDGET_DEFAULT_MS, BUDGET_MAX_MS).map_err(Some)?,
    );
    let Pair { url, key } = resolve_pair_for(var, "nothing was recalled").map_err(Some)?;
    let home = var("HOME").filter(|h| !h.is_empty());
    let words = prompt_words(prompt, &[key.as_str()], home.as_deref());
    if words.is_empty() {
        return Err(None);
    }
    let query = words.join(" ");
    let started = Instant::now();
    let mut scopes: Vec<(Option<String>, bool)> = Vec::with_capacity(2);
    if let Some(chat) = chat_workarea(var) {
        scopes.push((Some(chat), true));
    }
    scopes.push((None, false));
    let mut hits: Vec<Value> = Vec::new();
    // A miss is counted before anything is sent and taken back by an
    // answer in time, so a failed, refused or killed request counts (L3).
    cache.misses += 1;
    let _ = cache.store(path);
    for (workarea, unreviewed) in scopes {
        let Some(left) = budget
            .checked_sub(started.elapsed())
            .filter(|d| !d.is_zero())
        else {
            return miss(cache, path, budget);
        };
        let client = Client::for_hook(url.clone(), Some(key.clone()), left, "user-prompt");
        let mut body = json!({
            "query": query,
            "k": TURN_K,
            "match": "any",
            "include_text": true,
            "include_unreviewed": unreviewed,
            "max_tokens": TURN_MAX_TOKENS,
        });
        if let Some(w) = workarea {
            body["workarea"] = json!(w);
        }
        let opts = CallOpts {
            auth: true,
            idempotency: false,
        };
        match client.post("/v1/memories/recall", &body, &opts) {
            Ok((_, answer)) => {
                if let Some(list) = answer["hits"].as_array() {
                    hits.extend(list.iter().map(|h| h["memory"].clone()));
                }
            }
            Err(e) => {
                if started.elapsed() >= budget {
                    return miss(cache, path, budget);
                }
                return Err(Some(format!(
                    "the turn recall: {}, so nothing was injected ({} of {MISSES_MAX} misses this session)",
                    describe(e),
                    cache.misses
                )));
            }
        }
    }
    if started.elapsed() > budget {
        return miss(cache, path, budget);
    }
    let fresh: Vec<&Value> = hits
        .iter()
        .filter(|m| {
            m["artifact_id"]
                .as_str()
                .is_some_and(|id| !cache.served.contains(id))
        })
        .collect();
    cache.misses = cache.misses.saturating_sub(1);
    let mut seen = std::collections::BTreeSet::new();
    let fresh: Vec<&Value> = fresh
        .into_iter()
        .filter(|m| seen.insert(m["artifact_id"].as_str().unwrap_or_default().to_owned()))
        .collect();
    for m in &fresh {
        if let Some(id) = m["artifact_id"].as_str() {
            cache.served.insert(id.to_owned());
        }
    }
    cache
        .store(path)
        .map_err(|e| Some(format!("the chat cache could not be written: {e}")))?;
    if fresh.is_empty() {
        return Err(None);
    }
    let redact = |text: &str| text.replace(key.as_str(), "[OML_API_KEY]");
    Ok(render_turn(&fresh, &redact))
}

fn miss(cache: &ChatCache, _path: &Path, budget: Duration) -> Result<String, Quiet> {
    Err(Some(format!(
        "the turn recall missed its {} ms budget, so nothing was injected ({} of {MISSES_MAX} misses this session)",
        budget.as_millis(),
        cache.misses
    )))
}

/// The injected text: the frame, the may-be-irrelevant line, one line per
/// hit naming its id, origin kind, workarea and time, `[unreviewed]` when
/// held; each line at most 400 bytes and the whole at most 1,600 (d10).
#[must_use]
pub fn render_turn(hits: &[&Value], redact: &dyn Fn(&str) -> String) -> String {
    let field = |m: &Value, name: &str| one_line(&redact(m[name].as_str().unwrap_or("?")));
    let mut out = vec![FRAMING.to_owned(), TURN_LINE.to_owned()];
    for m in hits {
        let kind = m["origin"]["kind"].as_str().unwrap_or("none");
        let mark = if m["state"].as_str() == Some("held") {
            " [unreviewed]"
        } else {
            ""
        };
        let line = format!(
            "- {}{mark} {} ({kind}, workarea {}, recorded {}): {}",
            field(m, "artifact_id"),
            cut(field(m, "claim_key"), KEY_CAP),
            cut(field(m, "workarea_path"), KEY_CAP),
            field(m, "created_at"),
            field(m, "text"),
        );
        out.push(cut_bytes(line, TURN_LINE_CAP));
    }
    let mut text = out.join("\n");
    if text.len() > TURN_CONTEXT_CAP {
        text = cut_bytes(text, TURN_CONTEXT_CAP);
    }
    text
}

/// Up to eight distinctive words of a prompt, in order, each at least
/// three characters with a letter, no stop word, no repeat, and no word of
/// a secret's shape, the key's value or the home directory removed first
/// (ADR-0063 d10, by ADR-0038 d5's filter).
#[must_use]
pub fn prompt_words(prompt: &str, secrets: &[&str], home: Option<&str>) -> Vec<String> {
    let mut text = prompt.to_owned();
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        text = text.replace(secret, " ");
    }
    if let Some(home) = home.filter(|h| h.len() > 1) {
        text = text.replace(home, " ");
    }
    let mut out: Vec<String> = Vec::new();
    for chunk in text.split_whitespace() {
        if is_secret(chunk)
            || chunk.contains("://")
            || chunk.contains('=') && {
                let name = chunk.split('=').next().unwrap_or("").to_ascii_uppercase();
                ["PASS", "TOKEN", "SECRET", "KEY", "AUTH"]
                    .iter()
                    .any(|s| name.contains(s))
            }
        {
            continue;
        }
        for word in chunk.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            let lower = word.to_lowercase();
            if word.chars().count() < 3
                || word.chars().count() > 64
                || !word.chars().any(char::is_alphabetic)
                || is_secret(word)
                || crate::hook::STOP_WORDS.contains(&lower.as_str())
                || PROMPT_STOP_WORDS.contains(&lower.as_str())
                || out.iter().any(|w| w.to_lowercase() == lower)
            {
                continue;
            }
            out.push(word.to_owned());
            if out.len() == PROMPT_WORDS_MAX {
                return out;
            }
        }
    }
    out
}

/// Words a prompt asks with that name nothing.
const PROMPT_STOP_WORDS: &[&str] = &[
    "the", "and", "for", "you", "your", "can", "could", "would", "should", "please", "what", "how",
    "why", "when", "where", "which", "who", "this", "that", "these", "those", "with", "from",
    "into", "about", "have", "has", "had", "are", "was", "were", "will", "just", "now", "then",
    "than", "them", "they", "there", "here", "let", "lets", "make", "use", "using", "get", "got",
    "need", "want", "like", "also", "but", "not", "all", "any", "some", "our", "out", "its",
    "it's", "does", "did", "done", "one", "two", "yes", "okay", "thanks", "thank", "see",
];

/// The chat handler of `Stop`: when a capture is due, the block reason that asks
/// the running agent to write what the uncaptured turns established
/// (ADR-0063 d6). Never when `stop_hook_active`, never past the session's
/// budgets, and the cache records the request before it is printed.
///
/// # Errors
///
/// `Err(None)` when nothing is due; `Err(Some(why))` for a reason on stderr.
pub fn on_stop(var: &dyn Fn(&str) -> Option<String>, object: &Object) -> Result<Reply, Quiet> {
    if !switch(var, CAPTURE_SWITCH).map_err(Some)? {
        return Err(None);
    }
    let workarea = chat_workarea(var).ok_or_else(|| {
        Some(format!(
            "{CAPTURE_SWITCH}=1 but no chat workarea is named; run oml chat init and set {WORKAREA_VAR}, so nothing was captured"
        ))
    })?;
    let every = bounded(var, EVERY_VAR, EVERY_DEFAULT, EVERY_MAX).map_err(Some)?;
    if object.get("stop_hook_active") == Some(&Value::Bool(true)) {
        return Err(None);
    }
    let Some(Value::String(session_id)) = object.get("session_id") else {
        return Err(None);
    };
    let path = chat_cache_path(var, session_id).ok_or(None)?;
    let mut cache = ChatCache::load(&path);
    let last = cache.last_turn();
    let uncaptured = last.saturating_sub(cache.captured_through);
    // The facts asked so far are counted here, so no request asks past the
    // session's budget (ADR-0063 d6).
    let facts = FACTS_PER_CAPTURE.min(FACTS_PER_SESSION.saturating_sub(cache.facts_asked));
    let due = uncaptured > 0
        && facts > 0
        && cache.captures < CAPTURES_MAX
        && (uncaptured >= every || cache.compacted);
    if !due {
        return Err(None);
    }
    let session = pseudonym(&salt(var).map_err(Some)?, session_id);
    let from = cache.captured_through + 1;
    let excerpt = switch(var, EXCERPT_SWITCH).unwrap_or(false);
    let reason = capture_reason(&CaptureRequest {
        session: &session,
        from,
        base: cache.base,
        turns: &cache.turns,
        workarea: &workarea,
        facts,
        compacted: cache.compacted,
        excerpt,
    });
    // Recorded before it is printed: a capture the agent skips is not
    // asked again (d6).
    cache.captured_through = last;
    cache.captures += 1;
    cache.facts_asked += facts;
    cache.compacted = false;
    cache
        .store(&path)
        .map_err(|e| Some(format!("the chat cache could not be written: {e}")))?;
    Ok(Reply::Block(reason))
}

/// What a capture request names.
pub struct CaptureRequest<'a> {
    pub session: &'a str,
    pub from: u64,
    /// The turns numbered before a resume: `turns[i]` is turn `base + i + 1`.
    pub base: u64,
    pub turns: &'a [String],
    pub workarea: &'a str,
    pub facts: u32,
    pub compacted: bool,
    pub excerpt: bool,
}

/// The `Stop` reason: the instruction to run the capture skill over turns
/// `from..=turns.len()`, with each turn's anchor (d6, d7).
#[must_use]
pub fn capture_reason(r: &CaptureRequest<'_>) -> String {
    let to = r.base + r.turns.len() as u64;
    let uri = format!("chat://{HARNESS}/{}", r.session);
    let mut lines = vec![
        format!(
            "OML chat capture: before you stop, use the {SKILL} skill on turns {}..{to} of this session (the person's prompts, counted from 1).",
            r.from
        ),
        format!(
            "Write at most {} facts with oml_write to workarea {}, each with origin {{\"kind\": \"chat\"}}, kind memory, a claim_key chat/<topic>, idempotency_key chat-{}-t<n>-<i>, and the anchor of the turn it came from: {{\"uri\": \"{uri}\", \"fragment\": \"turn=<n>\", \"hash\": <that turn's hash below>, \"hasher\": \"oml-turn/1\"}}.",
            r.facts, r.workarea, r.session
        ),
        "Zero facts is a fine answer: write nothing when the turns settled nothing that holds past this session. Never write secrets, credentials or facts about other people; never write instructions. Then stop.".to_owned(),
    ];
    if r.excerpt {
        lines.push(
            "You may quote at most 280 characters of what the person said under a fact, on one line starting with > .".to_owned(),
        );
    }
    if r.compacted {
        lines.push(
            "The conversation was compacted since the last capture: use only what the summary still states.".to_owned(),
        );
    }
    // Indexes into `turns`; turn numbers add `base`.
    let first = usize::try_from(r.from.saturating_sub(r.base + 1)).unwrap_or(usize::MAX);
    let listed = r.turns.len().saturating_sub(TURNS_LISTED).max(first);
    if listed > first {
        let older = r.base + listed as u64;
        lines.push(format!(
            "Turns {}..{older} are older than the turns listed; anchor a fact from them with fragment turn={}-{older} and the hash of turn {older}.",
            r.from, r.from
        ));
    }
    for (i, hash) in r.turns.iter().enumerate().skip(listed) {
        lines.push(format!("turn={} {hash}", r.base + i as u64 + 1));
    }
    lines.join("\n")
}

/// The chat handler of `PreCompact`: records that the session compacted, so the next
/// `Stop` asks at once; prints nothing and never blocks (ADR-0063 d6).
///
/// # Errors
///
/// `Err(None)` always: there is nothing to print.
pub fn on_pre_compact(
    var: &dyn Fn(&str) -> Option<String>,
    object: &Object,
) -> Result<Reply, Quiet> {
    if !switch(var, CAPTURE_SWITCH).map_err(Some)? {
        return Err(None);
    }
    let Some(Value::String(session_id)) = object.get("session_id") else {
        return Err(None);
    };
    let path = chat_cache_path(var, session_id).ok_or(None)?;
    let mut cache = ChatCache::load(&path);
    cache.compacted = true;
    cache
        .store(&path)
        .map_err(|e| Some(format!("the chat cache could not be written: {e}")))?;
    Ok(Reply::Nothing)
}

/// `oml doctor`'s chat lines (ADR-0063 d13), from this machine alone: a
/// switch with a value it does not take; capture or per-turn recall on
/// with no chat workarea named; a salt file that is not 0600; sessions
/// whose per-turn recall missed its budget.
#[must_use]
pub fn doctor_lines(var: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut on = Vec::new();
    for name in [CAPTURE_SWITCH, TURN_RECALL_SWITCH, EXCERPT_SWITCH] {
        match switch(var, name) {
            Ok(true) => on.push(name),
            Ok(false) => {}
            Err(why) => out.push(format!("warn chat: {why}, so it is off")),
        }
    }
    let capture_or_recall = on.contains(&CAPTURE_SWITCH) || on.contains(&TURN_RECALL_SWITCH);
    match chat_workarea(var) {
        None if on.contains(&CAPTURE_SWITCH) => out.push(format!(
            "warn chat: {CAPTURE_SWITCH}=1 but no chat workarea is named, so nothing is captured; run oml chat init with this account's own key and set {WORKAREA_VAR}"
        )),
        None if capture_or_recall => out.push(format!(
            "warn chat: {TURN_RECALL_SWITCH}=1 recalls from the key's default workarea alone; run oml chat init and set {WORKAREA_VAR} to recall the chat facts too"
        )),
        Some(w) if capture_or_recall => out.push(format!("ok chat: {} on, workarea {w}", on.join(", "))),
        _ => {}
    }
    #[cfg(unix)]
    if let Some(path) = salt_path(var)
        && let Ok(meta) = std::fs::metadata(&path)
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o600 {
            out.push(format!(
                "warn chat: {} is mode {mode:o}, not 600; anyone who reads it can link the chat anchors to the sessions: chmod 600 it",
                path.display()
            ));
        }
    }
    if let Some(dir) = cache_path(var, "x").and_then(|p| p.parent().map(Path::to_path_buf))
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        let missed = entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".chat.json"))
            .filter(|e| ChatCache::load(&e.path()).misses > 0)
            .count();
        if missed > 0 {
            out.push(format!(
                "warn chat: per-turn recall missed its budget in {missed} open session(s); raise {BUDGET_VAR} (at most {BUDGET_MAX_MS}) or check the server's recall latency"
            ));
        }
    }
    if out.is_empty() {
        out.push("ok chat: capture and per-turn recall are off".to_owned());
    }
    out
}

/// The doctor line for a named chat workarea at `path`: a warning unless it
/// is `universe.<user>.chat` (ADR-0063 d8, d13).
#[must_use]
pub fn chat_scope_line(path: Option<&str>, user: Option<&str>) -> Option<String> {
    let path = path?;
    let user = user?;
    let theirs = format!("universe.{user}.chat");
    (path != theirs).then(|| {
        format!(
            "warn chat: the chat workarea is {path}, not {theirs}, the one oml chat init opens with this account's own key; point {WORKAREA_VAR} at {theirs}"
        )
    })
}

/// The doctor line for the server's answer to a listing filtered by
/// `origin_kinds`: a warning when it does not take the filter (ADR-0063 d13).
#[must_use]
pub fn origin_probe_line(status: u16, body: &Value) -> Option<String> {
    let refused = status == 400
        && body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("origin_kinds"));
    refused.then(|| {
        "warn chat: the server does not accept origin (it is older than v0.6.0), so chat facts are refused; upgrade the server".to_owned()
    })
}

/// Whether `oml chat init` may use an existing `chat` at `path` for
/// `user`: only the one directly under the user's root (ADR-0063 d8).
///
/// # Errors
///
/// The named reason it may not.
pub fn adopt(path: &str, user: &str) -> Result<(), String> {
    let theirs = format!("universe.{user}.chat");
    if path == theirs {
        Ok(())
    } else {
        Err(format!(
            "chat_scope_not_the_persons: the workarea named chat is {path}, not {theirs}, so it is not the chat workarea a person opens; oml chat init does not adopt it"
        ))
    }
}

/// Deletes a session's chat cache, for `oml hook session-end`.
pub fn forget_session(var: &dyn Fn(&str) -> Option<String>, session_id: &str) {
    let Some(path) = chat_cache_path(var, session_id) else {
        return;
    };
    // The digests go; the turn count stays, so a resume numbers on (M3).
    if path.exists() {
        let last = ChatCache::load(&path).last_turn();
        let _ = write_owner_only(&counter_path(&path), last.to_string().as_bytes());
    }
    let _ = std::fs::remove_file(&path);
    prune_counters(&path);
}

/// Removes turn counters idle past [`COUNTER_TTL`].
fn prune_counters(path: &Path) {
    let Some(dir) = path.parent() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let idle = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > COUNTER_TTL);
        if idle && entry.file_name().to_string_lossy().ends_with(".chat-turns") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{BufRead as _, BufReader, Write as _};

    struct Env {
        dir: PathBuf,
        vars: HashMap<String, String>,
    }

    impl Drop for Env {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    impl Env {
        fn new(pairs: &[(&str, &str)]) -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!("oml-cli-chat-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("scratch");
            let mut vars = HashMap::new();
            vars.insert("HOME".to_owned(), dir.display().to_string());
            for (k, v) in pairs {
                vars.insert((*k).to_owned(), (*v).to_owned());
            }
            Self { dir, vars }
        }
        fn var(&self) -> impl Fn(&str) -> Option<String> + '_ {
            move |name: &str| self.vars.get(name).cloned()
        }
    }

    fn prompt_input(session: &str, text: &str) -> Vec<u8> {
        prompt(session, text)
    }

    fn prompt(session: &str, text: &str) -> Vec<u8> {
        json!({"session_id": session, "prompt": text, "hook_event_name": "UserPromptSubmit"})
            .to_string()
            .into_bytes()
    }

    fn stop_input(session: &str, active: bool) -> Vec<u8> {
        json!({"session_id": session, "stop_hook_active": active, "hook_event_name": "Stop"})
            .to_string()
            .into_bytes()
    }

    const SALT_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn seeded(env: &Env) {
        let path = salt_path(&env.var()).expect("salt path");
        write_owner_only(&path, &hex::decode(SALT_HEX).expect("hex")).expect("salt");
    }

    #[test]
    fn doctor_names_capture_without_a_chat_workarea_and_a_bad_switch() {
        let env = Env::new(&[(CAPTURE_SWITCH, "1"), (TURN_RECALL_SWITCH, "on")]);
        let lines = doctor_lines(&env.var());
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("warn chat: OML_TURN_RECALL is \"on\"")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("oml chat init")),
            "{lines:?}"
        );
        let env = Env::new(&[]);
        assert_eq!(
            doctor_lines(&env.var()),
            vec!["ok chat: capture and per-turn recall are off"]
        );
    }

    #[test]
    fn the_pseudonym_is_32_hex_digits_that_depend_on_the_salt_and_never_the_raw_id() {
        let id = "00893aaf-19fa-41d2-8238-13269b9b3ca0";
        let a = pseudonym(&[1; 32], id);
        let b = pseudonym(&[2; 32], id);
        assert_eq!(a.len(), 32);
        assert!(
            a.bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_ne!(a, b);
        assert!(!a.contains("00893aaf"));
        assert_eq!(a, pseudonym(&[1; 32], id), "deterministic");
    }

    #[test]
    fn the_turn_hash_is_hmac_sha256_of_the_nfc_prompt_keyed_by_the_salt() {
        // "é" composed and decomposed hash alike.
        assert_eq!(turn_hash(b"k", "caf\u{e9}"), turn_hash(b"k", "cafe\u{301}"));
        assert_eq!(
            turn_hash(b"Jefe", "what do ya want for nothing?"),
            // RFC 4231 test case 2: HMAC-SHA-256 keyed by the salt (H2).
            "sha256:5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // The same prompt under two salts gives unrelated hashes, and
        // neither is the unkeyed digest anyone could recompute (H2).
        let prompt = "use squash merges";
        let plain = format!("sha256:{}", hex::encode(Sha256::digest(prompt.as_bytes())));
        let (a, b) = (turn_hash(&[1; 32], prompt), turn_hash(&[2; 32], prompt));
        assert_ne!(a, b);
        assert_ne!(a, plain);
        assert_ne!(b, plain);
    }

    /// The unkeyed SHA-256 of a prompt, which nothing may store or send.
    fn unkeyed(prompt: &str) -> String {
        let nfc: String = prompt.nfc().collect();
        hex::encode(Sha256::digest(nfc.as_bytes()))
    }

    #[test]
    fn no_unkeyed_digest_of_a_prompt_reaches_the_cache_or_the_stop_reason() {
        let env = Env::new(&[
            (CAPTURE_SWITCH, "1"),
            (WORKAREA_VAR, "chat"),
            (EVERY_VAR, "1"),
        ]);
        seeded(&env);
        let var = env.var();
        let prompt = "my address is 12 Elm Street";
        let _ = user_prompt(&var, &mut &prompt_input("s1", prompt)[..]);
        let reason = stop(&var, &mut &stop_input("s1", false)[..]).expect("due");
        let cache = std::fs::read_to_string(chat_cache_path(&var, "s1").expect("p")).expect("c");
        assert!(!reason.contains(&unkeyed(prompt)), "{reason}");
        assert!(!cache.contains(&unkeyed(prompt)), "{cache}");
    }

    /// A resumed session keeps numbering its turns where it stopped, so a
    /// turn's anchor and idempotency key never repeat (ADR-0063 d6; M3).
    #[test]
    fn turn_numbers_continue_across_a_resumed_session() {
        let env = Env::new(&[
            (CAPTURE_SWITCH, "1"),
            (WORKAREA_VAR, "chat"),
            (EVERY_VAR, "3"),
        ]);
        seeded(&env);
        let var = env.var();
        for n in 1..=3 {
            let _ = user_prompt(&var, &mut &prompt_input("s1", &format!("first {n}"))[..]);
        }
        let first = stop(&var, &mut &stop_input("s1", false)[..]).expect("due");
        assert!(first.contains("turns 1..3"), "{first}");
        crate::hook::session_end(
            &var,
            &mut &br#"{"session_id": "s1", "reason": "other"}"#[..],
        )
        .expect("session end");
        for n in 1..=3 {
            let _ = user_prompt(&var, &mut &prompt_input("s1", &format!("again {n}"))[..]);
        }
        let resumed = stop(&var, &mut &stop_input("s1", false)[..]).expect("due after resume");
        assert!(resumed.contains("turns 4..6"), "{resumed}");
        assert!(resumed.contains("turn=4 "), "{resumed}");
        assert!(!resumed.contains("turn=1 "), "{resumed}");
    }

    /// The facts the requests ask for are counted in the hook's state and
    /// never pass the session's budget, whatever the request count (M4).
    #[test]
    fn the_facts_asked_never_pass_the_session_budget() {
        let env = Env::new(&[
            (CAPTURE_SWITCH, "1"),
            (WORKAREA_VAR, "chat"),
            (EVERY_VAR, "1"),
        ]);
        seeded(&env);
        let var = env.var();
        let path = chat_cache_path(&var, "s1").expect("p");
        let spent = ChatCache {
            captures: 1,
            facts_asked: FACTS_PER_SESSION - 2,
            ..ChatCache::default()
        };
        spent.store(&path).expect("seed");
        let _ = user_prompt(&var, &mut &prompt_input("s1", "one")[..]);
        let last = stop(&var, &mut &stop_input("s1", false)[..]).expect("two facts left");
        assert!(last.contains("Write at most 2 facts"), "{last}");
        assert_eq!(ChatCache::load(&path).facts_asked, FACTS_PER_SESSION);
        let _ = user_prompt(&var, &mut &prompt_input("s1", "two")[..]);
        assert_eq!(
            stop(&var, &mut &stop_input("s1", false)[..]),
            Err(None),
            "budget spent"
        );
    }

    /// Any failed per-turn recall counts as a miss, not only a slow one, so
    /// an unreachable server stops being asked (L3).
    #[test]
    fn any_failed_turn_recall_counts_as_a_miss() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", closed.local_addr().expect("addr"));
        drop(closed);
        let env = Env::new(&[
            (TURN_RECALL_SWITCH, "1"),
            ("OML_URL", &url),
            ("OML_API_KEY", "oml_test_key"),
        ]);
        let var = env.var();
        for n in 1..=MISSES_MAX {
            let _ = user_prompt(
                &var,
                &mut &prompt_input("s4", "postgres replication lag")[..],
            );
            assert_eq!(
                ChatCache::load(&chat_cache_path(&var, "s4").expect("p")).misses,
                n
            );
        }
        assert_eq!(
            user_prompt(
                &var,
                &mut &prompt_input("s4", "postgres replication lag")[..]
            ),
            Err(None)
        );
    }

    #[test]
    fn doctor_names_a_chat_workarea_outside_the_persons_root_and_a_server_without_origin() {
        assert!(chat_scope_line(Some("universe.eugene.notes.chat"), Some("eugene")).is_some());
        assert!(chat_scope_line(Some("universe.mallory.chat"), Some("eugene")).is_some());
        assert_eq!(
            chat_scope_line(Some("universe.eugene.chat"), Some("eugene")),
            None
        );
        let old = json!({"error": {"code": "request.validation",
                         "message": "unknown query parameter \"origin_kinds\""}});
        assert!(origin_probe_line(400, &old).is_some());
        assert_eq!(origin_probe_line(200, &json!({"items": []})), None);
    }

    #[test]
    fn chat_init_adopts_only_the_chat_directly_under_the_persons_root() {
        let refused = adopt("universe.eugene.notes.chat", "eugene").expect_err("nested");
        assert!(refused.contains("chat_scope_not_the_persons"), "{refused}");
        assert!(adopt("universe.eugene.chat", "eugene").is_ok());
    }

    #[test]
    fn the_hooks_do_nothing_and_write_nothing_unless_switched_on() {
        let env = Env::new(&[]);
        let var = env.var();
        assert_eq!(
            user_prompt(&var, &mut &prompt("s1", "hello")[..]),
            Err(None)
        );
        assert_eq!(stop(&var, &mut &stop_input("s1", false)[..]), Err(None));
        assert_eq!(pre_compact(&var, &mut &b"{}"[..]), Err(None));
        assert!(!chat_cache_path(&var, "s1").expect("path").exists());
        let env = Env::new(&[(CAPTURE_SWITCH, "yes")]);
        assert!(matches!(
            user_prompt(&env.var(), &mut &prompt("s1", "x")[..]),
            Err(Some(why)) if why.contains("takes 1 or 0")
        ));
    }

    #[test]
    fn the_cache_holds_turn_digests_and_never_a_prompt() {
        let env = Env::new(&[(CAPTURE_SWITCH, "1"), (WORKAREA_VAR, "chat")]);
        let var = env.var();
        let secret_prompt = "my favourite colour is ultramarine";
        assert_eq!(
            user_prompt(&var, &mut &prompt("s1", secret_prompt)[..]),
            Err(None)
        );
        let path = chat_cache_path(&var, "s1").expect("path");
        let stored = std::fs::read_to_string(&path).expect("cache");
        assert!(!stored.contains("ultramarine"), "{stored}");
        let salt = salt(&var).expect("salt");
        assert_eq!(
            ChatCache::load(&path).turns,
            vec![turn_hash(&salt, secret_prompt)]
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    /// The golden capture request: five turns, then `Stop` asks once over
    /// turns 1..5 with each turn's anchor; the same `Stop` again, and a
    /// `Stop` while a stop hook is active, ask nothing (d6).
    #[test]
    fn the_stop_hook_asks_once_per_due_span_and_never_while_a_stop_hook_is_active() {
        let env = Env::new(&[
            (CAPTURE_SWITCH, "1"),
            (WORKAREA_VAR, "universe.eugene.chat"),
        ]);
        seeded(&env);
        let var = env.var();
        for n in 1..=4 {
            let _ = user_prompt(&var, &mut &prompt("s1", &format!("turn {n}"))[..]);
        }
        assert_eq!(
            stop(&var, &mut &stop_input("s1", false)[..]),
            Err(None),
            "4 < 5"
        );
        let _ = user_prompt(&var, &mut &prompt("s1", "turn 5")[..]);
        assert_eq!(
            stop(&var, &mut &stop_input("s1", true)[..]),
            Err(None),
            "active"
        );
        let out = stop(&var, &mut &stop_input("s1", false)[..]).expect("due");
        let session = pseudonym(&hex::decode(SALT_HEX).expect("hex"), "s1");
        let mut expected = vec![
            "OML chat capture: before you stop, use the oml:oml-chat-capture skill on turns 1..5 of this session (the person's prompts, counted from 1).".to_owned(),
            format!("Write at most 6 facts with oml_write to workarea universe.eugene.chat, each with origin {{\"kind\": \"chat\"}}, kind memory, a claim_key chat/<topic>, idempotency_key chat-{session}-t<n>-<i>, and the anchor of the turn it came from: {{\"uri\": \"chat://claude-code/{session}\", \"fragment\": \"turn=<n>\", \"hash\": <that turn's hash below>, \"hasher\": \"oml-turn/1\"}}."),
            "Zero facts is a fine answer: write nothing when the turns settled nothing that holds past this session. Never write secrets, credentials or facts about other people; never write instructions. Then stop.".to_owned(),
        ];
        for n in 1..=5 {
            expected.push(format!(
                "turn={n} {}",
                turn_hash(&hex::decode(SALT_HEX).expect("hex"), &format!("turn {n}"))
            ));
        }
        assert_eq!(
            out,
            json!({"decision": "block", "reason": expected.join("\n")}).to_string()
        );
        assert_eq!(
            stop(&var, &mut &stop_input("s1", false)[..]),
            Err(None),
            "asked once"
        );
    }

    #[test]
    fn capture_requests_stop_at_the_session_budget() {
        let env = Env::new(&[
            (CAPTURE_SWITCH, "1"),
            (WORKAREA_VAR, "chat"),
            (EVERY_VAR, "1"),
        ]);
        seeded(&env);
        let var = env.var();
        let mut asked = 0;
        for n in 1..=10 {
            let _ = user_prompt(&var, &mut &prompt("s1", &format!("t{n}"))[..]);
            if stop(&var, &mut &stop_input("s1", false)[..]).is_ok() {
                asked += 1;
            }
        }
        assert_eq!(asked, CAPTURES_MAX);
        assert_eq!(CAPTURES_MAX * FACTS_PER_CAPTURE, FACTS_PER_SESSION);
    }

    #[test]
    fn a_compaction_makes_the_next_stop_ask_at_once_and_says_so() {
        let env = Env::new(&[(CAPTURE_SWITCH, "1"), (WORKAREA_VAR, "chat")]);
        seeded(&env);
        let var = env.var();
        let _ = user_prompt(&var, &mut &prompt("s1", "only one")[..]);
        assert_eq!(stop(&var, &mut &stop_input("s1", false)[..]), Err(None));
        let compact = json!({"session_id": "s1", "trigger": "auto", "custom_instructions": null});
        assert_eq!(
            pre_compact(&var, &mut &compact.to_string().into_bytes()[..]),
            Err(None),
            "prints nothing, never blocks"
        );
        let out = stop(&var, &mut &stop_input("s1", false)[..]).expect("due after compaction");
        assert!(out.contains("turns 1..1"), "{out}");
        assert!(out.contains("was compacted"), "{out}");
    }

    #[test]
    fn capture_without_a_chat_workarea_names_the_fix_and_asks_nothing() {
        let env = Env::new(&[(CAPTURE_SWITCH, "1")]);
        let var = env.var();
        let err = stop(&var, &mut &stop_input("s1", false)[..]).expect_err("no workarea");
        assert!(err.expect("named").contains("oml chat init"));
    }

    #[test]
    fn older_turns_are_one_span_and_the_last_twelve_are_listed() {
        let turns: Vec<String> = (1..=20).map(|n| turn_hash(b"k", &n.to_string())).collect();
        let reason = capture_reason(&CaptureRequest {
            session: "0123456789abcdef0123456789abcdef",
            from: 3,
            base: 0,
            turns: &turns,
            workarea: "chat",
            facts: 6,
            compacted: false,
            excerpt: true,
        });
        assert!(
            reason.contains("fragment turn=3-8 and the hash of turn 8"),
            "{reason}"
        );
        assert!(reason.contains(&format!("turn=9 {}", turns[8])));
        assert!(!reason.contains("turn=8 sha256"));
        assert!(reason.contains("280 characters"));
    }

    #[test]
    fn prompt_words_drop_stop_words_secrets_urls_and_the_key() {
        let key = "oml_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ";
        let words = prompt_words(
            &format!(
                "How do I rotate the postgres password? key {key} TOKEN=hunter2 see https://u:p@db.example.com/x and ghp_0123456789abcdefABCDEF please use pg_rewind"
            ),
            &[key],
            Some("/home/dg"),
        );
        assert_eq!(
            words,
            vec!["rotate", "postgres", "password", "key", "pg_rewind"]
        );
    }

    /// A stub server that answers each request after `delay` with `hits`.
    type Seen = std::sync::Arc<std::sync::Mutex<Vec<Value>>>;

    fn stub(delay: Duration, hits: Value) -> String {
        stub_seen(delay, hits).0
    }

    /// A stub that also records each request body it was sent.
    fn stub_seen(delay: Duration, hits: Value) -> (String, Seen) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let seen: Seen = Seen::default();
        let record = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let hits = hits.clone();
                let record = record.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                            length = v.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0; length];
                    let _ = std::io::Read::read_exact(&mut reader, &mut body);
                    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                        record.lock().expect("lock").push(v);
                    }
                    std::thread::sleep(delay);
                    let answer = json!({"hits": hits}).to_string();
                    let mut stream = stream;
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                        answer.len()
                    );
                });
            }
        });
        (format!("http://{addr}"), seen)
    }

    /// The law of d10's scopes: per-turn recall reads promoted rows only,
    /// but for the opted-in chat workarea, whose unreviewed rows it labels
    /// (M2). Flipping the default scope's include_unreviewed fails it.
    #[test]
    fn per_turn_recall_reads_promoted_rows_only_outside_the_opted_in_chat_scope() {
        let (url, seen) = stub_seen(Duration::ZERO, json!([]));
        let env = Env::new(&[
            (TURN_RECALL_SWITCH, "1"),
            (WORKAREA_VAR, "universe.eugene.chat"),
            ("OML_URL", &url),
            ("OML_API_KEY", "oml_test_key"),
        ]);
        let _ = user_prompt(
            &env.var(),
            &mut &prompt("s5", "which editor keybindings")[..],
        );
        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen.len(), 2, "{seen:?}");
        let (chat, default) = (&seen[0], &seen[1]);
        assert_eq!(chat["workarea"], "universe.eugene.chat");
        assert_eq!(chat["include_unreviewed"], true);
        assert!(default.get("workarea").is_none(), "{default}");
        assert_eq!(default["include_unreviewed"], false, "{default}");
        for body in [chat, default] {
            assert_eq!(body["match"], "any");
            assert_eq!(body["k"], TURN_K);
            assert_eq!(body["max_tokens"], TURN_MAX_TOKENS);
        }
    }

    fn hit(id: &str, state: &str, kind: &str) -> Value {
        json!({"memory": {
            "artifact_id": id, "claim_key": "chat/editor", "workarea_path": "universe.eugene.chat",
            "created_at": "2026-11-05T10:00:00Z", "state": state, "origin": {"kind": kind},
            "text": "The person prefers vim keybindings."
        }})
    }

    #[test]
    fn per_turn_recall_injects_labelled_hits_within_its_budget() {
        let url = stub(Duration::ZERO, json!([hit("a1", "held", "chat")]));
        let env = Env::new(&[
            (TURN_RECALL_SWITCH, "1"),
            (WORKAREA_VAR, "universe.eugene.chat"),
            ("OML_URL", &url),
            ("OML_API_KEY", "oml_test_key"),
        ]);
        let var = env.var();
        let out = user_prompt(&var, &mut &prompt("s2", "which editor keybindings")[..])
            .expect("injected");
        let doc: Value = serde_json::from_str(&out).expect("json");
        let context = doc["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("context");
        assert_eq!(
            doc["hookSpecificOutput"]["hookEventName"],
            "UserPromptSubmit"
        );
        assert_eq!(
            context,
            format!(
                "{FRAMING}\n{TURN_LINE}\n- a1 [unreviewed] chat/editor (chat, workarea universe.eugene.chat, recorded 2026-11-05T10:00:00Z): The person prefers vim keybindings."
            )
        );
        assert!(context.len() <= TURN_CONTEXT_CAP);
        // The same hit is not injected twice in a session.
        assert_eq!(
            user_prompt(&var, &mut &prompt("s2", "editor keybindings again")[..]),
            Err(None)
        );
    }

    /// Three missed deadlines end per-turn recall for the session, and the
    /// call after them sends nothing. A miss is counted before a request is
    /// sent and the clock starts before the cache write (ADR-0063 d10), so
    /// on a runner slow enough to spend the budget first, a counted miss
    /// has no request: requests are at most one per miss, not exactly one
    /// (#312). The test asserts what the design fixes, the misses and the
    /// quiet after them, and no count that depends on the runner.
    #[test]
    fn per_turn_recall_is_skipped_on_timeout_and_stops_after_three_misses() {
        // The stub answers long after the budget, so an answer in time is
        // impossible however loaded the machine is.
        let (url, seen) = stub_seen(
            Duration::from_secs(5),
            json!([hit("a1", "promoted", "doc")]),
        );
        let env = Env::new(&[
            (TURN_RECALL_SWITCH, "1"),
            (BUDGET_VAR, "100"),
            ("OML_URL", &url),
            ("OML_API_KEY", "oml_test_key"),
        ]);
        let var = env.var();
        let sent = |seen: &Seen| seen.lock().expect("lock").len();
        for n in 1..=MISSES_MAX {
            let started = Instant::now();
            let out = user_prompt(&var, &mut &prompt("s3", "postgres replication lag")[..]);
            let took = started.elapsed();
            assert!(
                matches!(&out, Err(Some(why)) if why.contains("missed its 100 ms budget")),
                "{out:?}"
            );
            // Skipped, not waited out: the hook returned well before the answer.
            assert!(took < Duration::from_secs(4), "took {took:?}");
            assert_eq!(
                ChatCache::load(&chat_cache_path(&var, "s3").expect("p")).misses,
                n
            );
        }
        // Wait until the stub has read every request that was sent: the
        // count has stopped changing for a while.
        let mut settled = (sent(&seen), Instant::now());
        let deadline = Instant::now() + Duration::from_secs(10);
        while settled.1.elapsed() < Duration::from_millis(500) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            let now = sent(&seen);
            if now != settled.0 {
                settled = (now, Instant::now());
            }
        }
        let before = settled.0;
        assert!(
            before <= MISSES_MAX as usize,
            "at most one request per miss, got {before}"
        );
        assert_eq!(
            user_prompt(&var, &mut &prompt("s3", "postgres replication lag")[..]),
            Err(None),
            "off for the session"
        );
        // A request sent now would have missed the budget and answered
        // Err(Some(..)); the cache says no attempt began, and the stub, given
        // the time a request needs on loopback, has read no new one.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(sent(&seen), before, "the off-for-session call sent nothing");
        assert_eq!(
            ChatCache::load(&chat_cache_path(&var, "s3").expect("p")).misses,
            MISSES_MAX
        );
    }
}
