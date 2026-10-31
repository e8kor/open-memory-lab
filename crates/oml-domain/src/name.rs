//! Names — the unique, immutable key a workarea, crew, key or tenant carries
//! beside its id: the grammar a name may spell, the parse-time
//! classification of a reference as an id or a name, the derived name, and
//! the naming of rows that predate names (ADR-0026 d1, d2, d3, d6, d11).
//!
//! The rules are declared on `docs/api/http-api.md`, "Names and ids" and
//! "Rows created before names"; this module mirrors them and adds nothing.
//! Resolving a name to its row needs the store and is not here
//! (ADR-0026 d4).

use std::collections::{BTreeMap, BTreeSet};

use uuid::Uuid;

/// The name of every tenant's root workarea, the one workarea name the
/// design fixes (ADR-0026 d6).
pub const UNIVERSE: &str = "universe";

/// The four kinds that carry a name, each in a flat namespace of its own
/// (ADR-0026 d1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NamedKind {
    Tenant,
    Workarea,
    Crew,
    Key,
}

impl NamedKind {
    const ALL: [NamedKind; 4] = [
        NamedKind::Tenant,
        NamedKind::Workarea,
        NamedKind::Crew,
        NamedKind::Key,
    ];

    /// The kind's word: `tenant`, `workarea`, `crew` or `key`, as
    /// `name.taken`'s `details.kind` and `system.names_assigned`'s `kind`
    /// spell it (errors.md; event-catalog.md).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            NamedKind::Tenant => "tenant",
            NamedKind::Workarea => "workarea",
            NamedKind::Crew => "crew",
            NamedKind::Key => "key",
        }
    }

    /// The prefix of the kind's derived name: `tenant`, `wa`, `crew` or
    /// `key` (ADR-0026 d11, rule 3).
    fn derived_prefix(self) -> &'static str {
        match self {
            NamedKind::Tenant => "tenant",
            NamedKind::Workarea => "wa",
            NamedKind::Crew => "crew",
            NamedKind::Key => "key",
        }
    }
}

/// Why a value is not a name. A request field carrying one, where a name
/// or an id is expected, is refused with `request.validation`, never
/// coerced into a name it resembles (http-api.md, "Names and ids";
/// errors.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameRefusal {
    /// Not a lowercase RFC 1035 label: a letter first, then letters, digits
    /// and hyphens, a letter or digit last, 1–63 ASCII characters.
    #[error(
        "not a lowercase RFC 1035 label of 1-63 characters (http-api.md names and ids; ADR-0026 d2, request.validation)"
    )]
    Grammar,
    /// A label in one of the three spellings reserved so that no name reads
    /// as an id: the hex-and-dash form, 32 hexadecimal digits, or the
    /// derived form `tenant-`, `wa-`, `crew-` or `key-` and 32 hexadecimal
    /// digits.
    #[error(
        "reads as an id: the hex-and-dash form, 32 hex digits or the derived form (http-api.md names and ids; ADR-0026 d2, request.validation)"
    )]
    ReadsAsId,
}

/// A row's name: one the grammar admits ([`Name::parse`]), or the derived
/// name a row that predates names is given ([`Name::derived`]). Names
/// compare byte for byte; there is no normalization and no case mapping
/// (ADR-0026 d2).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(String);

impl Name {
    /// The value as a name a creation may choose, under the creation grammar
    /// of ADR-0026 d2: `^[a-z]([a-z0-9-]{0,61}[a-z0-9])?$`, minus the three
    /// spellings that read as an id. Every allowed character is ASCII, so
    /// the value is taken as it is or refused.
    pub fn parse(value: &str) -> Result<Self, NameRefusal> {
        let name = Self::parse_reference(value)?;
        if is_derived_form(value) {
            return Err(NameRefusal::ReadsAsId);
        }
        Ok(name)
    }

    /// The value as a name in a reference, under the reference grammar of
    /// ADR-0026 d2: the creation grammar with the derived form admitted, so
    /// a row d11 named in that form is addressed by it like any other. It is
    /// also how a stored name reads back, since the schema's `oml_name`
    /// domain checks this grammar (postgres-schema.md names and tenants).
    pub fn parse_reference(value: &str) -> Result<Self, NameRefusal> {
        if !is_label(value) {
            return Err(NameRefusal::Grammar);
        }
        if is_id_form(value) || is_hex32(value) {
            return Err(NameRefusal::ReadsAsId);
        }
        Ok(Name(value.to_owned()))
    }

    /// The derived name of a row of `kind`: the kind's prefix, a hyphen and
    /// the 32 lowercase hexadecimal digits of its id (ADR-0026 d11, rule 3).
    /// [`Name::parse`] refuses the form, so a derived name collides with
    /// no created one.
    #[must_use]
    pub fn derived(kind: NamedKind, id: Uuid) -> Self {
        Name(format!("{}-{}", kind.derived_prefix(), id.simple()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A lowercase RFC 1035 label of at most 63 characters (RFC 1035 §2.3.1;
/// ADR-0026 d2).
fn is_label(value: &str) -> bool {
    let bytes = value.as_bytes();
    let (Some(first), Some(last)) = (bytes.first(), bytes.last()) else {
        return false;
    };
    bytes.len() <= 63
        && first.is_ascii_lowercase()
        && (last.is_ascii_lowercase() || last.is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// RFC 9562 §4 hex-and-dash text, 8-4-4-4-12 hexadecimal digits in either
/// case (ADR-0026 d3).
fn is_id_form(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

fn is_hex32(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_derived_form(value: &str) -> bool {
    NamedKind::ALL.iter().any(|kind| {
        value
            .strip_prefix(kind.derived_prefix())
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(is_hex32)
    })
}

/// The id a value spells, when it is in the one form ids of the four named
/// kinds take on input: RFC 9562 §4 hex-and-dash text, either case
/// (ADR-0026 d3). Every other UUID spelling — 32 bare digits, braced, URN —
/// is `None`.
#[must_use]
pub fn parse_id(value: &str) -> Option<Uuid> {
    if is_id_form(value) {
        Uuid::parse_str(value).ok()
    } else {
        None
    }
}

/// A value that names a workarea, crew, key or tenant, classified at parse,
/// before any check that consults state (ADR-0026 d3). The two classes never
/// overlap, so a value's class never depends on what exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    Id(Uuid),
    Name(Name),
}

impl Reference {
    /// The classification of ADR-0026 d3: in the id form, an id; otherwise
    /// a name under d2's reference grammar, the derived form included;
    /// otherwise the name's refusal, which the caller answers with
    /// `request.validation` and `details.field` naming the field
    /// (http-api.md, "Names and ids").
    pub fn parse(value: &str) -> Result<Self, NameRefusal> {
        match parse_id(value) {
            Some(id) => Ok(Reference::Id(id)),
            None => Name::parse_reference(value).map(Reference::Name),
        }
    }
}

/// A row of a named kind that predates names: its id and the name its
/// creation event carries, if any (ADR-0026 d11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnnamedRow {
    pub kind: NamedKind,
    pub id: Uuid,
    /// `prior_name` in the assignment; `None` for every key before names.
    pub prior_name: Option<String>,
}

/// A workarea's crew object as `workarea.create` and `workarea.crew_updated`
/// carried it before crews were records, every member optional
/// (event-catalog.md; ADR-0026 d8, d11 rule 4).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CrewObject {
    pub name: Option<String>,
    pub charter_uri: Option<String>,
    pub external_manager: Option<String>,
}

/// One row's name: an entry of `system.names_assigned`'s `names`
/// (event-catalog.md; ADR-0026 d11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub kind: NamedKind,
    pub id: Uuid,
    pub name: Name,
    pub prior_name: Option<String>,
}

/// A crew record made from one distinct crew object, with the workareas
/// that point at it: an entry of `system.names_assigned`'s `crews`
/// (event-catalog.md; ADR-0026 d11, rule 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrewRecord {
    pub crew_id: Uuid,
    pub name: Name,
    pub charter_uri: Option<String>,
    pub external_manager: Option<String>,
    /// In the order the workareas were given.
    pub workarea_ids: Vec<Uuid>,
}

/// What ADR-0026 d11 assigns in one chain: the content of its
/// `system.names_assigned`. Both lists empty means the chain has nothing to
/// assign and journals no such event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Naming {
    /// One per row, in the order the rows were given.
    pub names: Vec<Assignment>,
    /// In order of first appearance.
    pub crews: Vec<CrewRecord>,
}

/// The names held so far in one chain, per kind.
#[derive(Default)]
struct Held(BTreeSet<(NamedKind, Name)>);

impl Held {
    /// Rules 1–3 for one row: the root is `universe`; a prior name the
    /// grammar admits, not `universe` on a non-root workarea and not yet
    /// held in its kind, is kept; anything else is derived from the id.
    fn name(&mut self, kind: NamedKind, id: Uuid, prior: Option<&str>, root: bool) -> Name {
        let name = if root {
            Name(UNIVERSE.to_owned())
        } else {
            prior
                .and_then(|p| Name::parse(p).ok())
                .filter(|n| !(kind == NamedKind::Workarea && n.as_str() == UNIVERSE))
                .filter(|n| !self.0.contains(&(kind, n.clone())))
                .unwrap_or_else(|| Name::derived(kind, id))
        };
        self.0.insert((kind, name.clone()));
        name
    }
}

/// Names the rows of one chain that predate names, by the deterministic
/// rule of ADR-0026 d11 (http-api.md, "Rows created before names"):
///
/// 1. the workarea `root`, the tenant's root, is named `universe`, whatever
///    its creation event says;
/// 2. per kind, in the order given — journal order of creation — a row
///    whose prior name the grammar admits, that is not `universe` for a
///    non-root workarea, and that no earlier row of its kind holds, keeps
///    it;
/// 3. every other row is given its [`Name::derived`] name;
/// 4. `crews` pairs each workarea with its current crew object (the latest
///    of its `workarea.create` and `workarea.crew_updated`), in journal
///    order of the workareas; each distinct object becomes one crew record
///    with an id from `fresh_id`, named by rules 2 and 3 in order of first
///    appearance, and every workarea with that object points at it.
///
/// A tenant's chain passes its root, its workareas and its keys; the
/// provider chain passes no root, its tenants and its operator keys. Fresh
/// ids enter as inputs, so the function is deterministic, and the event it
/// fills records every id it was given.
pub fn assign_names(
    root: Option<Uuid>,
    rows: &[UnnamedRow],
    crews: &[(Uuid, CrewObject)],
    mut fresh_id: impl FnMut() -> Uuid,
) -> Naming {
    let mut held = Held::default();
    let names = rows
        .iter()
        .map(|row| {
            let is_root = row.kind == NamedKind::Workarea && root == Some(row.id);
            Assignment {
                kind: row.kind,
                id: row.id,
                name: held.name(row.kind, row.id, row.prior_name.as_deref(), is_root),
                prior_name: row.prior_name.clone(),
            }
        })
        .collect();
    let mut records: Vec<CrewRecord> = Vec::new();
    let mut record_of: BTreeMap<&CrewObject, usize> = BTreeMap::new();
    for (workarea_id, object) in crews {
        if let Some(&index) = record_of.get(object) {
            records[index].workarea_ids.push(*workarea_id);
            continue;
        }
        let crew_id = fresh_id();
        record_of.insert(object, records.len());
        records.push(CrewRecord {
            crew_id,
            name: held.name(NamedKind::Crew, crew_id, object.name.as_deref(), false),
            charter_uri: object.charter_uri.clone(),
            external_manager: object.external_manager.clone(),
            workarea_ids: vec![*workarea_id],
        });
    }
    Naming {
        names,
        crews: records,
    }
}
