//! Writing and verifying the tar (d1, d7).
//!
//! The reader checks, in ADR-0084 d7's order: the layout, paths, sizes,
//! lines, nesting, duplicates and unreferenced bodies before it trusts any
//! content; then the file digests; then the manifest signature and its
//! tenant; then each version's text against its digest and its source event;
//! then each source event's signature.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::lines::{EdgeLine, EventLine, VerdictLine, VersionEntry, WorkspaceLine};
use crate::manifest::{FileEntry, Manifest, Mode, parse_public_key, parse_signature};
use crate::{
    BODIES_DIR, BundleError, FORMAT, INLINE_TEXT_MAX, LINE_FILES, MANIFEST, MAX_DEPTH,
    MAX_LINE_BYTES, State, sha256_digest,
};

/// The size limits of d7. Until measurement sets them they follow the
/// target's quota, which the caller hands in; [`Limits::default`] is the
/// CLI's (reading R9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The tar's total unpacked size.
    pub max_total_bytes: u64,
    /// The number of entries, the manifest included.
    pub max_entries: usize,
    /// One entry's size.
    pub max_entry_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_total_bytes: 1024 * 1024 * 1024,
            max_entries: 100_000,
            max_entry_bytes: 64 * 1024 * 1024,
        }
    }
}

/// What a reader checks against.
#[derive(Debug, Clone, Default)]
pub struct VerifyOptions {
    pub limits: Limits,
    /// `--expect-key <fingerprint>` (d5).
    pub expect_key: Option<String>,
}

/// A bundle's records, as the writer takes them and the reader returns them.
/// The manifest's `files` and `signature` are the writer's to fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleContents {
    pub manifest: Manifest,
    pub workspaces: Vec<WorkspaceLine>,
    pub versions: Vec<VersionEntry>,
    pub verdicts: Vec<VerdictLine>,
    pub events: Vec<EventLine>,
    pub edges: Vec<EdgeLine>,
    /// Bodies and attachments by lowercase hex SHA-256 of their bytes.
    pub bodies: BTreeMap<String, Vec<u8>>,
}

/// A bundle that passed every check of d7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedBundle {
    pub contents: BundleContents,
    /// The fingerprint of the key that signed the manifest.
    pub signer_fingerprint: String,
    /// True only when `expect_key` named that key: without it the signature
    /// proves integrity, not who holds the key (d5, d7).
    pub authenticated: bool,
}

/// Writes `contents` as an `oml-bundle/1` tar to `out`, signs the manifest
/// with `key`, and returns the signed manifest. The contents pass the same
/// record checks a reader runs, so a writer cannot make what a reader
/// refuses. The output is plaintext: encrypt it with [`crate::crypt`].
pub fn write_bundle<W: Write>(
    contents: &BundleContents,
    key: &SigningKey,
    out: W,
) -> Result<Manifest, BundleError> {
    write_bundle_with(
        contents,
        &key.verifying_key(),
        &|bytes| key.sign(bytes),
        out,
    )
}

/// [`write_bundle`] through a signer the caller holds, such as the server's
/// signer port, so the journal key never leaves it.
pub fn write_bundle_with<W: Write>(
    contents: &BundleContents,
    public: &VerifyingKey,
    sign: &dyn Fn(&[u8]) -> Signature,
    out: W,
) -> Result<Manifest, BundleError> {
    check_records(contents)?;
    let mut files: Vec<(String, Vec<u8>, Option<u64>)> = vec![
        lines_file(LINE_FILES[0], &contents.workspaces, to_value)?,
        lines_file(LINE_FILES[1], &contents.versions, VersionEntry::to_value)?,
        lines_file(LINE_FILES[2], &contents.verdicts, to_value)?,
        lines_file(LINE_FILES[3], &contents.events, to_value)?,
        lines_file(LINE_FILES[4], &contents.edges, to_value)?,
    ];
    for (hex, bytes) in &contents.bodies {
        files.push((format!("{BODIES_DIR}{hex}"), bytes.clone(), None));
    }
    let mut manifest = contents.manifest.clone();
    manifest.format = FORMAT.to_owned();
    manifest.files = files
        .iter()
        .map(|(path, bytes, lines)| FileEntry {
            path: path.clone(),
            bytes: bytes.len() as u64,
            lines: *lines,
            sha256: sha256_digest(bytes),
        })
        .collect();
    manifest.check_mode()?;
    let manifest = crate::sign_manifest_with(manifest, public, sign)?;
    let manifest_bytes = serde_json::to_vec(&manifest)
        .map_err(|_| BundleError::malformed(MANIFEST, 1, "manifest does not serialise"))?;

    let mut tar = tar::Builder::new(out);
    append(&mut tar, MANIFEST, &manifest_bytes)?;
    for (path, bytes, _) in &files {
        append(&mut tar, path, bytes)?;
    }
    tar.into_inner().map_err(|e| BundleError::io(&e))?;
    Ok(manifest)
}

fn to_value<T: Serialize>(line: &T) -> Value {
    serde_json::to_value(line).unwrap_or(Value::Null)
}

fn lines_file<T>(
    name: &str,
    lines: &[T],
    encode: impl Fn(&T) -> Value,
) -> Result<(String, Vec<u8>, Option<u64>), BundleError> {
    let mut bytes = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let text = serde_json::to_vec(&encode(line))
            .map_err(|_| BundleError::malformed(name, i + 1, "line does not serialise"))?;
        if text.len() > MAX_LINE_BYTES {
            return Err(BundleError::LineTooLong {
                file: name.to_owned(),
                line: i + 1,
            });
        }
        bytes.extend_from_slice(&text);
        bytes.push(b'\n');
    }
    Ok((name.to_owned(), bytes, Some(lines.len() as u64)))
}

/// One regular file, mode 0644, owner 0, mtime 0: the same contents always
/// make the same tar.
fn append<W: Write>(
    tar: &mut tar::Builder<W>,
    path: &str,
    bytes: &[u8],
) -> Result<(), BundleError> {
    let mut header = tar::Header::new_ustar();
    header.set_path(path).map_err(|e| BundleError::io(&e))?;
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    tar.append(&header, bytes).map_err(|e| BundleError::io(&e))
}

/// Reads and verifies an `oml-bundle/1` tar (decrypted) from `input`.
pub fn read_bundle<R: Read>(
    input: R,
    options: &VerifyOptions,
) -> Result<VerifiedBundle, BundleError> {
    let entries = read_entries(input, &options.limits)?;
    // The manifest first: an unknown format stops everything else (d1).
    let manifest_bytes = &entries[0].1;
    check_depth(manifest_bytes, MANIFEST, 1)?;
    let manifest_value: Value = serde_json::from_slice(manifest_bytes)
        .map_err(|e| BundleError::malformed(MANIFEST, 1, serde_reason(&e)))?;
    let format = manifest_value
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("");
    if format != FORMAT {
        return Err(BundleError::FormatUnknown {
            found: format.to_owned(),
        });
    }
    let manifest: Manifest = serde_json::from_value(manifest_value)
        .map_err(|e| BundleError::malformed(MANIFEST, 1, serde_reason(&e)))?;
    manifest.check_mode()?;

    let file = |name: &str| -> &[u8] {
        entries
            .iter()
            .find(|(path, _)| path == name)
            .map_or(&[][..], |(_, bytes)| bytes.as_slice())
    };
    let workspaces: Vec<WorkspaceLine> = parse_lines(LINE_FILES[0], file(LINE_FILES[0]), |v| {
        serde_json::from_value(v).map_err(|e| serde_reason(&e))
    })?;
    let versions: Vec<VersionEntry> =
        parse_lines(LINE_FILES[1], file(LINE_FILES[1]), VersionEntry::from_value)?;
    let verdicts: Vec<VerdictLine> = parse_lines(LINE_FILES[2], file(LINE_FILES[2]), from_value)?;
    let events: Vec<EventLine> = parse_lines(LINE_FILES[3], file(LINE_FILES[3]), from_value)?;
    let edges: Vec<EdgeLine> = parse_lines(LINE_FILES[4], file(LINE_FILES[4]), from_value)?;
    let mut bodies = BTreeMap::new();
    for (path, bytes) in &entries {
        if let Some(hex) = path.strip_prefix(BODIES_DIR) {
            bodies.insert(hex.to_owned(), bytes.clone());
        }
    }
    let contents = BundleContents {
        manifest,
        workspaces,
        versions,
        verdicts,
        events,
        edges,
        bodies,
    };
    check_records(&contents)?;

    // The file digests (d7, first of the ordered checks).
    check_files(&contents.manifest, &entries)?;
    // The manifest signature and its source tenant.
    let signer_fingerprint =
        crate::verify_manifest(&contents.manifest, options.expect_key.as_deref())?;
    // Each version's recomputed digest, against its line and its event.
    check_version_digests(&contents)?;
    // Each source event's signature.
    check_events(&contents)?;

    Ok(VerifiedBundle {
        contents,
        signer_fingerprint,
        authenticated: options.expect_key.is_some(),
    })
}

fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| serde_reason(&e))
}

/// A serde error's reason without the offending value, which may be content
/// (a refusal is printed and counted, never a place for text).
pub(crate) fn serde_reason(err: &serde_json::Error) -> String {
    let text = err.to_string();
    for prefix in ["unknown field", "missing field", "unknown variant"] {
        if text.starts_with(prefix) {
            return text.split(',').next().unwrap_or(prefix).to_owned();
        }
    }
    format!("{:?} error at column {}", err.classify(), err.column()).to_lowercase()
}

/// The layout walk: entry types, paths, order and limits (d1, d7).
fn read_entries<R: Read>(input: R, limits: &Limits) -> Result<Vec<(String, Vec<u8>)>, BundleError> {
    let mut archive = tar::Archive::new(input);
    let entries = archive
        .entries()
        .map_err(|e| BundleError::io(&e))?
        .raw(true);
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total: u64 = 0;
    for entry in entries {
        let mut entry = entry.map_err(|e| BundleError::layout(format!("unreadable tar: {e}")))?;
        let path = String::from_utf8_lossy(&entry.header().path_bytes()).into_owned();
        let kind = entry.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            return Err(BundleError::PathUnsafe {
                path,
                detail: "a link".to_owned(),
            });
        }
        check_path(&path)?;
        if !(kind.is_file()) {
            return Err(BundleError::layout(format!(
                "entry {path:?} is not a regular file"
            )));
        }
        if out.len() + 1 > limits.max_entries {
            return Err(BundleError::TooLarge {
                detail: format!("more than {} entries", limits.max_entries),
            });
        }
        let size = entry.size();
        if size > limits.max_entry_bytes {
            return Err(BundleError::TooLarge {
                detail: format!(
                    "entry {path:?} is {size} bytes, over {}",
                    limits.max_entry_bytes
                ),
            });
        }
        total = total.saturating_add(size);
        if total > limits.max_total_bytes {
            return Err(BundleError::TooLarge {
                detail: format!("unpacked size over {}", limits.max_total_bytes),
            });
        }
        check_order(&out, &path)?;
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(limits.max_entry_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| BundleError::io(&e))?;
        if bytes.len() as u64 != size {
            return Err(BundleError::layout(format!("entry {path:?} is truncated")));
        }
        out.push((path, bytes));
    }
    if out.len() < 1 + LINE_FILES.len() {
        let missing = std::iter::once(MANIFEST)
            .chain(LINE_FILES)
            .nth(out.len())
            .unwrap_or(MANIFEST);
        return Err(BundleError::layout(format!("{missing} is missing")));
    }
    Ok(out)
}

/// d7's unsafe paths: absolute, `..`, a backslash, or an empty or `.`
/// segment.
fn check_path(path: &str) -> Result<(), BundleError> {
    let unsafe_path = |detail: &str| {
        Err(BundleError::PathUnsafe {
            path: path.to_owned(),
            detail: detail.to_owned(),
        })
    };
    if path.starts_with('/') {
        return unsafe_path("an absolute path");
    }
    if path.contains('\\') {
        return unsafe_path("a backslash");
    }
    if path.split('/').any(|s| s == "..") {
        return unsafe_path("a .. segment");
    }
    if path.is_empty() || path.split('/').any(|s| s.is_empty() || s == ".") {
        return unsafe_path("an empty or . segment");
    }
    Ok(())
}

/// The fixed order (d1, reading R3): the manifest, the five line files, then
/// bodies by ascending digest.
fn check_order(seen: &[(String, Vec<u8>)], path: &str) -> Result<(), BundleError> {
    let fixed: Vec<&str> = std::iter::once(MANIFEST).chain(LINE_FILES).collect();
    if let Some(expected) = fixed.get(seen.len()) {
        if path == *expected {
            return Ok(());
        }
        return Err(BundleError::layout(format!(
            "entry {path:?} where {expected} belongs"
        )));
    }
    let Some(hex) = path.strip_prefix(BODIES_DIR) else {
        return Err(BundleError::layout(format!(
            "entry {path:?} is outside the layout"
        )));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(BundleError::layout(format!(
            "entry {path:?} is not bodies/ and 64 lowercase hex"
        )));
    }
    let previous = &seen[seen.len() - 1].0;
    if previous == path {
        return Err(BundleError::Duplicate {
            detail: format!("body {path:?} twice"),
        });
    }
    if previous.starts_with(BODIES_DIR) && previous.as_str() > path {
        return Err(BundleError::layout(format!("body {path:?} out of order")));
    }
    Ok(())
}

/// d7's nesting bound, counted over the raw bytes before any parse.
fn check_depth(bytes: &[u8], file: &str, line: usize) -> Result<(), BundleError> {
    let (mut depth, mut in_string, mut escaped) = (0_usize, false, false);
    for &b in bytes {
        if in_string {
            match (escaped, b) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(BundleError::Depth {
                        file: file.to_owned(),
                        line,
                    });
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

fn parse_lines<T>(
    file: &str,
    bytes: &[u8],
    parse: impl Fn(Value) -> Result<T, String>,
) -> Result<Vec<T>, BundleError> {
    let mut out = Vec::new();
    if bytes.is_empty() {
        return Ok(out);
    }
    if bytes.last() != Some(&b'\n') {
        return Err(BundleError::malformed(
            file,
            0,
            "the last line has no newline",
        ));
    }
    for (i, line) in bytes[..bytes.len() - 1].split(|&b| b == b'\n').enumerate() {
        let n = i + 1;
        if line.len() > MAX_LINE_BYTES {
            return Err(BundleError::LineTooLong {
                file: file.to_owned(),
                line: n,
            });
        }
        check_depth(line, file, n)?;
        let value: Value = serde_json::from_slice(line)
            .map_err(|e| BundleError::malformed(file, n, serde_reason(&e)))?;
        out.push(parse(value).map_err(|reason| BundleError::malformed(file, n, reason))?);
    }
    Ok(out)
}

fn is_digest(text: &str) -> Option<&str> {
    let hex = text.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))).then_some(hex)
}

/// The record checks a writer and a reader share: closed shapes the types
/// cannot say, duplicates, references and unreferenced bodies (d1–d4, d7,
/// d13, R1, R5, R7).
fn check_records(c: &BundleContents) -> Result<(), BundleError> {
    let versions_file = LINE_FILES[1];
    let mut ids = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    let events: BTreeSet<&str> = c.events.iter().map(|e| e.event_id.as_str()).collect();
    if events.len() != c.events.len() {
        return Err(BundleError::Duplicate {
            detail: "an event id twice".to_owned(),
        });
    }
    let mut used_events = BTreeSet::new();
    for (i, entry) in c.versions.iter().enumerate() {
        let n = i + 1;
        let (artifact_id, version) = entry.id();
        if !ids.insert((artifact_id.to_owned(), version)) {
            return Err(BundleError::Duplicate {
                detail: format!("artifact {artifact_id:?} version {version} twice"),
            });
        }
        let VersionEntry::Version(v) = entry else {
            continue;
        };
        let bad = |reason: &str| Err(BundleError::malformed(versions_file, n, reason));
        if c.manifest.mode == Mode::Release && v.state != State::Promoted {
            return bad("state_outside_release");
        }
        if is_digest(&v.plaintext_digest).is_none() {
            return bad("plaintext_digest is not sha256: and 64 hex");
        }
        match (&v.text, &v.body) {
            (Some(text), None) if text.len() > INLINE_TEXT_MAX => {
                return bad("text_over_inline_limit");
            }
            (Some(_), None) => {}
            (None, Some(body)) => {
                if body != &v.plaintext_digest {
                    return bad("body names a digest other than plaintext_digest");
                }
                referenced.insert(is_digest(body).unwrap_or_default().to_owned());
            }
            _ => return bad("exactly one of text and body"),
        }
        for attachment in &v.attachments {
            let Some(hex) = is_digest(&attachment.digest) else {
                return bad("attachment digest is not sha256: and 64 hex");
            };
            referenced.insert(hex.to_owned());
        }
        if let Some(event_id) = &v.source_event {
            if v.author.pseudonym != c.manifest.exporter_pseudonym {
                return bad("event_on_foreign_version");
            }
            if !events.contains(event_id.as_str()) {
                return bad("source_event names no carried event");
            }
            used_events.insert(event_id.as_str());
        }
    }
    if let Some(unused) = events.difference(&used_events).next() {
        return Err(BundleError::malformed(
            LINE_FILES[3],
            0,
            format!("event {unused:?} is referenced by no version"),
        ));
    }
    for hex in &referenced {
        if !c.bodies.contains_key(hex) {
            return Err(BundleError::layout(format!("{BODIES_DIR}{hex} is missing")));
        }
    }
    for (hex, bytes) in &c.bodies {
        if !referenced.contains(hex) {
            return Err(BundleError::UnreferencedBody {
                path: format!("{BODIES_DIR}{hex}"),
            });
        }
        if bytes.len() <= INLINE_TEXT_MAX
            && c.versions.iter().any(|e| {
                matches!(e, VersionEntry::Version(v) if v.body.as_deref().and_then(is_digest) == Some(hex.as_str()))
            })
        {
            return Err(BundleError::malformed(
                versions_file,
                0,
                format!("body {hex} is under the inline limit"),
            ));
        }
    }
    Ok(())
}

fn check_files(manifest: &Manifest, entries: &[(String, Vec<u8>)]) -> Result<(), BundleError> {
    let actual = &entries[1..];
    if manifest.files.len() != actual.len()
        || manifest
            .files
            .iter()
            .zip(actual)
            .any(|(f, (p, _))| &f.path != p)
    {
        return Err(BundleError::layout(
            "the manifest's file list is not the bundle's entries",
        ));
    }
    for (stated, (path, bytes)) in manifest.files.iter().zip(actual) {
        if stated.bytes != bytes.len() as u64 || stated.sha256 != sha256_digest(bytes) {
            return Err(BundleError::digest(format!(
                "{path} is not the file the manifest signed"
            )));
        }
        if let Some(hex) = path.strip_prefix(BODIES_DIR)
            && sha256_digest(bytes) != format!("sha256:{hex}")
        {
            return Err(BundleError::digest(format!(
                "{path} does not hash to its name"
            )));
        }
        let lines = path
            .ends_with(".jsonl")
            .then(|| bytes.iter().filter(|&&b| b == b'\n').count() as u64);
        if stated.lines != lines {
            return Err(BundleError::digest(format!("{path} line count differs")));
        }
    }
    Ok(())
}

fn check_version_digests(c: &BundleContents) -> Result<(), BundleError> {
    let events: BTreeMap<&str, &EventLine> =
        c.events.iter().map(|e| (e.event_id.as_str(), e)).collect();
    for entry in &c.versions {
        let VersionEntry::Version(v) = entry else {
            continue;
        };
        let text: &[u8] = match (&v.text, &v.body) {
            (Some(text), _) => text.as_bytes(),
            (None, Some(body)) => is_digest(body)
                .and_then(|hex| c.bodies.get(hex))
                .map_or(&[][..], Vec::as_slice),
            (None, None) => &[],
        };
        let id = format!("artifact {:?} version {}", v.artifact_id, v.version);
        if sha256_digest(text) != v.plaintext_digest {
            return Err(BundleError::digest(format!(
                "{id}: text does not hash to plaintext_digest"
            )));
        }
        if let Some(event) = v.source_event.as_deref().and_then(|e| events.get(e)) {
            let p = &event.payload;
            let matches = p.get("artifact_id").and_then(Value::as_str) == Some(&v.artifact_id)
                && p.get("version").and_then(Value::as_u64) == Some(v.version)
                && p.get("plaintext_digest").and_then(Value::as_str) == Some(&v.plaintext_digest);
            if !matches {
                return Err(BundleError::digest(format!(
                    "{id}: its source event signed another version"
                )));
            }
        }
    }
    Ok(())
}

fn check_events(c: &BundleContents) -> Result<(), BundleError> {
    let keys = c
        .manifest
        .event_keys
        .iter()
        .map(|k| {
            parse_public_key(k)
                .ok_or_else(|| BundleError::signature("an event key is not 32 bytes of hex"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for event in &c.events {
        let at = format!("event {:?}", event.event_id);
        if event.tenant_id != c.manifest.source.tenant_id {
            return Err(BundleError::signature(format!(
                "{at} belongs to another tenant"
            )));
        }
        let bytes = event
            .signed_bytes()
            .map_err(|_| BundleError::signature(format!("{at} has no canonical form")))?;
        if sha256_digest(&bytes) != event.hash {
            return Err(BundleError::signature(format!(
                "{at}: hash does not match its fields"
            )));
        }
        let sig = parse_signature(&event.sig).ok_or_else(|| {
            BundleError::signature(format!("{at}: sig is not ed25519: and 128 hex"))
        })?;
        if !keys.iter().any(|k| k.verify(&bytes, &sig).is_ok()) {
            return Err(BundleError::signature(format!(
                "{at}: signature verifies under no event key"
            )));
        }
    }
    Ok(())
}
