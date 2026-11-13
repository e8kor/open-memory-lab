//! The one workarea picker of the console (the design system's
//! `WorkareaField`): a `GET` text input with the browser's own hints, the
//! chosen line with Clear, and a Browse of the workareas directly under the
//! field, one bordered unit, with no script. It stands on Memories, the
//! Atlas and Review on both sides, and alone on Guardrails.
//!
//! Every workarea is named by its **reference** ([`reference()`]): its bare
//! name in the shared namespace (`platform`), `<user>.<name>` inside a
//! person's root (`alice.docs`, whatever its depth), the root's own name
//! (`alice`), or `universe`. Every field takes it on both sides, since a
//! qualified name resolves for every tenant key (ADR-0044 d5); a path
//! never is (ADR-0026 d7).
//!
//! The hints are names only, from the uncounted tree read
//! (`GET /v2/workareas/{root}/tree`, [`Names::read`]): one page of 200
//! on Memories and the Atlas ([`LIST_HINTS_PAGES`]), up to [`HINTS_MAX`] on
//! Guardrails. Counts and marks appear only inside an opened Browse,
//! which the page draws when its query carries `browse=1` and which then
//! makes the counted read (`?counts=true`, [`Browse::read`]); a closed
//! Browse is a link, so a page that does not ask for it makes no counted
//! read for the picker (the operator's decision of 2026-11-13 on the
//! design notes' row 14).
//!
//! An unknown name and an unreadable one get one refusal, with up to eight
//! closest names taken only from the names the viewer's read returned, so
//! the refusal names nothing Browse would not ([`Names::closest`]).

use std::collections::BTreeSet;
use std::fmt::Write as _;

use http::StatusCode;
use oml_page::web::{attr, glyph_svg, state_tag, text};
use oml_sdk::{CountedNode, encode};

use crate::Web;
use crate::atlas::Counts;
use crate::session::{PageCalls, call};

/// The page size of the tree read, the API's maximum.
const TREE_PAGE: u32 = 200;

/// The most tree pages the Guardrails hints read: 10 pages of 200, the
/// counted read's own cap of 2,000 nodes, so the hints and Browse hold the
/// same set. Past it the hint says so.
pub const HINTS_PAGES: usize = 10;

/// The tree pages the Memories and Atlas hints read: one, so the list's
/// page adds one call of a bounded cost (ADR-0070's as-built rows for the
/// redesign); past it the hint says the list is partial and that Browse
/// and a typed name still work.
pub const LIST_HINTS_PAGES: usize = 1;

/// The most names the hints hold.
pub const HINTS_MAX: usize = HINTS_PAGES * TREE_PAGE as usize;

/// The most closest names a refusal offers.
pub const CLOSEST_MAX: usize = 8;

/// The query parameter that opens Browse.
pub const BROWSE: &str = "browse";

/// The reference of the workarea at `path`, where `is_user_root` says
/// whether a label under `universe` names a person's root: `universe`, a
/// top-level name, `<user>.<name>` beneath a user root, or the bare name
/// beneath a shared workarea. A path alone cannot tell
/// `universe.platform.oncall` (`oncall`) from `universe.alice.docs`
/// (`alice.docs`), so the caller says which top labels are user roots.
#[must_use]
pub fn reference(path: &str, is_user_root: impl Fn(&str) -> bool) -> String {
    let labels: Vec<&str> = path.split('.').collect();
    match labels.as_slice() {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [_, top] => (*top).to_owned(),
        [_, top, .., last] if is_user_root(top) => format!("{top}.{last}"),
        [.., last] => (*last).to_owned(),
    }
}

/// The top labels under `universe` a page knows the kind of: a person's
/// root, which makes the names beneath it `<user>.<name>`, or a shared
/// workarea, whose descendants are named by their bare name (design notes
/// row 20). A path alone cannot tell `universe.platform.oncall` (`oncall`)
/// from `universe.alice.docs` (`alice.docs`), so a page fills this from
/// what it read: the tree read's `user_root` marks, the person's own root,
/// and, for a top label none of those names, one read of that label's
/// node ([`Roots::learn`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roots {
    user: BTreeSet<String>,
    shared: BTreeSet<String>,
}

impl Roots {
    /// The kinds a tree read's nodes mark: every node marked a user root,
    /// and every node directly under `universe` not marked one.
    fn of_nodes<'a>(nodes: impl IntoIterator<Item = (&'a str, &'a str, bool)>) -> Self {
        let mut roots = Self::default();
        for (name, path, user_root) in nodes {
            if user_root {
                roots.user.insert(name.to_owned());
            } else if path.split('.').count() == 2 {
                roots.shared.insert(name.to_owned());
            }
        }
        roots
    }

    /// The person's own root, `name`, where the page knows it.
    #[must_use]
    pub fn with_own(mut self, name: Option<&str>) -> Self {
        if let Some(name) = name {
            self.shared.remove(name);
            self.user.insert(name.to_owned());
        }
        self
    }

    /// The reference of `path` by these kinds; a top label they do not
    /// know is read as a shared workarea's.
    #[must_use]
    pub fn reference(&self, path: &str) -> String {
        reference(path, |l| self.user.contains(l))
    }

    /// Whether `name` is a person's root.
    #[must_use]
    pub fn is_user_root(&self, name: &str) -> bool {
        self.user.contains(name)
    }

    /// The top label of `path` whose kind changes its reference and that
    /// these kinds do not know: one under `universe`, with a workarea
    /// beneath it.
    fn unknown<'a>(&self, path: &'a str) -> Option<&'a str> {
        let mut labels = path.split('.');
        let (Some("universe"), Some(top), Some(_)) = (labels.next(), labels.next(), labels.next())
        else {
            return None;
        };
        (!self.user.contains(top) && !self.shared.contains(top)).then_some(top)
    }

    /// These kinds with every top label of `paths` they do not know read
    /// once, as `credential`, by its node's first tree row
    /// (`GET /v2/workareas/{top}/tree?limit=1`), whose `user_root` mark
    /// says which it is; the reads run together. A page whose reads name
    /// only the person's root and `universe` makes none. A label the key
    /// cannot read, or whose read fails, stays shared: a person's review
    /// key reaches another person's root only by a grant on that root,
    /// which reads it (ADR-0045 d10).
    pub async fn learn<'a>(
        mut self,
        web: &Web,
        credential: &str,
        calls: &PageCalls,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let mut tops: Vec<String> = Vec::new();
        for path in paths {
            if let Some(top) = self.unknown(path)
                && !tops.iter().any(|t| t == top)
            {
                tops.push(top.to_owned());
            }
        }
        let reads = tops.iter().map(|top| async move {
            let ids = calls.next_ids();
            let first = web
                .api
                .workarea_tree(&call(credential, &ids), top, 1, None)
                .await;
            let user_root = first.ok().is_some_and(|page| {
                page.items
                    .first()
                    .is_some_and(|n| n.user_root && n.name == *top)
            });
            (top.clone(), user_root)
        });
        for (top, user_root) in join_all(reads.collect()).await {
            if user_root {
                self.user.insert(top);
            } else {
                self.shared.insert(top);
            }
        }
        self
    }
}

/// Every future of `futures`, polled together, their outputs in order.
async fn join_all<F: Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut futures: Vec<std::pin::Pin<Box<F>>> = futures.into_iter().map(Box::pin).collect();
    let mut out: Vec<Option<F::Output>> = futures.iter().map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut pending = false;
        for (future, slot) in futures.iter_mut().zip(out.iter_mut()) {
            if slot.is_none() {
                match future.as_mut().poll(cx) {
                    std::task::Poll::Ready(value) => *slot = Some(value),
                    std::task::Poll::Pending => pending = true,
                }
            }
        }
        if pending {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(())
        }
    })
    .await;
    out.into_iter().flatten().collect()
}

/// One workarea of the tree read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub path: String,
    pub reference: String,
}

/// The names the viewer may choose, from the uncounted tree read: each
/// node's id, name, path and reference, and whether the read stopped at
/// its page cap.
#[derive(Debug, Clone, Default)]
pub struct Names {
    places: Vec<Place>,
    roots: Roots,
    /// The read stopped at the cap: a name not here may still exist.
    more: bool,
    /// The read failed: the request id, and nothing is known.
    unavailable: Option<String>,
    /// Built from another read than the tree's: a name not here may exist.
    partial: bool,
}

impl Names {
    /// The names under `root` as `credential` reads them: the tree a page
    /// at a time, at most `pages` pages. A failed call, or a
    /// cursor the API repeats, leaves the names unavailable with the
    /// request id of the call that ended the read; `own_root` names the
    /// person's root where the page knows it, so a path under it gets its
    /// reference even when the read did not list the root.
    pub async fn read(
        web: &Web,
        credential: &str,
        calls: &PageCalls,
        root: &str,
        own_root: Option<&str>,
        pages: usize,
    ) -> Self {
        let mut nodes = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen = BTreeSet::new();
        let mut more = false;
        for page in 0..pages {
            let ids = calls.next_ids();
            let request_id = ids.request_id.clone().unwrap_or_default();
            let read = web
                .api
                .workarea_tree(&call(credential, &ids), root, TREE_PAGE, cursor.as_deref())
                .await;
            let read = match read {
                Ok(read) => read,
                Err(_) => return Self::unavailable(request_id),
            };
            nodes.extend(read.items);
            match read.next_cursor {
                None => break,
                Some(next) if !seen.insert(next.clone()) => {
                    return Self::unavailable(request_id);
                }
                Some(_) if page + 1 == pages => more = true,
                Some(next) => cursor = Some(next),
            }
        }
        let roots = Roots::of_nodes(
            nodes
                .iter()
                .map(|n| (n.name.as_str(), n.path.as_str(), n.user_root)),
        )
        .with_own(own_root);
        let places = nodes
            .into_iter()
            .map(|n| Place {
                reference: roots.reference(&n.path),
                id: n.id,
                name: n.name,
                path: n.path,
            })
            .collect();
        Self {
            places,
            roots,
            more,
            ..Self::default()
        }
    }

    /// Names a failed read leaves: none, with the request id.
    #[must_use]
    pub fn unavailable(request_id: String) -> Self {
        Self {
            unavailable: Some(request_id),
            ..Self::default()
        }
    }

    /// Names from a counted tree the page already read under `universe`:
    /// every workarea the viewer reads unless the read was cut at its cap.
    #[must_use]
    pub fn of_counted(nodes: &[CountedNode], truncated: bool) -> Self {
        let roots = Roots::of_nodes(
            nodes
                .iter()
                .map(|n| (n.name.as_str(), n.path.as_str(), n.user_root)),
        );
        let places = nodes
            .iter()
            .map(|n| Place {
                reference: roots.reference(&n.path),
                id: n.id.clone(),
                name: n.name.clone(),
                path: n.path.clone(),
            })
            .collect();
        Self {
            places,
            roots,
            more: truncated,
            ..Self::default()
        }
    }

    /// Names from a read the page already made, `(id, path)` each, with
    /// the kinds of top label it knows, and no call. Such a set is never
    /// taken as every workarea the viewer reads, so a name it lacks is
    /// left to the API.
    #[must_use]
    pub fn of_places(places: impl IntoIterator<Item = (String, String)>, roots: Roots) -> Self {
        let mut seen = BTreeSet::new();
        let places = places
            .into_iter()
            .filter(|(id, _)| seen.insert(id.clone()))
            .map(|(id, path)| Place {
                reference: roots.reference(&path),
                name: path.rsplit('.').next().unwrap_or_default().to_owned(),
                id,
                path,
            })
            .collect();
        Self {
            places,
            roots,
            partial: true,
            ..Self::default()
        }
    }

    /// The names of `places`, as a read would hold them.
    #[cfg(test)]
    fn of(paths: &[(&str, &str, bool)]) -> Self {
        let roots =
            Roots::of_nodes(paths.iter().map(|(_, path, root)| {
                (path.rsplit('.').next().unwrap_or_default(), *path, *root)
            }));
        let places = paths
            .iter()
            .map(|(id, path, _)| Place {
                id: (*id).to_owned(),
                name: path.rsplit('.').next().unwrap_or_default().to_owned(),
                path: (*path).to_owned(),
                reference: roots.reference(path),
            })
            .collect();
        Self {
            places,
            roots,
            ..Self::default()
        }
    }

    /// Every workarea read, a parent before its children.
    #[must_use]
    pub fn places(&self) -> &[Place] {
        &self.places
    }

    /// The reference of `path`, with the user roots this read found.
    #[must_use]
    pub fn reference(&self, path: &str) -> String {
        self.roots.reference(path)
    }

    /// Whether `name` is a person's root.
    #[must_use]
    pub fn is_user_root(&self, name: &str) -> bool {
        self.roots.is_user_root(name)
    }

    /// The kinds of top label this read knows.
    #[must_use]
    pub fn roots(&self) -> &Roots {
        &self.roots
    }

    /// Whether the read holds every workarea the viewer reads, so a name
    /// it lacks is one the viewer cannot read.
    fn whole(&self) -> bool {
        !self.more && !self.partial && self.unavailable.is_none()
    }

    /// The workarea `typed` names: by id, by reference, or by a bare name
    /// only one workarea read has, which is how `docs` names `alice.docs`
    /// on the admin page as it does on the person's (ADR-0044 d5 resolves a
    /// bare name in the person's root for their keys only).
    #[must_use]
    pub fn find(&self, typed: &str) -> Option<&Place> {
        if let Some(place) = self
            .places
            .iter()
            .find(|p| p.id == typed || p.reference == typed)
        {
            return Some(place);
        }
        if typed.contains('.') {
            return None;
        }
        let mut named = self.places.iter().filter(|p| p.name == typed);
        match (named.next(), named.next()) {
            (Some(one), None) => Some(one),
            _ => None,
        }
    }

    /// Up to [`CLOSEST_MAX`] references near `typed`, from this read only:
    /// those whose last label starts with the typed last label, then those
    /// that hold it, then those within two edits of it, ties by name. A
    /// path offers the names of its labels after the first, last label
    /// first.
    #[must_use]
    pub fn closest(&self, typed: &str) -> Vec<String> {
        let labels: Vec<&str> = typed.split('.').filter(|l| !l.is_empty()).collect();
        let wanted: Vec<&str> = if labels.len() > 2 {
            labels[1..].iter().rev().copied().collect()
        } else {
            labels.last().copied().into_iter().collect()
        };
        let mut out: Vec<String> = Vec::new();
        for label in wanted {
            let last = |p: &Place| {
                p.reference
                    .rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            };
            let mut sorted: Vec<&Place> = self.places.iter().collect();
            sorted.sort_by(|a, b| a.reference.cmp(&b.reference));
            let tier = |p: &Place, n: u8| match n {
                0 => last(p).starts_with(label),
                1 => p.reference.contains(label),
                _ => edits(&last(p), label) <= 2,
            };
            for n in 0..3 {
                for p in sorted.iter().filter(|p| tier(p, n)) {
                    if out.len() < CLOSEST_MAX && !out.contains(&p.reference) {
                        out.push(p.reference.clone());
                    }
                }
            }
        }
        out
    }

    /// `chosen` settled against this read: a value the API served becomes
    /// the place it names where the read holds it, and a refusal gets its
    /// closest names.
    #[must_use]
    pub fn settle(&self, chosen: Chosen) -> Chosen {
        match chosen {
            Chosen::Ask(typed) => match self.find(&typed) {
                Some(place) => Chosen::Place(place.clone()),
                None => Chosen::Ask(typed),
            },
            Chosen::Miss(miss) => Chosen::Miss(Miss {
                closest: self.closest(&miss.typed),
                ..miss
            }),
            other => other,
        }
    }

    /// The answer to `chosen` before any call: a value this read holds is
    /// the place it names, and a value the read lacks, when it holds every
    /// workarea the viewer reads, is refused without a call; otherwise the
    /// API decides.
    #[must_use]
    pub fn resolve(&self, chosen: Chosen) -> Chosen {
        match chosen {
            Chosen::Ask(typed) => match self.find(&typed) {
                Some(place) => Chosen::Place(place.clone()),
                None if self.whole() => self.miss(&typed),
                None => Chosen::Ask(typed),
            },
            other => self.settle(other),
        }
    }

    /// What a page does when the API answered `access.not_found` to the
    /// read it made with `chosen`'s value: a bare name the read holds
    /// under a person's root is asked again by its place
    /// (`Some(Chosen::Place)`), a value the read lacks is the field's miss,
    /// and a value the read holds as sent is readable, so the refusal was
    /// for another field of the request (`None`).
    #[must_use]
    pub fn not_found(&self, chosen: &Chosen) -> Option<Chosen> {
        match chosen {
            Chosen::Ask(typed) => match self.find(typed) {
                Some(place) if place.reference != *typed && place.id != *typed => {
                    Some(Chosen::Place(place.clone()))
                }
                Some(_) => None,
                None => Some(self.miss(typed)),
            },
            _ => None,
        }
    }

    fn miss(&self, typed: &str) -> Chosen {
        Chosen::Miss(Miss {
            code: "access.not_found",
            typed: typed.to_owned(),
            closest: self.closest(typed),
        })
    }
}

/// The edit distance of `a` and `b`, by characters.
fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// Whether `value` reads as an id: RFC 9562 hex and dashes, 8-4-4-4-12.
fn is_id(value: &str) -> bool {
    let groups: Vec<&str> = value.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Whether `label` is a name under the reference grammar (ADR-0026 d2): a
/// lower-case RFC 1035 label, a letter first, letters, digits and hyphens
/// inside, no hyphen last, at most 63 characters.
fn is_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1] != b'-'
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// The field's value and what it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chosen {
    /// Empty: every workarea the viewer reads.
    Every,
    /// A workarea the tree read holds.
    Place(Place),
    /// A well-formed value the tree read does not settle: the API decides.
    Ask(String),
    /// Refused: the field keeps what was typed and says why.
    Miss(Miss),
}

/// A refused value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Miss {
    pub code: &'static str,
    pub typed: String,
    pub closest: Vec<String>,
}

impl Chosen {
    /// The value a page's query sent, trimmed and lower-cased (names are
    /// lower-case by grammar), checked as the API would check it before
    /// any call: empty is every workarea; a value of three labels or more,
    /// or one under `universe.`, is a path and `request.validation`, as is
    /// a value that is neither an id nor a name (ADR-0026 d2, d7; ADR-0044
    /// d5); anything else the tree read or the API settles.
    #[must_use]
    pub fn of(sent: Option<&str>) -> Self {
        let typed = sent.unwrap_or_default().trim().to_lowercase();
        if typed.is_empty() {
            return Self::Every;
        }
        if is_id(&typed) {
            return Self::Ask(typed);
        }
        let labels: Vec<&str> = typed.split('.').collect();
        let refused = labels.len() > 2
            || (labels.len() == 2 && labels[0] == "universe")
            || !labels.iter().all(|l| is_label(l));
        if refused {
            return Self::Miss(Miss {
                code: "request.validation",
                typed,
                closest: Vec::new(),
            });
        }
        Self::Ask(typed)
    }

    /// The value to send the API as `workarea`: the place's id, or the
    /// value as typed; `None` for every workarea and for a refusal.
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Place(place) => Some(&place.id),
            Self::Ask(typed) => Some(typed),
            Self::Every | Self::Miss(_) => None,
        }
    }

    /// What the field shows and every link carries: the reference, or the
    /// value as typed.
    #[must_use]
    pub fn shown(&self) -> Option<&str> {
        match self {
            Self::Place(place) => Some(&place.reference),
            Self::Ask(typed) => Some(typed),
            Self::Miss(miss) => Some(&miss.typed),
            Self::Every => None,
        }
    }

    /// The status a page answers with while the field is refused.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Miss(miss) if miss.code == "request.validation" => StatusCode::BAD_REQUEST,
            Self::Miss(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::OK,
        }
    }

    /// Whether the field is refused, and the page lists nothing.
    #[must_use]
    pub fn refused(&self) -> bool {
        matches!(self, Self::Miss(_))
    }
}

/// Whether the API's answer is `access.not_found`, which a page reads as
/// the field's miss when it asked by the field's value.
#[must_use]
pub fn is_not_found(e: &oml_sdk::Error) -> bool {
    matches!(e, oml_sdk::Error::Api(api) if api.code == "access.not_found")
}

/// Whether the page's query asks for Browse open.
#[must_use]
pub fn browse_asked(sent: Option<&str>) -> bool {
    sent == Some("1")
}

/// Browse's counted read: each workarea with its own counts and marks, or
/// the request id of the read that failed.
pub struct Browse {
    nodes: Result<(Vec<CountedNode>, bool), String>,
}

impl Browse {
    /// The counted tree under `root` as `credential` reads it: made only
    /// when the page asked for Browse open.
    pub async fn read(web: &Web, credential: &str, calls: &PageCalls, root: &str) -> Self {
        let ids = calls.next_ids();
        let request_id = ids.request_id.clone().unwrap_or_default();
        let nodes = web
            .api
            .workarea_counts(&call(credential, &ids), root, false)
            .await
            .map(|c| (c.items, c.truncated))
            .map_err(|_| request_id);
        Self { nodes }
    }

    /// Browse from a counted read the page already made.
    #[must_use]
    pub fn of(nodes: Vec<CountedNode>, truncated: bool) -> Self {
        Self {
            nodes: Ok((nodes, truncated)),
        }
    }
}

/// One field as a page draws it.
pub struct Field<'a> {
    /// The input's `id`; the datalist, hint and refusal derive theirs.
    pub id: &'a str,
    /// The page's path: the form's action and every link's.
    pub action: &'a str,
    /// The page's other filters in force, which Browse, Clear and the
    /// closest names keep.
    pub keep: &'a [(&'a str, String)],
    pub chosen: &'a Chosen,
    pub names: &'a Names,
    /// Browse's read, when the page asked for it open.
    pub browse: Option<&'a Browse>,
    /// The admin page: every workarea, counted as memories.
    pub admin: bool,
    /// The field stands alone in its own form, with Show.
    pub standalone: bool,
    /// What empty means here.
    pub empty: &'a str,
}

impl Field<'_> {
    /// The page's link with the other filters kept, `workarea` set to
    /// `reference` where given and `browse=1` where asked.
    fn link(&self, reference: Option<&str>, browse: bool) -> String {
        let mut pairs: Vec<String> = Vec::new();
        if let Some(reference) = reference {
            pairs.push(format!("workarea={}", encode(reference)));
        }
        for (name, value) in self.keep {
            pairs.push(format!("{name}={}", encode(value)));
        }
        if browse {
            pairs.push(format!("{BROWSE}=1"));
        }
        if pairs.is_empty() {
            self.action.to_owned()
        } else {
            format!("{}?{}", self.action, pairs.join("&"))
        }
    }

    /// The unit: label, input with its datalist, hint or refusal, the
    /// chosen line, and Browse last.
    #[must_use]
    pub fn html(&self) -> String {
        let id = self.id;
        let miss = match self.chosen {
            Chosen::Miss(miss) => Some(miss),
            _ => None,
        };
        let described = if miss.is_some() {
            format!("{id}-miss")
        } else {
            format!("{id}-hint")
        };
        let (open, close) = if self.standalone {
            (
                format!(
                    r#"<form class="oml-wa" method="get" action="{}">"#,
                    attr(self.action)
                ),
                "</form>",
            )
        } else {
            (
                r#"<div class="oml-wa" role="group" aria-label="Workarea">"#.to_owned(),
                "</div>",
            )
        };
        let mut out = open;
        let _ = write!(
            out,
            r#"<label for="{id}">Workarea</label><div class="ctl"><input class="oml-input" id="{id}" name="workarea" type="text" value="{}" list="{id}-list" autocomplete="off" autocapitalize="none" spellcheck="false" aria-describedby="{described}"{}>{}</div>"#,
            attr(self.chosen.shown().unwrap_or_default()),
            if miss.is_some() {
                r#" aria-invalid="true""#
            } else {
                ""
            },
            if self.standalone {
                r#"<button type="submit">Show</button>"#
            } else {
                ""
            },
        );
        let _ = write!(out, r#"<datalist id="{id}-list">"#);
        for place in self.names.places() {
            let _ = write!(
                out,
                r#"<option value="{}"></option>"#,
                attr(&place.reference)
            );
        }
        out.push_str("</datalist>");
        match miss {
            Some(miss) => out.push_str(&self.miss(miss)),
            None => {
                let _ = write!(
                    out,
                    r#"<p class="oml-hint" id="{id}-hint">{}</p>"#,
                    self.hint()
                );
            }
        }
        if let Chosen::Place(place) = self.chosen {
            let _ = write!(
                out,
                r#"<p class="oml-wa-on">Showing <span class="ref">{}</span><a href="{}">Clear</a></p>"#,
                text(&place.reference),
                attr(&self.link(None, false))
            );
        } else if let Chosen::Ask(typed) = self.chosen {
            let _ = write!(
                out,
                r#"<p class="oml-wa-on">Showing <span class="ref">{}</span><a href="{}">Clear</a></p>"#,
                text(typed),
                attr(&self.link(None, false))
            );
        }
        out.push_str(&self.browse());
        out.push_str(close);
        out
    }

    fn hint(&self) -> String {
        if let Some(request_id) = &self.names.unavailable {
            return format!(
                r#"The workarea list is unavailable (request <code>{}</code>). {}"#,
                text(request_id),
                text(self.empty)
            );
        }
        let mut hint = text(self.empty);
        if self.admin {
            hint.push_str(
                " Type for hints; a workarea in a person's root is <code>user.name</code>.",
            );
        }
        if self.names.more {
            let _ = write!(
                hint,
                " The hints hold the first {} workareas: the list is partial. Browse and a typed name still work.",
                self.names.places.len()
            );
        }
        hint
    }

    fn miss(&self, miss: &Miss) -> String {
        let typed = text(&miss.typed);
        let sentence = if miss.code == "request.validation" {
            if miss.typed.split('.').count() > 2 || miss.typed.starts_with("universe.") {
                format!(
                    r#"<code>{typed}</code> is a path, not a name. A field takes the reference."#
                )
            } else {
                format!(r#"<code>{typed}</code> is neither an id nor a name."#)
            }
        } else if self.admin {
            format!(r#"No workarea is named <code>{typed}</code>."#)
        } else {
            format!(r#"No readable workarea is named <code>{typed}</code>."#)
        };
        let mut out = format!(
            r#"<div class="oml-wa-miss" id="{}-miss" role="alert"><p class="code">{}<code>{}</code></p><p>{sentence}</p>"#,
            self.id,
            glyph_svg("rejected"),
            text(miss.code),
        );
        if miss.closest.is_empty() {
            out.push_str(r#"<p class="oml-caption">No close name; Browse lists every one.</p>"#);
        } else {
            out.push_str(r#"<p class="oml-caption">Closest names:</p><ul class="oml-wa-close">"#);
            for name in &miss.closest {
                let _ = write!(
                    out,
                    r#"<li><a href="{}">{}</a></li>"#,
                    attr(&self.link(Some(name), false)),
                    text(name)
                );
            }
            out.push_str("</ul>");
        }
        out.push_str("</div>");
        out
    }

    /// Browse: closed, a link that asks for it open; open, the tree as
    /// links with each workarea's count and marks.
    fn browse(&self) -> String {
        let shown = self.chosen.shown();
        let Some(browse) = self.browse else {
            return format!(
                r#"<a class="oml-wa-browse oml-wa-ask" href="{}">Browse</a>"#,
                attr(&self.link(shown, true))
            );
        };
        let (nodes, truncated) = match &browse.nodes {
            Ok((nodes, truncated)) => (nodes, *truncated),
            Err(request_id) => {
                return format!(
                    r#"<details class="oml-wa-browse" open><summary>Browse</summary><div class="panel"><p class="error" role="status">The counts are unavailable (request <code>{}</code>).</p></div></details>"#,
                    text(request_id)
                );
            }
        };
        let unit = if self.admin { "memories" } else { "readable" };
        let mut out = format!(
            r#"<details class="oml-wa-browse" open><summary>Browse <span class="n">{} workareas{}</span></summary><div class="panel"><ul class="oml-wa-tree">"#,
            nodes.len(),
            if truncated {
                " · counted from the first 2,000"
            } else {
                ""
            }
        );
        let roots: BTreeSet<&str> = nodes
            .iter()
            .filter(|n| n.user_root)
            .map(|n| n.name.as_str())
            .collect();
        let refer = |n: &CountedNode| {
            reference(&n.path, |l| roots.contains(l) || self.names.is_user_root(l))
        };
        let current = match self.chosen {
            Chosen::Place(place) => Some(place.id.as_str()),
            _ => None,
        };
        // Depth-first from each node whose parent is not listed.
        let listed: BTreeSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
        let tops: Vec<&CountedNode> = nodes
            .iter()
            .filter(|n| n.parent_id.as_deref().is_none_or(|p| !listed.contains(p)))
            .collect();
        for top in tops {
            self.node(&mut out, nodes, top, &refer, current, unit);
        }
        out.push_str("</ul></div></details>");
        out
    }

    fn node(
        &self,
        out: &mut String,
        nodes: &[CountedNode],
        n: &CountedNode,
        refer: &dyn Fn(&CountedNode) -> String,
        current: Option<&str>,
        unit: &str,
    ) {
        let r = refer(n);
        let in_progress = n.mode == "in_progress";
        let _ = write!(
            out,
            r#"<li><span class="n"><a class="ref" href="{}"{}>{}</a><span class="ct">{} {unit}</span>{}{}</span>"#,
            attr(&self.link(Some(&r), true)),
            if current == Some(n.id.as_str()) {
                r#" aria-current="true""#
            } else {
                ""
            },
            text(&r),
            Counts::of(&n.counts).total(),
            if n.user_root {
                r#"<span class="oml-tag">user root</span>"#
            } else if n.parent_id.is_none() {
                r#"<span class="oml-tag">shared root</span>"#
            } else {
                ""
            },
            if in_progress {
                state_tag("in progress")
            } else {
                String::new()
            },
        );
        let children: Vec<&CountedNode> = nodes
            .iter()
            .filter(|c| c.parent_id.as_deref() == Some(n.id.as_str()))
            .collect();
        if !children.is_empty() {
            let _ = write!(
                out,
                r#"<ul{}>"#,
                if in_progress { r#" class="ip""# } else { "" }
            );
            for child in children {
                self.node(out, nodes, child, refer, current, unit);
            }
            out.push_str("</ul>");
        }
        out.push_str("</li>");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "00000000-0000-4000-8000-000000000001";

    fn alice() -> Names {
        Names::of(&[
            ("u", "universe", false),
            (ROOT, "universe.alice", true),
            ("p", "universe.alice.proj", false),
            ("d", "universe.alice.proj.docs", false),
            ("n", "universe.alice.notes", false),
            ("b", "universe.bob", true),
            ("bd", "universe.bob.docs", false),
            ("pl", "universe.platform", false),
            ("oc", "universe.platform.oncall", false),
        ])
    }

    #[test]
    fn a_reference_is_the_bare_name_or_user_dot_name_and_never_a_path() {
        let roots = |l: &str| l == "alice";
        assert_eq!(reference("universe", roots), "universe");
        assert_eq!(reference("universe.alice", roots), "alice");
        assert_eq!(reference("universe.alice.proj", roots), "alice.proj");
        assert_eq!(reference("universe.alice.proj.docs", roots), "alice.docs");
        assert_eq!(reference("universe.platform", roots), "platform");
        assert_eq!(reference("universe.platform.oncall", roots), "oncall");
        assert!(
            alice()
                .places()
                .iter()
                .all(|p| p.reference.split('.').count() <= 2)
        );
    }

    #[test]
    fn a_typed_name_is_trimmed_and_lower_cased_and_a_path_is_refused_without_a_call() {
        assert_eq!(Chosen::of(Some("  ")), Chosen::Every);
        assert_eq!(Chosen::of(Some(" PROJ ")), Chosen::Ask("proj".to_owned()));
        for path in [
            "alice.proj.docs",
            "universe.alice",
            "Bad Name",
            "a..b",
            "-x",
        ] {
            match Chosen::of(Some(path)) {
                Chosen::Miss(miss) => assert_eq!(miss.code, "request.validation", "{path}"),
                other => panic!("{path}: {other:?}"),
            }
        }
        assert_eq!(Chosen::of(Some(ROOT)), Chosen::Ask(ROOT.to_owned()));
    }

    #[test]
    fn a_bare_name_held_by_one_person_root_resolves_on_both_sides() {
        let names = alice();
        let chosen = names.resolve(Chosen::of(Some("notes")));
        assert!(
            matches!(&chosen, Chosen::Place(p) if p.reference == "alice.notes"),
            "{chosen:?}"
        );
        // `docs` is held by alice and by bob: not one workarea.
        assert!(matches!(
            names.resolve(Chosen::of(Some("docs"))),
            Chosen::Miss(_)
        ));
        assert!(matches!(
            names.resolve(Chosen::of(Some("alice.docs"))),
            Chosen::Place(p) if p.id == "d"
        ));
    }

    #[test]
    fn the_closest_names_come_from_the_read_alone_and_a_path_offers_its_last_label_first() {
        let names = alice();
        let Chosen::Miss(miss) = names.resolve(Chosen::of(Some("alice.proj.docs"))) else {
            panic!("a path is refused");
        };
        assert_eq!(&miss.closest[..2], ["alice.docs", "bob.docs"]);
        assert!(miss.closest.contains(&"alice.proj".to_owned()));
        let Chosen::Miss(miss) = names.resolve(Chosen::of(Some("pro"))) else {
            panic!("unknown");
        };
        assert_eq!(miss.code, "access.not_found");
        assert_eq!(miss.closest[0], "alice.proj");
        let Chosen::Miss(miss) = names.resolve(Chosen::of(Some("secret"))) else {
            panic!("unknown");
        };
        assert!(miss.closest.len() <= CLOSEST_MAX);
        assert!(
            miss.closest
                .iter()
                .all(|c| names.places().iter().any(|p| &p.reference == c))
        );
    }

    #[test]
    fn a_name_an_incomplete_read_lacks_is_left_to_the_api() {
        let mut names = alice();
        names.more = true;
        assert_eq!(
            names.resolve(Chosen::of(Some("elsewhere"))),
            Chosen::Ask("elsewhere".to_owned())
        );
        let unavailable = Names::unavailable("req".to_owned());
        assert_eq!(
            unavailable.resolve(Chosen::of(Some("proj"))),
            Chosen::Ask("proj".to_owned())
        );
    }

    #[test]
    fn edits_counts_insertions_deletions_and_substitutions() {
        assert_eq!(edits("docs", "docs"), 0);
        assert_eq!(edits("docs", "dogs"), 1);
        assert_eq!(edits("doc", "docs"), 1);
        assert_eq!(edits("notes", "nots"), 1);
        assert_eq!(edits("proj", "docs"), 4);
    }

    #[test]
    fn a_closed_browse_is_a_link_that_asks_for_it_and_the_hints_are_names_only() {
        let names = alice();
        let chosen = names.resolve(Chosen::of(Some("alice.proj")));
        let keep = [("state", "held".to_owned())];
        let field = Field {
            id: "wa",
            action: "/web/memories",
            keep: &keep,
            chosen: &chosen,
            names: &names,
            browse: None,
            admin: false,
            standalone: false,
            empty: "Empty: every readable workarea.",
        };
        let html = field.html();
        assert!(
            html.contains(r#"<option value="alice.docs"></option>"#),
            "{html}"
        );
        assert!(
            !html.contains("readable</span>"),
            "no count when closed: {html}"
        );
        assert!(
            html.contains(
                r#"href="/web/memories?workarea=alice.proj&amp;state=held&amp;browse=1">Browse</a>"#
            ),
            "{html}"
        );
        assert!(
            html.contains(r#"Showing <span class="ref">alice.proj</span><a href="/web/memories?state=held">Clear</a>"#),
            "{html}"
        );
        assert!(html.contains(r#"aria-describedby="wa-hint""#), "{html}");
    }

    #[test]
    fn a_miss_is_aria_invalid_and_described_by_its_refusal() {
        let names = alice();
        let chosen = names.resolve(Chosen::of(Some("pro")));
        let field = Field {
            id: "wa",
            action: "/web/admin/memories",
            keep: &[],
            chosen: &chosen,
            names: &names,
            browse: None,
            admin: true,
            standalone: false,
            empty: "Empty: every workarea.",
        };
        let html = field.html();
        assert!(
            html.contains(r#"aria-describedby="wa-miss" aria-invalid="true""#),
            "{html}"
        );
        assert!(html.contains("<code>access.not_found</code>"), "{html}");
        assert!(
            html.contains(r#"<a href="/web/admin/memories?workarea=alice.proj">alice.proj</a>"#),
            "{html}"
        );
        assert!(!html.contains("wa-hint"), "{html}");
    }
}
