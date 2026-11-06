//! The Atlas view (ADR-0045 d25, d30, d32): the workareas the page's key
//! may read under the scope, as one squarified treemap whose areas are
//! their readable counts, drawn on the server as SVG with no script; its
//! table twin; and the scope's panel with its counts, its links and its
//! dropdown of acts. Every region is a plain link that zooms the Atlas to
//! it. Nothing the key cannot read is drawn or counted: the regions and
//! their counts are the app's, read through the same listings as the list.

use std::collections::HashMap;
use std::fmt::Write as _;

use uuid::Uuid;

use oml_app::app::{Caller, Region, Regions, WorkareaOffers};
use oml_app::domain::name::Reference;
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};

use super::page::{attr, glyph_svg, text};
use crate::ApiState;
use crate::error::{ApiError, ErrorContext, map_app_error};

/// Counts by state, in the design system's bar order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub approved: u64,
    pub held: u64,
    pub in_progress: u64,
    pub other: u64,
}

impl Counts {
    fn of(r: &Region) -> Self {
        Self {
            approved: r.approved,
            held: r.held,
            in_progress: r.in_progress,
            other: r.other,
        }
    }

    fn add(&mut self, o: Self) {
        self.approved += o.approved;
        self.held += o.held;
        self.in_progress += o.in_progress;
        self.other += o.other;
    }

    pub fn total(self) -> u64 {
        self.approved + self.held + self.in_progress + self.other
    }

    /// `(class, count, word)` of each non-zero state.
    fn parts(self) -> Vec<(&'static str, u64, &'static str)> {
        [
            ("promoted", self.approved, "promoted"),
            ("held", self.held, "held"),
            ("unreviewed", self.in_progress, "unreviewed in progress"),
            ("superseded", self.other, "other"),
        ]
        .into_iter()
        .filter(|(_, n, _)| *n > 0)
        .collect()
    }

    /// The counts in words, the bar's `aria-label`.
    pub fn words(self) -> String {
        let parts = self.parts();
        if parts.is_empty() {
            return "none readable".to_owned();
        }
        parts
            .iter()
            .map(|(_, n, w)| format!("{n} {w}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// One tile of the map: a child of the scope with its subtree's counts,
/// or the scope's own rows.
#[derive(Clone)]
struct Tile {
    name: String,
    href: String,
    counts: Counts,
    status: String,
    in_progress: bool,
}

/// The squarified layout (Bruls, Huizing and van Wijk) of `sizes`, largest
/// first, in a `w` by `h` box: one rectangle `(x, y, w, h)` per size, in
/// order, each with an area proportional to its size.
#[must_use]
pub fn squarify(sizes: &[f64], w: f64, h: f64) -> Vec<(f64, f64, f64, f64)> {
    let total: f64 = sizes.iter().sum();
    if total <= 0.0 || sizes.is_empty() {
        return Vec::new();
    }
    let scale = w * h / total;
    let areas: Vec<f64> = sizes.iter().map(|s| s * scale).collect();
    let (mut x, mut y, mut w, mut h) = (0.0, 0.0, w, h);
    let mut out = Vec::with_capacity(areas.len());
    let worst = |row: &[f64], side: f64| -> f64 {
        let sum: f64 = row.iter().sum();
        let max = row.iter().copied().fold(0.0, f64::max);
        let min = row.iter().copied().fold(f64::INFINITY, f64::min);
        (side * side * max / (sum * sum)).max(sum * sum / (side * side * min))
    };
    let mut lay = |row: &[f64], x: &mut f64, y: &mut f64, w: &mut f64, h: &mut f64| {
        let sum: f64 = row.iter().sum();
        if *w >= *h {
            let cw = sum / *h;
            let mut cy = *y;
            for a in row {
                out.push((*x, cy, cw, a / cw));
                cy += a / cw;
            }
            *x += cw;
            *w -= cw;
        } else {
            let rh = sum / *w;
            let mut cx = *x;
            for a in row {
                out.push((cx, *y, a / rh, rh));
                cx += a / rh;
            }
            *y += rh;
            *h -= rh;
        }
    };
    let mut row: Vec<f64> = Vec::new();
    let mut i = 0;
    while i < areas.len() {
        let side = w.min(h);
        let mut with = row.clone();
        with.push(areas[i]);
        if row.is_empty() || worst(&with, side) <= worst(&row, side) {
            row = with;
            i += 1;
        } else {
            lay(&row, &mut x, &mut y, &mut w, &mut h);
            row.clear();
        }
    }
    if !row.is_empty() {
        lay(&row, &mut x, &mut y, &mut w, &mut h);
    }
    out
}

/// `name` cut to `chars` characters, an ellipsis marking the cut.
fn cut(name: &str, chars: usize) -> String {
    if name.chars().count() <= chars {
        name.to_owned()
    } else if chars <= 1 {
        "…".to_owned()
    } else {
        name.chars().take(chars - 1).collect::<String>() + "…"
    }
}

/// The map in a `w` by `h` viewBox, labels dropping by priority as a tile
/// shrinks: name and count, then the state bar, then the in-progress mark;
/// a tile too small for its name keeps a cut name or its count, and every
/// tile keeps its full name and counts in its `title` and `aria-label`.
fn svg(tiles: &[Tile], w: f64, h: f64, font: f64, class: &str) -> String {
    let sizes: Vec<f64> = tiles.iter().map(|t| t.counts.total() as f64).collect();
    let rects = squarify(&sizes, w, h);
    let mut out = format!(
        r#"<svg class="tm {class}" viewBox="0 0 {w} {h}" role="group" aria-label="Workareas sized by readable memory count; the table below has the same rows">"#
    );
    let gap = 2.0;
    for (t, (x, y, rw, rh)) in tiles.iter().zip(rects) {
        let (x, y, rw, rh) = (
            x + gap / 2.0,
            y + gap / 2.0,
            (rw - gap).max(1.0),
            (rh - gap).max(1.0),
        );
        let n = t.counts.total();
        let label = format!(
            "{}, {n} readable: {}{}",
            t.name,
            t.counts.words(),
            if t.in_progress { "; in progress" } else { "" }
        );
        let _ = write!(
            out,
            r#"<a href="{}" aria-label="{}"><title>{}</title><rect class="tm-r{}" x="{x:.1}" y="{y:.1}" width="{rw:.1}" height="{rh:.1}" rx="4"/>"#,
            attr(&t.href),
            attr(&label),
            text(&label),
            if t.in_progress { " ip" } else { "" },
        );
        let pad = font * 0.6;
        let fits = |chars: usize| ((rw - 2.0 * pad) / (font * 0.62)).floor() as usize >= chars;
        let room = ((rw - 2.0 * pad) / (font * 0.62)).floor().max(0.0) as usize;
        if rh >= font * 2.6 && room >= 3 {
            // Name and count.
            let _ = write!(
                out,
                r#"<text class="tm-nm" x="{:.1}" y="{:.1}" font-size="{font}">{}</text><text class="tm-n" x="{:.1}" y="{:.1}" font-size="{:.1}">{n}</text>"#,
                x + pad,
                y + pad + font,
                text(&cut(&t.name, room)),
                x + pad,
                y + pad + font * 2.3,
                font * 0.92
            );
            if rh >= font * 4.6 {
                // The state bar.
                let bw = rw - 2.0 * pad;
                let by = y + rh - pad - font * 0.55;
                let mut bx = x + pad;
                let total = n.max(1) as f64;
                for (class, count, _) in t.counts.parts() {
                    let sw = bw * count as f64 / total;
                    let _ = write!(
                        out,
                        r#"<rect class="bar {class}" x="{bx:.1}" y="{by:.1}" width="{:.1}" height="{:.1}"/>"#,
                        (sw - 1.0).max(0.5),
                        font * 0.55
                    );
                    bx += sw;
                }
                if t.in_progress && rh >= font * 6.2 && fits(11) {
                    let _ = write!(
                        out,
                        r#"<text class="tm-ip" x="{:.1}" y="{:.1}" font-size="{:.1}">in progress</text>"#,
                        x + pad,
                        y + pad + font * 3.6,
                        font * 0.85
                    );
                }
            }
        } else if rh >= font * 1.4 && room >= 2 {
            // A cut name keeps the tile named.
            let _ = write!(
                out,
                r#"<text class="tm-nm" x="{:.1}" y="{:.1}" font-size="{:.1}">{}</text>"#,
                x + pad * 0.6,
                y + rh / 2.0 + font * 0.32,
                font * 0.85,
                text(&cut(&t.name, room))
            );
        } else if rh >= font && rw >= font * 1.2 {
            let _ = write!(
                out,
                r#"<text class="tm-n" x="{:.1}" y="{:.1}" font-size="{:.1}">{n}</text>"#,
                x + 2.0,
                y + rh / 2.0 + font * 0.3,
                font * 0.8
            );
        }
        out.push_str("</a>");
    }
    out.push_str("</svg>");
    out
}

/// The design system's `LifecycleBar`: one `i` a non-zero state, its
/// share a `f-<n>` class of twenty steps (the policy allows no inline
/// style), the counts as its label.
pub(crate) fn lifebar(c: Counts) -> String {
    let total = c.total().max(1) as f64;
    let mut out = format!(
        r#"<div class="oml-lifebar" role="img" aria-label="{}">"#,
        attr(&c.words())
    );
    for (class, n, _) in c.parts() {
        let share = ((20.0 * n as f64 / total).round() as u64).clamp(1, 20);
        let _ = write!(out, r#"<i class="{class} f-{share}"></i>"#);
    }
    out.push_str("</div>");
    out
}

fn legend(c: Counts) -> String {
    let mut out = String::from(r#"<ul class="oml-legend" aria-hidden="true">"#);
    for (class, n, word) in c.parts() {
        let state = class_state(class);
        let _ = write!(
            out,
            r#"<li><span class="c-{state}">{}</span><b>{n}</b> {word}</li>"#,
            glyph_svg(state)
        );
    }
    out.push_str("</ul>");
    out
}

fn class_state(class: &str) -> &str {
    match class {
        "unreviewed" => "in-progress",
        other => other,
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
    /// The Atlas of `scope` (the person's root, or `universe` on the admin
    /// page, when none): the regions as the page's key may read them, the
    /// map, the table twin and the scope's panel. `transitions` and `mode`
    /// act for the panel's dropdown as on the Workareas view (d28).
    pub(crate) async fn atlas_view(
        &self,
        base: &str,
        reader: &Caller,
        transitions: &Caller,
        mode: &Caller,
        scope: Option<Reference>,
        admin: bool,
    ) -> Result<String, ApiError> {
        let mapped = |e| map_app_error(e, ErrorContext::Write);
        let found: Regions = if admin {
            self.app.admin_regions(reader, scope).await
        } else {
            self.app.web_regions(reader, scope).await
        }
        .map_err(mapped)?;
        let Some(top) = found.regions.first() else {
            return Err(ApiError::not_found());
        };
        let top_id = top.workarea.workarea_id;
        // Each node's subtree counts, children folded into parents; the
        // tree read lists a parent before its children.
        let mut subtree: HashMap<Uuid, Counts> = found
            .regions
            .iter()
            .map(|r| (r.workarea.workarea_id, Counts::of(r)))
            .collect();
        for r in found.regions.iter().rev() {
            if let Some(parent) = r.workarea.parent_id
                && parent != r.workarea.workarea_id
                && subtree.contains_key(&parent)
                && r.workarea.workarea_id != top_id
            {
                let own = subtree[&r.workarea.workarea_id];
                subtree.get_mut(&parent).expect("present").add(own);
            }
        }
        let zoom = |id: Uuid| format!("{base}?view=memories&display=atlas&workarea={id}");
        let mut tiles: Vec<Tile> = found
            .regions
            .iter()
            .filter(|r| r.workarea.parent_id == Some(top_id))
            .map(|r| Tile {
                name: r.workarea.name.as_str().to_owned(),
                href: zoom(r.workarea.workarea_id),
                counts: subtree[&r.workarea.workarea_id],
                status: r.workarea.status.as_str().to_owned(),
                in_progress: r.workarea.in_progress,
            })
            .collect();
        let own = Counts::of(top);
        if own.total() > 0 || tiles.is_empty() {
            tiles.push(Tile {
                name: format!("{} · own rows", top.workarea.name.as_str()),
                href: format!("{base}?view=memories&workarea={top_id}"),
                counts: own,
                status: top.workarea.status.as_str().to_owned(),
                in_progress: top.workarea.in_progress,
            });
        }
        tiles.sort_by(|a, b| {
            b.counts
                .total()
                .cmp(&a.counts.total())
                .then(a.name.cmp(&b.name))
        });
        let drawn_owned: Vec<Tile> = tiles
            .iter()
            .filter(|t| t.counts.total() > 0)
            .cloned()
            .collect();
        let total = subtree[&top_id];

        let mut out = String::from(r#"<nav class="crumbs" aria-label="Breadcrumb">"#);
        for (id, name) in &found.ancestors {
            let _ = write!(
                out,
                r#"<a href="{}">{}</a> / "#,
                attr(&zoom(*id)),
                text(name.as_str())
            );
        }
        let _ = write!(
            out,
            r#"<span aria-current="page">{}</span></nav>"#,
            text(top.workarea.name.as_str())
        );
        let _ = write!(
            out,
            r#"<div class="oml-main"><div class="col"><p class="desc">{} readable · area = count{}</p>"#,
            total.total(),
            if found.partial {
                " · counted from the first 2,000 rows"
            } else {
                ""
            }
        );
        if drawn_owned.is_empty() {
            out.push_str(r#"<p class="desc">Nothing readable here yet.</p>"#);
        } else {
            out.push_str(r#"<figure class="tm-fig">"#);
            out.push_str(&svg(&drawn_owned, 960.0, 540.0, 20.0, "wide"));
            out.push_str(&svg(&drawn_owned, 360.0, 270.0, 14.0, "narrow"));
            out.push_str("</figure>");
        }
        // The table twin: the same rows, the numbers right-aligned.
        out.push_str(r#"<table class="twin" id="atlas-table"><thead><tr><th scope="col">Workarea</th><th scope="col">Status</th><th scope="col" class="num">Readable</th><th scope="col">By state</th></tr></thead><tbody>"#);
        for t in &tiles {
            let _ = write!(
                out,
                r#"<tr><td class="mono"><a href="{}">{}</a></td><td>{}{}</td><td class="num">{}</td><td>{}<span class="cap">{}</span></td></tr>"#,
                attr(&t.href),
                text(&t.name),
                text(&t.status),
                if t.in_progress { " · in progress" } else { "" },
                t.counts.total(),
                lifebar(t.counts),
                text(&t.counts.words())
            );
        }
        out.push_str("</tbody></table></div>");

        // The scope's panel: what it holds, where to read it, what to do.
        let offers: WorkareaOffers = self
            .app
            .workarea_offers(transitions, mode, top_id)
            .await
            .map_err(mapped)?;
        let _ = write!(
            out,
            r#"<aside class="oml-rail-side" aria-labelledby="sel-h"><section class="oml-panel"><h2 id="sel-h" class="mono">{}</h2><p class="desc">{}{} · <b>{}</b> readable</p>{}{}<p><a href="{base}?view=memories&amp;workarea={top_id}">Open memories</a></p>"#,
            text(&top.path),
            text(top.workarea.status.as_str()),
            if top.workarea.in_progress {
                " · in progress"
            } else {
                ""
            },
            total.total(),
            lifebar(total),
            legend(total)
        );
        out.push_str(&super::workareas::dropdown(base, top_id, &offers));
        out.push_str("</section></aside></div>");
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_area_is_proportional_to_its_size_and_the_tiles_fill_the_box() {
        let sizes = [27.0, 16.0, 9.0, 5.0, 4.0, 2.0, 1.0];
        let rects = squarify(&sizes, 960.0, 540.0);
        let total: f64 = sizes.iter().sum();
        let mut covered = 0.0;
        for (size, (x, y, w, h)) in sizes.iter().zip(&rects) {
            let area = w * h;
            covered += area;
            assert!((area - 960.0 * 540.0 * size / total).abs() < 1e-6);
            assert!(*x >= -1e-9 && *y >= -1e-9 && x + w <= 960.0 + 1e-6 && y + h <= 540.0 + 1e-6);
        }
        assert!((covered - 960.0 * 540.0).abs() < 1e-3);
    }

    #[test]
    fn a_tile_too_small_for_its_name_keeps_a_cut_name_or_its_title() {
        let tiles = [
            Tile {
                name: "a-workarea-with-a-very-long-name".to_owned(),
                href: "/x".to_owned(),
                counts: Counts {
                    approved: 1000,
                    ..Counts::default()
                },
                status: "development".to_owned(),
                in_progress: false,
            },
            Tile {
                name: "tiny-but-named".to_owned(),
                href: "/y".to_owned(),
                counts: Counts {
                    held: 200,
                    ..Counts::default()
                },
                status: "development".to_owned(),
                in_progress: false,
            },
        ];
        let drawn = svg(&tiles, 360.0, 270.0, 13.0, "narrow");
        assert!(drawn.contains("<title>tiny-but-named, 200 readable: 200 held</title>"));
        assert!(drawn.contains("aria-label=\"tiny-but-named, 200 readable: 200 held\""));
        assert!(
            drawn.contains(">tiny…</text>"),
            "the narrow tile keeps a cut name: {drawn}"
        );
    }
}
