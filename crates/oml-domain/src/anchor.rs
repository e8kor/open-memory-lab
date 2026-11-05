//! A memory's source anchor: the origin its author declares, checked
//! against one grammar and never fetched (ADR-0057 d1-d3, d21).
//!
//! The anchor is part of the signed submission, immutable per version, and
//! clear metadata. This module holds the grammar alone: what a well-formed
//! anchor, filter and source check are, and which field a refusal names. It
//! computes nothing from an anchor.

use serde_json::{Map, Value};
use unicode_normalization::UnicodeNormalization;

/// The hashers the server admits (ADR-0057 d3, d7): `oml-md/1` in v0.5.8.
pub const HASHERS: [&str; 1] = ["oml-md/1"];

/// The longest `repo`, in characters (ADR-0057 d3).
pub const REPO_MAX: usize = 200;
/// The most segments a `repo` may have.
pub const REPO_SEGMENTS_MAX: usize = 8;
/// The longest `uri`, in bytes.
pub const URI_MAX: usize = 2048;
/// The longest `rev` with a `uri`.
pub const REV_MAX: usize = 128;
/// The longest `path` and `anchor_path_prefix`, in UTF-8 bytes.
pub const PATH_MAX_BYTES: usize = 1024;
/// The longest `fragment`, in characters.
pub const FRAGMENT_MAX: usize = 256;
/// The greatest line a line-span fragment may name (ADR-0057 d21).
pub const LINE_MAX: u64 = 10_000_000;

/// A malformed anchor, filter or check: the JSON pointer of the field it
/// names (`/anchor/path`, say) and why.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {reason}")]
pub struct AnchorError {
    pub field: String,
    pub reason: String,
}

fn refuse(field: &str, reason: impl Into<String>) -> AnchorError {
    AnchorError {
        field: field.to_owned(),
        reason: reason.into(),
    }
}

/// Where the anchored span lives: a file in a git repository at a commit,
/// or a page outside one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// `{repo, rev, path}`: `rev` is a full git object id.
    Repo {
        repo: String,
        rev: String,
        path: String,
    },
    /// `{uri, rev?}`: `rev` a page version or an `ETag`.
    Uri { uri: String, rev: Option<String> },
}

/// A version's anchor, as its author declared it (ADR-0057 d1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub origin: Origin,
    pub fragment: Option<String>,
    /// `(hash, hasher)`: both or neither.
    pub hash: Option<(String, String)>,
}

const MEMBERS: [&str; 7] = ["repo", "uri", "rev", "path", "fragment", "hash", "hasher"];

impl Anchor {
    /// Parses an anchor object under `base` (`/anchor`), refusing an
    /// unknown member, a missing or doubled origin, and any field outside
    /// the grammar, naming the field (ADR-0057 d3).
    ///
    /// # Errors
    ///
    /// [`AnchorError`] naming the first field that fails.
    pub fn from_json(value: &Value, base: &str) -> Result<Self, AnchorError> {
        let Some(object) = value.as_object() else {
            return Err(refuse(base, "an anchor is an object"));
        };
        for name in object.keys() {
            if !MEMBERS.contains(&name.as_str()) {
                return Err(refuse(
                    &format!("{base}/{name}"),
                    "not a member of the anchor",
                ));
            }
        }
        let text = |name: &str| -> Result<Option<&str>, AnchorError> {
            match object.get(name) {
                None => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.as_str())),
                Some(_) => Err(refuse(&format!("{base}/{name}"), "must be a string")),
            }
        };
        let field = |name: &str| format!("{base}/{name}");
        let origin = match (text("repo")?, text("uri")?) {
            (Some(_), Some(_)) => {
                return Err(refuse(&field("uri"), "exactly one of repo and uri"));
            }
            (None, None) => return Err(refuse(&field("repo"), "exactly one of repo and uri")),
            (Some(repo), None) => {
                check_repo(repo).map_err(|r| refuse(&field("repo"), r))?;
                let rev =
                    text("rev")?.ok_or_else(|| refuse(&field("rev"), "required with repo"))?;
                if !is_object_id(rev) {
                    return Err(refuse(
                        &field("rev"),
                        "a full git object id: 40 or 64 lower-case hex digits",
                    ));
                }
                let path =
                    text("path")?.ok_or_else(|| refuse(&field("path"), "required with repo"))?;
                check_path(path).map_err(|r| refuse(&field("path"), r))?;
                Origin::Repo {
                    repo: repo.to_owned(),
                    rev: rev.to_owned(),
                    path: path.to_owned(),
                }
            }
            (None, Some(uri)) => {
                check_uri(uri).map_err(|r| refuse(&field("uri"), r))?;
                if text("path")?.is_some() {
                    return Err(refuse(&field("path"), "refused with uri"));
                }
                let rev = text("rev")?;
                if let Some(rev) = rev
                    && (rev.is_empty()
                        || rev.len() > REV_MAX
                        || !rev.bytes().all(|b| b.is_ascii_graphic()))
                {
                    return Err(refuse(
                        &field("rev"),
                        "1 to 128 printable ASCII characters without whitespace",
                    ));
                }
                Origin::Uri {
                    uri: uri.to_owned(),
                    rev: rev.map(ToOwned::to_owned),
                }
            }
        };
        let fragment = text("fragment")?;
        if let Some(fragment) = fragment {
            check_fragment(fragment).map_err(|r| refuse(&field("fragment"), r))?;
        }
        let hash = match (text("hash")?, text("hasher")?) {
            (None, None) => None,
            (Some(_), None) => return Err(refuse(&field("hasher"), "required with hash")),
            (None, Some(_)) => return Err(refuse(&field("hash"), "required with hasher")),
            (Some(hash), Some(hasher)) => {
                if !is_digest(hash) {
                    return Err(refuse(
                        &field("hash"),
                        "sha256: and 64 lower-case hex digits",
                    ));
                }
                if !HASHERS.contains(&hasher) {
                    return Err(refuse(&field("hasher"), "one of oml-md/1"));
                }
                Some((hash.to_owned(), hasher.to_owned()))
            }
        };
        Ok(Self {
            origin,
            fragment: fragment.map(ToOwned::to_owned),
            hash,
        })
    }

    /// The anchor as served and journalled: its members in a fixed order,
    /// absent members omitted.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut object = Map::new();
        match &self.origin {
            Origin::Repo { repo, rev, path } => {
                object.insert("repo".into(), Value::from(repo.as_str()));
                object.insert("rev".into(), Value::from(rev.as_str()));
                object.insert("path".into(), Value::from(path.as_str()));
            }
            Origin::Uri { uri, rev } => {
                object.insert("uri".into(), Value::from(uri.as_str()));
                if let Some(rev) = rev {
                    object.insert("rev".into(), Value::from(rev.as_str()));
                }
            }
        }
        if let Some(fragment) = &self.fragment {
            object.insert("fragment".into(), Value::from(fragment.as_str()));
        }
        if let Some((hash, hasher)) = &self.hash {
            object.insert("hash".into(), Value::from(hash.as_str()));
            object.insert("hasher".into(), Value::from(hasher.as_str()));
        }
        Value::Object(object)
    }

    /// The repository, for a repository anchor.
    #[must_use]
    pub fn repo(&self) -> Option<&str> {
        match &self.origin {
            Origin::Repo { repo, .. } => Some(repo),
            Origin::Uri { .. } => None,
        }
    }

    /// The path, for a repository anchor.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        match &self.origin {
            Origin::Repo { path, .. } => Some(path),
            Origin::Uri { .. } => None,
        }
    }

    /// The fields the scan reads, with the pointer each names: `repo`,
    /// `uri`, `path` and `fragment` (ADR-0057 d2).
    #[must_use]
    pub fn scanned(&self) -> Vec<(&'static str, &str)> {
        let mut out = Vec::with_capacity(3);
        match &self.origin {
            Origin::Repo { repo, path, .. } => {
                out.push(("/anchor/repo", repo.as_str()));
                out.push(("/anchor/path", path.as_str()));
            }
            Origin::Uri { uri, .. } => out.push(("/anchor/uri", uri.as_str())),
        }
        if let Some(fragment) = &self.fragment {
            out.push(("/anchor/fragment", fragment.as_str()));
        }
        out
    }
}

/// `repo`'s grammar (ADR-0057 d3).
///
/// # Errors
///
/// Why it fails.
pub fn check_repo(repo: &str) -> Result<(), &'static str> {
    const WHY: &str =
        "lower case, 1 to 200 characters, 1 to 8 segments joined by /, each [a-z0-9][a-z0-9._-]*";
    if repo.is_empty() || repo.chars().count() > REPO_MAX {
        return Err(WHY);
    }
    let segments: Vec<&str> = repo.split('/').collect();
    if segments.len() > REPO_SEGMENTS_MAX {
        return Err(WHY);
    }
    for segment in segments {
        let mut bytes = segment.bytes();
        match bytes.next() {
            Some(b) if b.is_ascii_lowercase() || b.is_ascii_digit() => {}
            _ => return Err(WHY),
        }
        if !bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b)) {
            return Err(WHY);
        }
    }
    Ok(())
}

fn check_uri(uri: &str) -> Result<(), &'static str> {
    const WHY: &str =
        "an absolute https URL of at most 2,048 bytes, with no user information and no #";
    let Some(rest) = uri.strip_prefix("https://") else {
        return Err(WHY);
    };
    if uri.len() > URI_MAX
        || rest.is_empty()
        || rest.starts_with('/')
        || rest
            .chars()
            .any(|c| c == '@' || c == '#' || c.is_whitespace() || c.is_control())
    {
        return Err(WHY);
    }
    Ok(())
}

/// `path`'s grammar, which `anchor_path_prefix` shares but for its last
/// segment (ADR-0057 d3).
///
/// # Errors
///
/// Why it fails.
pub fn check_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() || path.len() > PATH_MAX_BYTES {
        return Err("1 to 1,024 UTF-8 bytes");
    }
    if path.nfc().collect::<String>() != path {
        return Err("must be NFC");
    }
    if path.starts_with('/') {
        return Err("a relative path: no leading /");
    }
    if path.contains('\\') || path.chars().any(char::is_control) {
        return Err("no backslash or control character");
    }
    if path
        .split('/')
        .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return Err("no empty, . or .. segment");
    }
    Ok(())
}

/// The `anchor_path_prefix` filter: at most 1,024 bytes, no control
/// character, no backslash and no leading `/` (ADR-0057 d4).
///
/// # Errors
///
/// Why it fails.
pub fn check_path_prefix(prefix: &str) -> Result<(), &'static str> {
    if prefix.is_empty() || prefix.len() > PATH_MAX_BYTES {
        return Err("1 to 1,024 UTF-8 bytes");
    }
    if prefix.starts_with('/') || prefix.contains('\\') || prefix.chars().any(char::is_control) {
        return Err("a relative path prefix: no leading /, backslash or control character");
    }
    Ok(())
}

/// `fragment`'s grammar: NFC, 1 to 256 characters, no whitespace, control
/// character or `#`; a line span `L<n>` or `L<n>-L<m>` well formed; `page=`
/// reserved (ADR-0057 d3, d21).
///
/// # Errors
///
/// Why it fails.
pub fn check_fragment(fragment: &str) -> Result<(), &'static str> {
    let count = fragment.chars().count();
    if count == 0 || count > FRAGMENT_MAX {
        return Err("1 to 256 characters");
    }
    if fragment.nfc().collect::<String>() != fragment {
        return Err("must be NFC");
    }
    if fragment
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '#')
    {
        return Err("no whitespace, control character or #");
    }
    if fragment.starts_with("page=") {
        return Err("page=<n> is reserved until a hasher reads PDFs");
    }
    let bytes = fragment.as_bytes();
    if bytes.len() > 1 && bytes[0] == b'L' && bytes[1].is_ascii_digit() {
        line_span(fragment).ok_or("a line span is L<n> or L<n>-L<m>, 1 <= n <= m <= 10000000")?;
    }
    Ok(())
}

/// The lines a line-span fragment names, `(n, m)`, or `None` when it is not
/// a well-formed one: no leading zero, `1 <= n <= m <= 10^7`.
#[must_use]
pub fn line_span(fragment: &str) -> Option<(u64, u64)> {
    let line = |s: &str| -> Option<u64> {
        let digits = s.strip_prefix('L')?;
        if digits.is_empty()
            || digits.starts_with('0')
            || !digits.bytes().all(|b| b.is_ascii_digit())
            || digits.len() > 8
        {
            return None;
        }
        let n: u64 = digits.parse().ok()?;
        (1..=LINE_MAX).contains(&n).then_some(n)
    };
    match fragment.split_once('-') {
        None => line(fragment).map(|n| (n, n)),
        Some((a, b)) => {
            let (n, m) = (line(a)?, line(b)?);
            (n <= m).then_some((n, m))
        }
    }
}

fn is_object_id(rev: &str) -> bool {
    (rev.len() == 40 || rev.len() == 64)
        && rev
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_digest(hash: &str) -> bool {
    hash.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Whether a path's file name has a secret's shape, the client's list
/// (ADR-0047 d3, ADR-0057 d6): `.env*`, `*.pem`, `*.key`, `id_*`,
/// `*credentials*` and `*secret*`, case-insensitively. The scan vetoes an
/// anchor naming one, since a written path is never erased (d2).
#[must_use]
pub fn secret_shaped(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    name.starts_with(".env")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_")
        || name.contains("credentials")
        || name.contains("secret")
}

/// What a source check found (ADR-0057 d5, d18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    Fresh,
    Equivalent,
    Moved,
    Changed,
    Removed,
}

impl CheckResult {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Equivalent => "equivalent",
            Self::Moved => "moved",
            Self::Changed => "changed",
            Self::Removed => "removed",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "fresh" => Self::Fresh,
            "equivalent" => Self::Equivalent,
            "moved" => Self::Moved,
            "changed" => Self::Changed,
            "removed" => Self::Removed,
            _ => return None,
        })
    }

    /// The `stale` mark this result leaves: `changed` and `removed` alone
    /// (ADR-0057 d5).
    #[must_use]
    pub const fn stale(self) -> Option<&'static str> {
        match self {
            Self::Changed => Some("changed"),
            Self::Removed => Some("removed"),
            Self::Fresh | Self::Equivalent | Self::Moved => None,
        }
    }
}

/// A source check's body, checked against the version's anchor: `rev` in
/// its origin's grammar, `hash` only where the anchor has a hasher, and
/// `path`/`fragment` for `moved` alone, `path` required then (ADR-0057 d5,
/// d18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCheckBody {
    pub rev: String,
    pub result: CheckResult,
    pub hash: Option<String>,
    pub moved_to: Option<(String, Option<String>)>,
}

impl SourceCheckBody {
    /// Checks a source check's fields against `anchor`.
    ///
    /// # Errors
    ///
    /// [`AnchorError`] naming the field (`/rev`, `/hash`, `/path`,
    /// `/fragment`).
    pub fn check(
        anchor: &Anchor,
        rev: &str,
        result: CheckResult,
        hash: Option<&str>,
        path: Option<&str>,
        fragment: Option<&str>,
    ) -> Result<Self, AnchorError> {
        match &anchor.origin {
            Origin::Repo { .. } if !is_object_id(rev) => {
                return Err(refuse(
                    "/rev",
                    "a full git object id: 40 or 64 lower-case hex digits",
                ));
            }
            Origin::Uri { .. }
                if rev.is_empty()
                    || rev.len() > REV_MAX
                    || !rev.bytes().all(|b| b.is_ascii_graphic()) =>
            {
                return Err(refuse(
                    "/rev",
                    "1 to 128 printable ASCII characters without whitespace",
                ));
            }
            _ => {}
        }
        if let Some(hash) = hash {
            if anchor.hash.is_none() {
                return Err(refuse("/hash", "the anchor has no hasher to hash with"));
            }
            if !is_digest(hash) {
                return Err(refuse("/hash", "sha256: and 64 lower-case hex digits"));
            }
        }
        let moved_to = if result == CheckResult::Moved {
            let Some(path) = path else {
                return Err(refuse("/path", "required with moved"));
            };
            if anchor.path().is_none() {
                return Err(refuse("/path", "a moved check needs a repository anchor"));
            }
            check_path(path).map_err(|r| refuse("/path", r))?;
            if let Some(fragment) = fragment {
                check_fragment(fragment).map_err(|r| refuse("/fragment", r))?;
            }
            Some((path.to_owned(), fragment.map(ToOwned::to_owned)))
        } else {
            if path.is_some() {
                return Err(refuse("/path", "refused unless result is moved"));
            }
            if fragment.is_some() {
                return Err(refuse("/fragment", "refused unless result is moved"));
            }
            None
        };
        Ok(Self {
            rev: rev.to_owned(),
            result,
            hash: hash.map(ToOwned::to_owned),
            moved_to,
        })
    }
}
