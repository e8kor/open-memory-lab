//! The offered skill set and the `oml://` resource URIs, as pure functions
//! (ADR-0080 d4, d5, d7, d10, d16; ADR-0081 d6). The core reads the rows and
//! the tree; this module decides which version answers which command, which
//! workspace wins a name shared along the chain, and how a URI reads. Nothing
//! here reads a store.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::claim_kinds::UNREVIEWED_SUFFIX;

/// One page of the offered set, as `GET /v1/skills`, `prompts/list` and
/// `resources/list` serve it: the listing's default page size (ADR-0080 d5).
pub const SKILL_PAGE: usize = 50;

/// The most held commands an offered set carries, counted after the sort
/// and dropped before any promoted command is, as the held guardrails are
/// capped (ADR-0080 d10).
pub const HELD_SKILLS_MAX: usize = 50;

/// The most index lines the session-start hook prints before "and N more"
/// (ADR-0080 d8).
pub const INDEX_MAX_LINES: usize = 20;

/// What a held skill's serve adds for its reviewers, the counterpart of the
/// guardrail's load warning (ADR-0080 d3).
pub const SKILL_LOAD_WARNING: &str = "every key that reads this workspace will be offered this skill as a command, here and below it unless its guardrail scope is workspace";

/// One skill version a scope may offer, before shadowing: a promoted tip,
/// or a held version the caller is served (its own, or the in-progress
/// audience's, ADR-0080 d10, d14), which the core has already judged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCandidate {
    pub artifact_id: Uuid,
    /// The skill's name, as its fields carry it.
    pub name: String,
    /// The workspace the version lives in, and that workspace's path.
    pub node: Uuid,
    pub path: String,
    /// The tree distance from the scope node, up or down: 0 on the scope
    /// node itself.
    pub distance: u32,
    /// Whether the node is the scope node or one of its ancestors; a node
    /// below the scope (inside an in-progress subtree) is not.
    pub ancestor: bool,
    /// A held version, served marked unreviewed.
    pub held: bool,
    pub version: u32,
    /// When the version was written, as the store signs it: the second key
    /// of the tie-break among held versions.
    pub created_at: String,
}

/// One offered command: the name a prompt and a resource use, the version
/// behind it, and the farther workspaces whose skill of the same name it
/// shadows, nearest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferedSkill {
    /// The skill's name, or `<name>-unreviewed` for a held edit of a
    /// promoted skill (ADR-0080 d10).
    pub command: String,
    pub candidate: SkillCandidate,
    pub overrides: Vec<String>,
}

/// The offered set: the commands offered, in d5's order, and every
/// workspace's commands before shadowing, which stay readable by URI (d5,
/// d7).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Offer {
    pub offered: Vec<OfferedSkill>,
    pub all: Vec<OfferedSkill>,
}

/// The commands one workspace's versions of one skill answer (ADR-0080
/// d10): the promoted tip under `<name>`, and a held edit newer than it under
/// `<name>-unreviewed`; with no promoted tip, the newest held version under
/// `<name>` itself. A held version no newer than the promoted tip answers
/// nothing. Held versions of one number go to the later created, then the
/// larger artifact id, so the answer never depends on read order.
fn commands(mut versions: Vec<SkillCandidate>) -> Vec<OfferedSkill> {
    versions.sort_by(|a, b| {
        (b.version, &b.created_at, b.artifact_id).cmp(&(a.version, &a.created_at, a.artifact_id))
    });
    let promoted = versions.iter().find(|c| !c.held).cloned();
    let held = versions.iter().find(|c| c.held).cloned();
    let command = |name: String, candidate: SkillCandidate| OfferedSkill {
        command: name,
        candidate,
        overrides: Vec::new(),
    };
    match (promoted, held) {
        (Some(p), Some(h)) if h.version > p.version => vec![
            command(p.name.clone(), p),
            command(format!("{}{UNREVIEWED_SUFFIX}", h.name), h),
        ],
        (Some(p), _) => vec![command(p.name.clone(), p)],
        (None, Some(h)) => vec![command(h.name.clone(), h)],
        (None, None) => Vec::new(),
    }
}

/// A place's rank for a shared name: tree distance, an ancestor before a
/// descendant, then the smaller path (ADR-0080 d5; R2).
fn rank(c: &SkillCandidate) -> (u32, bool, &str) {
    (c.distance, !c.ancestor, c.path.as_str())
}

/// The offered set from its candidates (ADR-0080 d4, d5, d10, d16). Each
/// workspace's versions of one name give its commands; a name held in more
/// than one workspace goes to the one nearest the scope node by tree
/// distance, up or down, an ancestor winning a tie with a descendant and,
/// between two descendants at one distance, the smaller path, so the answer
/// never depends on read order. A held version only adds: a workspace that
/// holds only held versions of a name never takes `<name>` while another
/// offered workspace holds it approved, and answers as `<name>-unreviewed`
/// beside it; the nearest such workspace, or the one that answers `<name>`
/// with a held edit, wins that command, and the rest stay readable by URI
/// (R15). The offered commands are ordered by distance, then command name,
/// and held commands past [`HELD_SKILLS_MAX`] are dropped from them in that
/// order, never a promoted one; `all` keeps every command.
#[must_use]
pub fn offer(candidates: Vec<SkillCandidate>) -> Offer {
    let mut slots: BTreeMap<(String, Uuid), Vec<SkillCandidate>> = BTreeMap::new();
    for c in candidates {
        slots.entry((c.name.clone(), c.node)).or_default().push(c);
    }
    let mut by_name: BTreeMap<String, Vec<Vec<OfferedSkill>>> = BTreeMap::new();
    for ((name, _), versions) in slots {
        let answered = commands(versions);
        if !answered.is_empty() {
            by_name.entry(name).or_default().push(answered);
        }
    }
    let mut offer = Offer::default();
    for (name, mut places) in by_name {
        places.sort_by(|a, b| rank(&a[0].candidate).cmp(&rank(&b[0].candidate)));
        let approved = places.iter().any(|p| p.iter().any(|o| !o.candidate.held));
        if approved {
            for place in places
                .iter_mut()
                .filter(|p| p.iter().all(|o| o.candidate.held))
            {
                for o in place {
                    o.command = format!("{name}{UNREVIEWED_SUFFIX}");
                }
            }
        }
        // The workspace whose `<name>` answers: the nearest approved one, or
        // the nearest of all when none is approved.
        let winner = places
            .iter()
            .position(|p| p.iter().any(|o| o.command == name));
        let mut answered: BTreeMap<String, usize> = BTreeMap::new();
        for (i, place) in places.into_iter().enumerate() {
            let held_only = place.iter().all(|o| o.candidate.held);
            for command in place {
                if command.command == name || held_only || Some(i) == winner {
                    match answered.get(&command.command) {
                        Some(&at) => offer.offered[at]
                            .overrides
                            .push(command.candidate.path.clone()),
                        None => {
                            answered.insert(command.command.clone(), offer.offered.len());
                            offer.offered.push(command.clone());
                        }
                    }
                }
                offer.all.push(command);
            }
        }
    }
    let order = |o: &OfferedSkill| (o.candidate.distance, o.command.clone());
    offer.offered.sort_by_key(order);
    offer.all.sort_by_key(order);
    // Only the offered list is capped: `all` is the URI lookup, read without
    // bodies, so every command it holds, an offered one included, stays
    // readable by its URI (R15, R19).
    cap_held(&mut offer.offered);
    offer
}

/// Keeps the first [`HELD_SKILLS_MAX`] held commands in order and every
/// promoted one.
fn cap_held(commands: &mut Vec<OfferedSkill>) {
    let mut held = 0;
    commands.retain(|o| {
        if !o.candidate.held {
            return true;
        }
        held += 1;
        held <= HELD_SKILLS_MAX
    });
}

/// A skill's resource URI (ADR-0080 d7; ADR-0081 d6): the workspace's name
/// and the command.
#[must_use]
pub fn skill_uri(workspace: &str, command: &str) -> String {
    format!("oml://workspace/{workspace}/skill/{command}")
}

/// A document's resource URI (ADR-0081 d4, d6): the workspace's name, the
/// claim key after `doc/`, `version=<n>` for one held version (the review's
/// reading R10) and `passage=<fragment>` for one passage, the fragment
/// [`oml_passage::uri_fragment`]'s (R11). Every character of a key in d2's
/// grammar is unreserved, so nothing is encoded.
#[must_use]
pub fn document_uri(
    workspace: &str,
    claim_key: &str,
    passage: Option<&str>,
    version: Option<u64>,
) -> String {
    let path = claim_key.strip_prefix("doc/").unwrap_or(claim_key);
    let query: Vec<String> = version
        .map(|v| format!("version={v}"))
        .into_iter()
        .chain(passage.map(|p| format!("passage={p}")))
        .collect();
    if query.is_empty() {
        format!("oml://workspace/{workspace}/doc/{path}")
    } else {
        format!("oml://workspace/{workspace}/doc/{path}?{}", query.join("&"))
    }
}

/// What an `oml://` URI names (ADR-0081 d6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceUri {
    Skill {
        workspace: String,
        name: String,
    },
    Document {
        workspace: String,
        /// The claim key after `doc/`.
        path: String,
        passage: Option<String>,
        /// One version, which a reader allowed to see it may read (R10).
        version: Option<u64>,
    },
}

/// Why a URI is not read (ADR-0081 d6): a `%` or a reserved character in a
/// segment is refused as a bad argument, JSON-RPC `-32602`; any other URI
/// outside the grammar answers as a missing resource does, `-32002`, so the
/// two never tell a row the key cannot read from one that is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UriRefusal {
    #[error("a URI segment holds a `%` or a reserved character")]
    Escaped,
    #[error("Resource not found")]
    NotFound,
}

/// RFC 3986's reserved characters, gen-delims and sub-delims.
const RESERVED: &str = ":/?#[]@!$&'()*+,;=";

/// Whether `segment` is one non-empty run of unreserved characters; a `%`
/// or a reserved character is [`UriRefusal::Escaped`].
fn segment(segment: &str) -> Result<&str, UriRefusal> {
    if segment.contains('%') || segment.chars().any(|c| RESERVED.contains(c)) {
        return Err(UriRefusal::Escaped);
    }
    let unreserved = segment
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~'));
    if segment.is_empty() || !unreserved {
        return Err(UriRefusal::NotFound);
    }
    Ok(segment)
}

/// Reads an `oml://workspace/<workspace>/skill/<name>` or
/// `oml://workspace/<workspace>/doc/<path>[?passage=<fragment>]` URI
/// (ADR-0081 d6). Every segment is unreserved text, matched as sent: nothing
/// is percent-decoded.
pub fn parse_resource_uri(uri: &str) -> Result<ResourceUri, UriRefusal> {
    let rest = uri
        .strip_prefix("oml://workspace/")
        .ok_or(UriRefusal::NotFound)?;
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    if query.is_some_and(|q| q.contains('#')) || path.contains('#') {
        return Err(UriRefusal::Escaped);
    }
    let mut parts = path.split('/');
    let workspace = segment(parts.next().unwrap_or_default())?.to_owned();
    match parts.next() {
        Some("skill") => {
            let name = segment(parts.next().ok_or(UriRefusal::NotFound)?)?.to_owned();
            if parts.next().is_some() || query.is_some() {
                return Err(UriRefusal::NotFound);
            }
            Ok(ResourceUri::Skill { workspace, name })
        }
        Some("doc") => {
            let segments: Vec<&str> = parts.map(segment).collect::<Result<_, _>>()?;
            if segments.is_empty() {
                return Err(UriRefusal::NotFound);
            }
            // `version=<n>` and `passage=<fragment>`, each at most once.
            let (mut passage, mut version) = (None, None);
            for pair in query.into_iter().flat_map(|q| q.split('&')) {
                if let Some(fragment) = pair.strip_prefix("passage=")
                    && passage.is_none()
                {
                    passage = Some(segment(fragment)?.to_owned());
                } else if let Some(n) = pair.strip_prefix("version=")
                    && version.is_none()
                {
                    let n = segment(n)?;
                    if !n.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(UriRefusal::NotFound);
                    }
                    version = Some(n.parse().map_err(|_| UriRefusal::NotFound)?);
                } else {
                    return Err(UriRefusal::NotFound);
                }
            }
            Ok(ResourceUri::Document {
                workspace,
                path: segments.join("/"),
                passage,
                version,
            })
        }
        _ => Err(UriRefusal::NotFound),
    }
}
