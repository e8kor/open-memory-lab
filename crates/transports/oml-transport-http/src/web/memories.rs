//! The Memories list of both pages and its Atlas view (ADR-0045 d22–d27).
//!
//! The list is server-rendered cards, one page at a time, resumed by the
//! listing's keyset cursor and narrowed by five filters. The user page
//! lists what `<user>-web` may read and the held rows of the person's own
//! keys (d23); the admin page lists through the admin read, one
//! `memory.admin_read` per page (d24). The Atlas view of the same page is
//! runs no script (d26): the server draws it as SVG with a table twin. The
//! JSON routes under `/web` and `/web/admin` answer the list's page and the
//! Atlas's workareas as data (d25, d32).

use std::fmt::Write as _;

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_app::app::{AdminMemory, AdminSearchRequest, Caller, WebListRequest};
use oml_app::domain::anchor::{Anchor, Origin};
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::origin::OriginKind;
use oml_app::domain::valid_time::ValidTime;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};
use oml_app::types::{AdminState, Bearer, HIT_KINDS, RecallRequest, ServedMemory};

use super::page::{attr, glyph_svg, input, select, state_tag, text};
use super::views::Fields;
use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::oauth::percent_encode;
use crate::{ApiState, retry_chain};

/// A page's rows when the query names no size (ADR-0045 d22).
pub const PAGE_DEFAULT: usize = 50;
/// The largest page, `api.page_limit_max`'s shipped value.
pub const PAGE_MAX: usize = 200;
/// Recall's own bound on one answer, the user page's text search.
pub const RECALL_MAX: usize = 100;

/// The Memories view's query, each field validated (ADR-0045 d22).
pub(crate) struct ListQuery {
    q: Option<String>,
    state: Option<AdminState>,
    workarea: Option<(String, Reference)>,
    origin: Option<OriginKind>,
    author: Option<String>,
    cursor: Option<String>,
    limit: usize,
    limit_sent: Option<String>,
    /// `display=atlas`.
    pub atlas: bool,
}

fn refused(field: &'static str, message: &str) -> ApiError {
    ApiError::validation(message.to_owned()).with_details(json!({ "field": field }))
}

impl ListQuery {
    pub(crate) fn parse(fields: &Fields) -> Result<Self, ApiError> {
        let limit_sent = fields.get("limit").map(ToOwned::to_owned);
        let limit = match &limit_sent {
            None => PAGE_DEFAULT,
            Some(v) => v
                .parse::<usize>()
                .ok()
                .filter(|l| (1..=PAGE_MAX).contains(l))
                .ok_or_else(|| refused("limit", &format!("limit is between 1 and {PAGE_MAX}")))?,
        };
        let state = fields
            .get("state")
            .map(|s| {
                AdminState::parse(s).ok_or_else(|| {
                    refused(
                        "state",
                        "state is held, promoted, rejected, superseded or demoted",
                    )
                })
            })
            .transpose()?;
        let origin = fields
            .get("origin")
            .map(|s| {
                OriginKind::parse(s).ok_or_else(|| {
                    refused(
                        "origin",
                        "origin is repo, doc, code, lake, chat, web or manual",
                    )
                })
            })
            .transpose()?;
        let workarea = fields
            .get("workarea")
            .map(|w| {
                Reference::parse(w)
                    .map(|r| (w.to_owned(), r))
                    .map_err(|e| refused("workarea", &e.to_string()))
            })
            .transpose()?;
        let author = fields.get("author").map(ToOwned::to_owned);
        if let Some(a) = &author {
            Reference::parse(a).map_err(|e| refused("author", &e.to_string()))?;
        }
        Ok(Self {
            q: fields.get("q").map(ToOwned::to_owned),
            state,
            workarea,
            origin,
            author,
            cursor: fields.get("cursor").map(ToOwned::to_owned),
            limit,
            limit_sent,
            atlas: fields.get("display") == Some("atlas"),
        })
    }

    /// The `workarea` filter as a reference, if one is set.
    pub(crate) fn workarea_reference(&self) -> Option<Reference> {
        self.workarea.as_ref().map(|(_, r)| r.clone())
    }

    /// The view's link on `base` with every filter kept, the cursor when
    /// given, and the Atlas when asked; escaped for an attribute.
    fn link(&self, base: &str, cursor: Option<&str>, atlas: bool) -> String {
        let mut out = format!("{base}?view=memories");
        let state = self.state.map(AdminState::as_str);
        let origin = self.origin.map(OriginKind::as_str);
        for (name, value) in [
            ("q", self.q.as_deref()),
            ("state", state),
            ("workarea", self.workarea.as_ref().map(|(w, _)| w.as_str())),
            ("origin", origin),
            ("author", self.author.as_deref()),
            ("limit", self.limit_sent.as_deref()),
            ("cursor", cursor),
        ] {
            if let Some(value) = value {
                let _ = write!(out, "&amp;{name}={}", attr(&percent_encode(value)));
            }
        }
        if atlas {
            out.push_str("&amp;display=atlas");
        }
        out
    }
}

/// One memory as the card shows it and the JSON route serves it: nothing
/// the card does not show (ADR-0045 d22, d25).
pub(crate) struct Card {
    id: Uuid,
    state: &'static str,
    workarea: String,
    version: u32,
    title: Option<String>,
    body: String,
    prose: bool,
    claim_key: String,
    /// `None` where the card shows the shared area (ADR-0044 d9).
    author: Option<String>,
    /// `agent`, `person` or `service`, the author key's `held_by`, where
    /// the page's key may know it; `None` otherwise.
    author_kind: Option<&'static str>,
    written: String,
    served: Option<String>,
    scan: Option<String>,
    anchor: Option<String>,
    format: Option<String>,
    valid: Option<String>,
}

/// An anchor as one line (ADR-0057 d2), unescaped.
pub(crate) fn anchor_text(anchor: Option<&Anchor>) -> Option<String> {
    let a = anchor?;
    let fragment = a
        .fragment
        .as_deref()
        .map_or_else(String::new, |f| format!("#{f}"));
    Some(match &a.origin {
        Origin::Repo { repo, rev, path } => {
            format!("{repo} @ {} {path}{fragment}", &rev[..rev.len().min(12)])
        }
        Origin::Uri { uri, .. } => format!("{uri}{fragment}"),
    })
}

fn valid_text(valid: &ValidTime) -> Option<String> {
    (!valid.is_open()).then(|| {
        format!(
            "from {}, {}",
            valid.from.as_ref().map_or("any time", |f| f.as_str()),
            valid
                .until
                .as_ref()
                .map_or_else(|| "open".to_owned(), |u| format!("until {}", u.as_str()))
        )
    })
}

fn is_prose(media_type: &str) -> bool {
    matches!(media_type, "text/plain" | "text/markdown")
}

fn kind_word(held_by: oml_app::domain::HeldBy) -> &'static str {
    match held_by {
        oml_app::domain::HeldBy::Human => "person",
        oml_app::domain::HeldBy::Agent => "agent",
        _ => "service",
    }
}

fn day(t: &oml_app::domain::EventTime) -> String {
    t.as_str().chars().take(10).collect()
}

impl Card {
    fn of_served(m: &ServedMemory, kinds: &[(Uuid, &'static str)]) -> Self {
        let r = &m.memory;
        Self {
            author_kind: (!m.anonymous)
                .then(|| {
                    kinds
                        .iter()
                        .find(|(k, _)| *k == r.author_key_id)
                        .map(|(_, w)| *w)
                })
                .flatten(),
            written: day(&r.created_at),
            served: r.promoted_at.as_ref().map(day),
            scan: r.scan_finding.clone(),
            id: r.memory_id,
            state: AdminState::of(r).as_str(),
            workarea: m.workarea_path.clone().unwrap_or_default(),
            version: r.version,
            title: r.format.title.clone(),
            body: r.body.clone(),
            prose: is_prose(&r.format.media_type),
            claim_key: r.claim_key.clone(),
            author: (!m.anonymous).then(|| r.author_principal.clone()),
            anchor: anchor_text(r.anchor.as_deref()),
            format: (!is_prose(&r.format.media_type)).then(|| r.format.media_type.clone()),
            valid: valid_text(&r.valid),
        }
    }

    fn of_admin(m: &AdminMemory) -> Self {
        let r = &m.memory;
        let key = m
            .author
            .as_ref()
            .and_then(|k| k.name.as_ref())
            .map_or_else(|| r.author_key_id.to_string(), |n| n.as_str().to_owned());
        Self {
            author_kind: m.author.as_ref().map(|k| kind_word(k.held_by)),
            written: day(&r.created_at),
            served: r.promoted_at.as_ref().map(day),
            scan: r.scan_finding.clone(),
            id: r.memory_id,
            state: m.state.as_str(),
            workarea: m.workarea_path.clone(),
            version: r.version,
            title: r.format.title.clone(),
            body: r.body.clone(),
            prose: is_prose(&r.format.media_type),
            claim_key: r.claim_key.clone(),
            author: Some(match &m.author_user {
                Some(user) => format!("{key} ({})", user.as_str()),
                None => key,
            }),
            anchor: anchor_text(r.anchor.as_deref()),
            format: (!is_prose(&r.format.media_type)).then(|| r.format.media_type.clone()),
            valid: valid_text(&r.valid),
        }
    }

    fn json(&self) -> Value {
        json!({
            "artifact_id": self.id,
            "state": self.state,
            "workarea": self.workarea,
            "version": self.version,
            "title": self.title,
            "text": self.body,
            "claim_key": self.claim_key,
            "author": self.author,
            "author_kind": self.author_kind,
            "anchor": self.anchor,
            "format": self.format,
            "valid": self.valid,
        })
    }

    /// The memory as the design system's Round 3 compact row (ADR-0045
    /// d32; ADR-0069 d3): the summary holds the state and the text,
    /// clamped, then the author's kind, the version and the way in; the
    /// body, closed by default, holds the provenance timeline and the facts.
    fn html(&self, open: &str) -> String {
        let reviewed = self.state != "held";
        let served = matches!(self.state, "promoted" | "superseded" | "demoted");
        let rejected = self.state == "rejected";
        let quiet = matches!(self.state, "superseded" | "rejected" | "demoted");
        let who = self
            .author
            .as_deref()
            .map_or_else(|| "the shared area".to_owned(), text);
        let kind = self.author_kind.unwrap_or("shared");
        let actor_glyph = match self.author_kind {
            Some("person") => glyph_svg("person"),
            Some(_) => glyph_svg("agent"),
            None => glyph_svg("shared"),
        };
        let segment = match self.state {
            "demoted" => "retired",
            other => other,
        };
        let mut out = format!(
            r#"<li><details class="oml-mrow{}" id="m-{}" data-author-kind="{kind}" data-reviewed="{reviewed}"><summary><div class="top">{}<span class="tx">{}</span></div><div class="sub"><span class="oml-actor">{actor_glyph}<span class="who">{who}</span><span class="kind">{kind}</span></span><span class="vb"><ol aria-label="Versions"><li aria-current="true"><i class="{segment}"></i></li></ol>v{}{}</span><span class="more"><span class="w">Provenance</span></span></div></summary><div class="body">"#,
            if quiet { " quiet" } else { "" },
            self.id,
            state_tag(self.state),
            text(self.title.as_deref().unwrap_or(&self.body)),
            self.version,
            if self.state == "promoted" {
                " · tip"
            } else {
                ""
            },
        );
        if self.title.is_some() {
            let _ = write!(
                out,
                r#"<p class="text{}">{}</p>"#,
                if self.prose { "" } else { " mono" },
                text(&self.body)
            );
        }
        let step = |done: bool| if done { "" } else { " class=\"wait\"" };
        let review_line = match self.state {
            "held" => "held until someone else reviews it".to_owned(),
            "rejected" => "rejected by a key outside its lineage".to_owned(),
            _ => "approved by a key outside its lineage".to_owned(),
        };
        let served_line = match (self.state, &self.served) {
            ("rejected", _) => "never: the text is kept".to_owned(),
            (_, Some(d)) => format!("to readers of {} · {d}", text(&self.workarea)),
            _ => "not yet".to_owned(),
        };
        let _ = write!(
            out,
            r#"<div class="oml-timeline-wrap"><ol class="oml-timeline" aria-label="Provenance"><li><span class="c-dim">{actor_glyph}</span><span class="step">Written</span><span><span class="who">{who}</span> · {kind}</span><span>{}</span></li><li><span class="c-held">{}</span><span class="step">Held</span><span>scan: {}</span></li><li{}><span class="{}">{}</span><span class="step">{}</span><span>{review_line}</span></li><li{}><span class="{}">{}</span><span class="step">Served</span><span>{served_line}</span></li></ol></div><details class="oml-more"><summary>Details</summary><dl class="oml-facts"><dt>Id</dt><dd><span class="id">{}</span></dd><dt>Workarea</dt><dd><span class="id">{}</span></dd><dt>Claim key</dt><dd><span class="id">{}</span></dd>"#,
            self.written,
            glyph_svg("held"),
            self.scan
                .as_deref()
                .map_or_else(|| "no findings".to_owned(), text),
            step(reviewed),
            if rejected {
                "c-rejected"
            } else if reviewed {
                "c-dim"
            } else {
                "c-faint"
            },
            glyph_svg("person"),
            if rejected { "Rejected" } else { "Reviewed" },
            step(served),
            if served { "c-promoted" } else { "c-faint" },
            glyph_svg("promoted"),
            self.id,
            text(&self.workarea),
            text(&self.claim_key),
        );
        for (label, value) in [
            ("Source", &self.anchor),
            ("Format", &self.format),
            ("Valid", &self.valid),
        ] {
            if let Some(value) = value {
                let _ = write!(out, "<dt>{label}</dt><dd>{}</dd>", text(value));
            }
        }
        let _ = write!(
            out,
            r#"<dt>Open</dt><dd><a href="{open}&amp;memory={}">this memory</a></dd></dl></details></div></details></li>"#,
            self.id
        );
        out
    }
}

/// One page of cards and the cursor of the next.
pub(crate) struct CardPage {
    cards: Vec<Card>,
    next: Option<String>,
}

impl CardPage {
    /// The JSON route's answer: `{items, next_cursor}`.
    pub(crate) fn json(&self) -> Value {
        json!({
            "items": self.cards.iter().map(Card::json).collect::<Vec<_>>(),
            "next_cursor": self.next,
        })
    }
}

/// A JSON answer of a web route: never cached, never sniffed, no CORS.
pub(crate) fn json_answer(status: StatusCode, value: &Value) -> Response {
    let mut response = Response::new(Body::from(value.to_string()));
    *response.status_mut() = status;
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    response
}

fn app_error(e: oml_app::types::AppError) -> ApiError {
    map_app_error(e, ErrorContext::Write)
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// The user page's page of the list (ADR-0045 d22, d23), as
    /// `<user>-web`: the merged listing, or with `q` recall's one ranked
    /// page narrowed by the other filters.
    pub(crate) async fn user_cards(
        &self,
        web: &Caller,
        q: &ListQuery,
    ) -> Result<CardPage, ApiError> {
        let Some(text_query) = &q.q else {
            let page = self
                .app
                .web_memories(
                    web,
                    &WebListRequest {
                        workarea: q.workarea.as_ref().map(|(_, r)| r.clone()),
                        state: q.state,
                        origin: q.origin,
                        author: q
                            .author
                            .as_deref()
                            .map(Name::parse)
                            .transpose()
                            .map_err(|_| ApiError::not_found())?,
                        cursor: q.cursor.clone(),
                        limit: q.limit,
                    },
                )
                .await
                .map_err(app_error)?;
            let kinds = self.own_kinds(web).await?;
            return Ok(CardPage {
                cards: page
                    .items
                    .iter()
                    .map(|m| Card::of_served(m, &kinds))
                    .collect(),
                next: page.next_cursor,
            });
        };
        // The author, one of the person's own keys, read before the search.
        let author = match &q.author {
            None => None,
            Some(name) => Some(
                self.app
                    .web_own_keys(web)
                    .await
                    .map_err(app_error)?
                    .into_iter()
                    .find(|k| k.key.name.as_ref().map(Name::as_str) == Some(name.as_str()))
                    .ok_or_else(ApiError::not_found)?
                    .key
                    .key_id,
            ),
        };
        let kinds = self.own_kinds(web).await?;
        let result = self
            .app
            .recall(
                Bearer::Session(web),
                &RecallRequest {
                    sources: None,
                    timings: false,
                    query: text_query.clone(),
                    k: q.limit.min(RECALL_MAX),
                    workarea: q.workarea.as_ref().map(|(_, r)| r.clone()),
                    kinds: HIT_KINDS.to_vec(),
                    include_text: true,
                    include_unreviewed: Some(true),
                    max_tokens: None,
                    search: None,
                    match_mode: oml_app::search::MatchMode::Any,
                    include_expired: false,
                    now: None,
                    anchor: oml_app::types::AnchorFilter {
                        repo: None,
                        path_prefix: None,
                        origin_kinds: q.origin.into_iter().collect(),
                    },
                    as_of: None,
                },
            )
            .await
            .map_err(app_error)?;
        let cards = result
            .steering
            .iter()
            .chain(result.hits.iter().map(|h| &h.served))
            .filter(|m| q.state.is_none_or(|s| AdminState::of(&m.memory) == s))
            .filter(|m| author.is_none_or(|a| m.memory.author_key_id == a))
            .map(|m| Card::of_served(m, &kinds))
            .collect();
        Ok(CardPage { cards, next: None })
    }

    /// The `held_by` of each of the person's keys, the authors the user
    /// page may name a kind for (ADR-0045 d32).
    async fn own_kinds(&self, web: &Caller) -> Result<Vec<(Uuid, &'static str)>, ApiError> {
        Ok(self
            .app
            .web_own_keys(web)
            .await
            .map_err(app_error)?
            .iter()
            .map(|k| (k.key.key_id, kind_word(k.key.held_by)))
            .collect())
    }

    /// The admin page's page of the list (ADR-0045 d22, d24), as
    /// `<user>-admin`: one admin read, one event.
    pub(crate) async fn admin_cards(
        &self,
        admin: &Caller,
        q: &ListQuery,
    ) -> Result<CardPage, ApiError> {
        let after = q
            .cursor
            .as_deref()
            .map(super::admin_read::parse_cursor)
            .transpose()
            .map_err(|_| refused("cursor", "cursor is not a cursor this list served"))?;
        let author = q
            .author
            .as_deref()
            .map(Reference::parse)
            .transpose()
            .map_err(|e| refused("author", &e.to_string()))?;
        let req = AdminSearchRequest {
            query: q.q.as_deref(),
            workarea: q.workarea.as_ref().map(|(_, r)| r.clone()),
            state: q.state,
            origin: q.origin,
            author,
            after,
            limit: q.limit,
        };
        let page = retry_chain(|| self.app.admin_search_filtered(Bearer::Session(admin), &req))
            .await
            .map_err(app_error)?;
        Ok(CardPage {
            cards: page.items.iter().map(Card::of_admin).collect(),
            next: page
                .next
                .as_ref()
                .map(|(at, id)| format!("{}_{id}", at.as_str())),
        })
    }
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// The Atlas's workareas as JSON (ADR-0045 d30): each node the viewer
    /// may read under the filter, its rows by state, its badges and the
    /// acts its dropdown offers. `reader` is the page's reading key,
    /// `<user>-web` or `<user>-admin`; transitions run as `<user>-admin` on
    /// the admin page and as `review` on the user page, the mode as
    /// `review` on both (d28).
    pub(crate) async fn regions_json(
        &self,
        reader: &Caller,
        review: &Caller,
        q: &ListQuery,
        admin: bool,
    ) -> Result<Value, ApiError> {
        // The person's root unless a workarea is chosen, as the HTML Atlas
        // does (d32); the admin page's scope is `universe`.
        let workarea = q.workarea_reference().or_else(|| {
            if admin {
                None
            } else {
                reader.key().user.map(Reference::Id)
            }
        });
        let found = if admin {
            self.app.admin_regions(reader, workarea).await
        } else {
            self.app.web_regions(reader, workarea).await
        }
        .map_err(app_error)?;
        let transitions = if admin { reader } else { review };
        let mut out = Vec::with_capacity(found.regions.len());
        for (i, r) in found.regions.iter().enumerate() {
            let w = &r.workarea;
            // Each offer is several store reads, so only the page's scope
            // node, the first region, carries them, as the HTML Atlas's
            // panel does; a region is zoomed to for its own (d28, d30).
            let offers = if i == 0 {
                Some(
                    self.app
                        .workarea_offers(transitions, review, w.workarea_id)
                        .await
                        .map_err(app_error)?,
                )
            } else {
                None
            };
            let offered: Option<Vec<Value>> = offers.as_ref().map(|o| {
                o.offers
                    .iter()
                    .map(|o| {
                        json!({
                            "value": super::workareas::offer_value(o),
                            "text": super::workareas::offer_text(o),
                        })
                    })
                    .collect()
            });
            let mut region = json!({
                "id": w.workarea_id,
                "parent": w.parent_id,
                "name": w.name.as_str(),
                "path": r.path,
                "status": w.status.as_str(),
                "in_progress": w.in_progress,
                // Ontology crews are designed (ADR-0027 [proposed in #10, unmerged]) and not stored
                // yet; the badge shows when a workarea can say so.
                "ontology": false,
                "counts": {
                    "approved": r.approved,
                    "held": r.held,
                    "in_progress": r.in_progress,
                    "other": r.other,
                    "total": r.approved + r.held + r.in_progress + r.other,
                },
            });
            if let (Some(offered), Some(o)) = (offered, offers) {
                region["offers"] = json!(offered);
                region["commit_withheld"] = json!(o.commit_withheld);
            }
            out.push(region);
        }
        Ok(json!({ "items": out, "partial": found.partial }))
    }
}

/// The filter form, the List · Atlas toggle and, for the list, the cards
/// and the pager (ADR-0045 d22, d25, d27).
pub(crate) fn list_view(base: &str, q: &ListQuery, page: Option<&CardPage>, lede: &str) -> String {
    let state = q.state.map_or("", AdminState::as_str);
    let origin = q.origin.map_or("", OriginKind::as_str);
    let selected = |options: &[(&'static str, &'static str)], chosen: &str| {
        // The chosen value first, so the form shows what this page filters.
        let mut out: Vec<(&str, &str)> = options
            .iter()
            .filter(|(v, _)| *v == chosen)
            .copied()
            .collect();
        out.extend(options.iter().filter(|(v, _)| *v != chosen).copied());
        out
    };
    let states = selected(
        &[
            ("", "any"),
            ("held", "held"),
            ("promoted", "promoted"),
            ("rejected", "rejected"),
            ("superseded", "superseded"),
            ("demoted", "demoted"),
        ],
        state,
    );
    let origins = selected(
        &[
            ("", "any"),
            ("manual", "manual"),
            ("repo", "repo"),
            ("doc", "doc"),
            ("code", "code"),
            ("lake", "lake"),
            ("chat", "chat"),
            ("web", "web"),
        ],
        origin,
    );
    let mut out = format!(
        r#"<header class="oml-section-head"><div class="top"><h1>{}</h1></div><p class="desc">{lede}</p></header><form class="filters" method="get" action="{base}"><input type="hidden" name="view" value="memories">{}{}{}{}{}{}<button type="submit">Filter</button></form>"#,
        "Memories",
        input("q", "Text", q.q.as_deref().unwrap_or(""), false),
        select("state", "State", &states),
        input(
            "workarea",
            "Workarea",
            q.workarea.as_ref().map_or("", |(w, _)| w.as_str()),
            false
        ),
        select("origin", "Source", &origins),
        input("author", "Author", q.author.as_deref().unwrap_or(""), false),
        if q.atlas {
            r#"<input type="hidden" name="display" value="atlas">"#
        } else {
            ""
        },
    );
    let list_href = q.link(base, q.cursor.as_deref(), false);
    let atlas_href = q.link(base, q.cursor.as_deref(), true);
    let _ = write!(
        out,
        r#"<nav class="toggle" aria-label="display"><a href="{list_href}"{}>List</a><a href="{atlas_href}"{}>Atlas</a></nav>"#,
        if q.atlas {
            ""
        } else {
            r#" class="on" aria-current="page""#
        },
        if q.atlas {
            r#" class="on" aria-current="page""#
        } else {
            ""
        },
    );
    let Some(page) = page else {
        return out;
    };
    if q.q.is_some() && base == "/web" {
        out.push_str(r#"<p class="dim">Best text matches first; one page.</p>"#);
    }
    if page.cards.is_empty() {
        out.push_str(r#"<p class="dim">No memory matches.</p>"#);
    }
    let open = q.link(base, None, false);
    out.push_str(r#"<ul class="oml-mlist">"#);
    for card in &page.cards {
        out.push_str(&card.html(&open));
    }
    out.push_str(r#"</ul><p class="pager">"#);
    if q.cursor.is_some() {
        let _ = write!(
            out,
            r#"<a href="{}">First page</a> "#,
            q.link(base, None, false)
        );
    }
    if let Some(next) = &page.next {
        let _ = write!(
            out,
            r#"<a rel="next" href="{}">Next page</a>"#,
            q.link(base, Some(next), false)
        );
    }
    out.push_str("</p>");
    out
}
