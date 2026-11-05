//! The `adr` adapter (ADR-0057 d7): one item per decision bullet under
//! `## Decision`, one summary and one rejected-alternatives item per record,
//! whole and never cut, and the `amends` and `supersedes` edges its header
//! states (d10).

use super::adapter::{Adapter, EdgeStatement, Extraction, item, utf8};
use super::markdown::{
    Line, cells, collapse, content_mask, fence_marker, heading, is_delimiter_row, is_table_row,
    lines, list_item, plain, plain_block,
};

pub struct AdrAdapter;

/// The record number of an `ADR-NNNN-*.md` path's file name.
#[must_use]
pub fn adr_number(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    let rest = name.strip_prefix("ADR-")?;
    let digits = rest.get(..4)?;
    (digits.chars().all(|c| c.is_ascii_digit())
        && rest[4..].starts_with('-')
        && name.ends_with(".md"))
    .then_some(digits)
}

/// The header lines a summary's span carries (ADR-0057 d7).
const HEADER_LABELS: [&str; 5] = ["Status", "Release", "Amends", "Supersedes", "Depends on"];

impl Adapter for AdrAdapter {
    fn name(&self) -> &'static str {
        "adr"
    }

    fn accepts(&self, path: &str) -> bool {
        adr_number(path).is_some()
    }

    fn extract(&self, path: &str, bytes: &[u8]) -> Result<Extraction, String> {
        let num = adr_number(path).ok_or_else(|| format!("{path} is not an ADR-NNNN-*.md"))?;
        let doc = utf8(path, bytes)?;
        Ok(Record::read(&doc).extraction(num))
    }
}

/// One `## ` section: its heading's line index and its lines' range.
struct Section {
    name: String,
    heading: usize,
    end: usize,
}

struct Record<'a> {
    lines: Vec<Line<'a>>,
    title: Option<usize>,
    /// The header paragraphs by label, each its line range.
    header: Vec<(&'static str, usize, usize)>,
    sections: Vec<Section>,
}

impl<'a> Record<'a> {
    fn read(doc: &'a str) -> Self {
        let lines = lines(doc);
        let mask = content_mask(&lines);
        let mut title = None;
        let mut sections: Vec<Section> = Vec::new();
        let mut fence: Option<String> = None;
        for (i, line) in lines.iter().enumerate() {
            if !mask[i] {
                continue;
            }
            let t = line.text.trim_start();
            if let Some(f) = &fence {
                if t.starts_with(f.as_str()) {
                    fence = None;
                }
                continue;
            }
            if let Some(f) = fence_marker(t) {
                fence = Some(f);
                continue;
            }
            match heading(line.text) {
                Some((1, _)) if title.is_none() && sections.is_empty() => title = Some(i),
                Some((2, text)) => {
                    if let Some(last) = sections.last_mut() {
                        last.end = i;
                    }
                    sections.push(Section {
                        name: collapse(&plain(text)).to_lowercase(),
                        heading: i,
                        end: lines.len(),
                    });
                }
                _ => {}
            }
        }
        let header_end = sections.first().map_or(lines.len(), |s| s.heading);
        let mut header = Vec::new();
        let mut i = title.map_or(0, |t| t + 1);
        while i < header_end {
            let t = lines[i].text;
            if mask[i]
                && let Some(label) = HEADER_LABELS.iter().find(|l| {
                    t.starts_with(&format!("**{l}:**")) || t.starts_with(&format!("**{l}**:"))
                })
            {
                let mut end = i + 1;
                while end < header_end
                    && !lines[end].text.trim().is_empty()
                    && !lines[end].text.starts_with("**")
                {
                    end += 1;
                }
                header.push((*label, i, end));
                i = end;
                continue;
            }
            i += 1;
        }
        Self {
            lines,
            title,
            header,
            sections,
        }
    }

    fn section(&self, prefix: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name.starts_with(prefix))
    }

    fn raw(&self, from: usize, to: usize) -> Vec<&'a str> {
        self.lines[from..to].iter().map(|l| l.text).collect()
    }

    /// `[from, to)` without blank lines at either end.
    fn trimmed(&self, mut from: usize, mut to: usize) -> (usize, usize) {
        while from < to && self.lines[from].text.trim().is_empty() {
            from += 1;
        }
        while to > from && self.lines[to - 1].text.trim().is_empty() {
            to -= 1;
        }
        (from, to)
    }

    /// The non-blank lines under `## Decision` that no decision bullet
    /// covers.
    fn uncovered(&self, decisions: &[(String, usize, usize)]) -> usize {
        let Some(section) = self.section("decision") else {
            return 0;
        };
        (section.heading + 1..section.end)
            .filter(|&i| !self.lines[i].text.trim().is_empty())
            .filter(|&i| !decisions.iter().any(|(_, s, e)| (*s..*e).contains(&i)))
            .count()
    }

    /// The decision bullets: `(handle, start, end)`, each a line range.
    fn decisions(&self) -> Vec<(String, usize, usize)> {
        let Some(section) = self.section("decision") else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut i = section.heading + 1;
        while i < section.end {
            if let Some(handle) = decision_handle(self.lines[i].text) {
                let end = self.bullet_end(i, section.end);
                out.push((handle, i, end));
                i = end;
            } else {
                i += 1;
            }
        }
        out
    }

    /// Where the list item opened at `start` ends: before the next line at
    /// column 0 that is not a lazy continuation, a table row following a
    /// row, or inside a fence the item opened.
    fn bullet_end(&self, start: usize, limit: usize) -> usize {
        let mut fence: Option<String> = None;
        let mut prev_blank = false;
        let mut prev_row = false;
        let mut j = start + 1;
        while j < limit {
            let text = self.lines[j].text;
            let t = text.trim();
            if let Some(f) = &fence {
                if t.starts_with(f.as_str()) {
                    fence = None;
                }
                j += 1;
                continue;
            }
            if t.is_empty() {
                prev_blank = true;
                j += 1;
                continue;
            }
            let indented = text.starts_with(' ') || text.starts_with('\t');
            let column0_continues = !indented
                && ((is_table_row(t) && prev_row)
                    || (!prev_blank && list_item(t).is_none() && heading(text).is_none()));
            if !indented && !column0_continues {
                break;
            }
            if let Some(f) = fence_marker(t) {
                fence = Some(f);
            }
            prev_row = is_table_row(t);
            prev_blank = false;
            j += 1;
        }
        self.trimmed(start, j).1
    }

    fn extraction(&self, num: &str) -> Extraction {
        let mut out = Extraction::default();
        let decisions = self.decisions();
        let uncovered = self.uncovered(&decisions);
        if uncovered > 0 {
            out.warnings.push(format!(
                "ADR-{num}: {uncovered} line(s) under ## Decision outside a dN bullet are not \
                 loaded (ADR-0057 d7)"
            ));
        }
        let mut headlines = Vec::new();
        let mut headline_raw = Vec::new();
        for (handle, start, end) in &decisions {
            let raw = self.raw(*start, *end);
            let mut body = raw.clone();
            let first = body[0].trim_start();
            body[0] = first.strip_prefix("- ").unwrap_or(first);
            let plain_text = plain_block(&body);
            let rest = strip_handle(&plain_text, handle);
            let text = format!("ADR-{num} {handle}: {rest}");
            let (headline, raw_headline) = bold_headline(&raw);
            headlines.push(headline);
            headline_raw.push(raw_headline);
            out.items.push(item(
                format!("adr-{num}-{handle}"),
                Some(handle.clone()),
                &text,
                &raw.join("\n"),
                (self.lines[*start].number, self.lines[*end - 1].number),
            ));
        }
        // The summary, before the decisions in document order.
        let mut span = Vec::new();
        let mut text = String::new();
        let mut first_line = 0;
        let mut last_line = 0;
        if let Some(t) = self.title {
            span.push(self.lines[t].text.to_owned());
            text.push_str(&collapse(&plain(
                heading(self.lines[t].text).map_or("", |h| h.1),
            )));
            first_line = self.lines[t].number;
        }
        for (label, from, to) in &self.header {
            let raw = self.raw(*from, *to);
            span.push(raw.join("\n"));
            if *label == "Status" {
                text.push('\n');
                text.push_str(&plain_block(&raw));
            }
            last_line = last_line.max(self.lines[*to - 1].number);
        }
        if let Some(context) = self.section("context") {
            let (from, _) = self.trimmed(context.heading + 1, context.end);
            let mut to = from;
            while to < context.end && !self.lines[to].text.trim().is_empty() {
                to += 1;
            }
            if to > from {
                let raw = self.raw(from, to);
                span.push(raw.join("\n"));
                text.push_str("\n\n");
                text.push_str(&plain_block(&raw));
            }
        }
        if !headlines.is_empty() {
            text.push_str("\n\nDecisions:");
            for (h, raw) in headlines.iter().zip(&headline_raw) {
                text.push('\n');
                text.push_str(h);
                span.push(raw.clone());
            }
            last_line = last_line.max(decisions.last().map_or(0, |d| self.lines[d.2 - 1].number));
        }
        out.items.insert(
            0,
            item(
                format!("adr-{num}-summary"),
                None,
                &text,
                &span.join("\n"),
                (first_line.max(1), last_line.max(first_line).max(1)),
            ),
        );
        if let Some(rejected) = self.section("rejected") {
            let (from, to) = self.trimmed(rejected.heading + 1, rejected.end);
            if to > from {
                let raw = self.raw(from, to);
                let rows = rejected_rows(&raw);
                let mut text = format!("ADR-{num} rejected alternatives:");
                for row in rows {
                    text.push_str("\n- ");
                    text.push_str(&row);
                }
                let span = self.raw(rejected.heading, to).join("\n");
                out.items.push(item(
                    format!("adr-{num}-rejected"),
                    Some("rejected-alternatives".to_owned()),
                    &text,
                    &span,
                    (
                        self.lines[rejected.heading].number,
                        self.lines[to - 1].number,
                    ),
                ));
            }
        }
        for (label, from, to) in &self.header {
            let edge_label = match *label {
                "Amends" => "amends",
                "Supersedes" => "supersedes",
                _ => continue,
            };
            let raw = self.raw(*from, *to).join(" ");
            let body = raw.split_once(":**").map_or(raw.as_str(), |(_, b)| b);
            for (target, handles) in targets(&plain(body), num) {
                let from_key = format!("adr-{num}-summary");
                if handles.is_empty() {
                    push_edge(
                        &mut out.edges,
                        &from_key,
                        edge_label,
                        format!("adr-{target}-summary"),
                    );
                }
                for h in handles {
                    push_edge(
                        &mut out.edges,
                        &from_key,
                        edge_label,
                        format!("adr-{target}-{h}"),
                    );
                }
            }
        }
        out
    }
}

fn push_edge(edges: &mut Vec<EdgeStatement>, from: &str, label: &str, to: String) {
    let edge = EdgeStatement {
        from: from.to_owned(),
        label: label.to_owned(),
        to,
    };
    if !edges.contains(&edge) {
        edges.push(edge);
    }
}

/// The handle of a decision bullet `- **dN …` at column 0, in lower case.
fn decision_handle(text: &str) -> Option<String> {
    let rest = text.strip_prefix("- **d")?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let mut handle = format!("d{}", &rest[..digits]);
    let mut after = rest[digits..].chars();
    let mut next = after.next();
    if let Some(c) = next
        && c.is_ascii_lowercase()
    {
        let following = after.next();
        if following.is_none_or(|f| !f.is_alphanumeric()) {
            handle.push(c);
            next = following;
        } else {
            return None;
        }
    }
    match next {
        Some(c) if c.is_alphanumeric() => None,
        _ => Some(handle),
    }
}

/// A plain bullet without its leading handle and the separator after it.
fn strip_handle<'t>(text: &'t str, handle: &str) -> &'t str {
    let Some(rest) = text.strip_prefix(handle) else {
        return text;
    };
    for sep in [" — ", " – ", " - ", ". ", ": ", "."] {
        if let Some(r) = rest.strip_prefix(sep) {
            return r.trim_start();
        }
    }
    text
}

/// The bold run that opens a bullet, plain and raw.
fn bold_headline(raw: &[&str]) -> (String, String) {
    let joined = raw.join("\n");
    let body = joined.trim_start().strip_prefix("- ").unwrap_or(&joined);
    if let Some(inner) = body.strip_prefix("**")
        && let Some(end) = inner.find("**")
    {
        let raw_bold = &inner[..end];
        return (collapse(&plain(raw_bold)), format!("**{raw_bold}**"));
    }
    let first = raw.first().copied().unwrap_or_default();
    (collapse(&plain(first)), first.to_owned())
}

/// A rejected-alternatives section as rows: a table's rows as
/// `alternative: why not`, else its top-level bullets, else its paragraphs.
fn rejected_rows(raw: &[&str]) -> Vec<String> {
    let rows: Vec<&str> = raw
        .iter()
        .map(|l| l.trim())
        .filter(|t| is_table_row(t))
        .collect();
    if let Some(delim) = rows.iter().position(|t| is_delimiter_row(t)) {
        return rows[delim + 1..]
            .iter()
            .filter(|t| !is_delimiter_row(t))
            .map(|t| {
                let cells: Vec<String> = cells(t).iter().map(|c| collapse(&plain(c))).collect();
                match cells.split_first() {
                    Some((alt, why)) if !why.is_empty() => format!("{alt}: {}", why.join(" | ")),
                    Some((alt, _)) => alt.clone(),
                    None => String::new(),
                }
            })
            .collect();
    }
    let mut items: Vec<Vec<&str>> = Vec::new();
    for line in raw {
        if list_item(line).is_some() && !line.starts_with(' ') {
            items.push(vec![
                line.strip_prefix("- ")
                    .or_else(|| line.strip_prefix("* "))
                    .unwrap_or(line),
            ]);
        } else if let Some(cur) = items.last_mut()
            && (line.starts_with(' ') || line.trim().is_empty() || !cur.is_empty())
        {
            cur.push(line);
        }
    }
    if items.is_empty() {
        return plain_block(raw)
            .split("\n\n")
            .map(collapse)
            .filter(|p| !p.is_empty())
            .collect();
    }
    items
        .iter()
        .map(|lines| collapse(&plain_block(lines)))
        .collect()
}

/// The records an `Amends:` or `Supersedes:` line names, each with the
/// decision handles it names: in the first parenthesis after the record,
/// those of each `;` part before its `:`, else the handles outside every
/// parenthesis before the next record (ADR-0057 d10). The record's own
/// number is skipped.
fn targets(text: &str, own: &str) -> Vec<(String, Vec<String>)> {
    let mut starts = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(off) = text[i..].find("ADR-") {
        let at = i + off;
        let digits = &text[at + 4..];
        if digits.len() >= 4 && digits.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
            starts.push(at);
        }
        i = at + 4;
    }
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for (n, &at) in starts.iter().enumerate() {
        let num = &text[at + 4..at + 8];
        let end = starts.get(n + 1).copied().unwrap_or(bytes.len());
        let segment = text[at + 8..end].trim_start();
        let mut handles = Vec::new();
        if let Some(group) = segment.strip_prefix('(') {
            let close = matching_paren(group).unwrap_or(group.len());
            let inside = &group[..close];
            // Each `;` part names the handles before its `:`, if it has
            // one; what follows a colon is commentary, which may cite the
            // amending record's own decisions.
            for part in inside.split(';') {
                let named = part.split(':').next().unwrap_or("");
                for h in handle_tokens(named) {
                    if !handles.contains(&h) {
                        handles.push(h);
                    }
                }
            }
        }
        if handles.is_empty() {
            handles = handle_tokens(&outside_parens(segment));
        }
        if num == own {
            continue;
        }
        match out.iter_mut().find(|(t, _)| t == num) {
            Some((_, hs)) => {
                for h in handles {
                    if !hs.contains(&h) {
                        hs.push(h);
                    }
                }
            }
            None => out.push((num.to_owned(), handles)),
        }
    }
    out
}

fn matching_paren(after_open: &str) -> Option<usize> {
    let mut depth = 1;
    for (i, c) in after_open.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn outside_parens(text: &str) -> String {
    let mut depth = 0_i32;
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn handle_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        let Some(rest) = word.strip_prefix('d') else {
            continue;
        };
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let tail = &rest[digits..];
        let ok = digits > 0
            && (tail.is_empty()
                || (tail.len() == 1 && tail.chars().all(|c| c.is_ascii_lowercase())));
        if ok && !out.contains(&word.to_owned()) {
            out.push(word.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_is_read_from_each_bullet_form() {
        assert_eq!(decision_handle("- **d1 — R1.**"), Some("d1".into()));
        assert_eq!(decision_handle("- **d13.** The laws"), Some("d13".into()));
        assert_eq!(
            decision_handle("- **d11 is a critical-path edit**"),
            Some("d11".into())
        );
        assert_eq!(decision_handle("- **d5a — split**"), Some("d5a".into()));
        assert_eq!(decision_handle("- **docker-compose**"), None);
        assert_eq!(decision_handle("  - **d1 — nested**"), None);
    }

    #[test]
    fn amends_targets_take_the_first_parenthesis_else_the_bare_handles() {
        let t = targets(
            "ADR-0015 (the decline of a web console; d2 gains), ADR-0010 (d1, d7), \
             ADR-0026 (d15: the admin key, d21), ADR-0049 (proposed): d3 (fusion), d4 (table). \
             ADR-0025 (d8: the score; d9: the read digest; d13), ADR-0013 (d4, at S7 only) \
             ADR-0034 d2, d3 and d11 (new tables)",
            "0045",
        );
        assert_eq!(
            t,
            vec![
                ("0015".into(), vec!["d2".to_owned()]),
                ("0010".into(), vec!["d1".into(), "d7".into()]),
                ("0026".into(), vec!["d15".into()]),
                ("0049".into(), vec!["d3".into(), "d4".into()]),
                ("0025".into(), vec!["d8".into(), "d9".into(), "d13".into()]),
                ("0013".into(), vec!["d4".into()]),
                ("0034".into(), vec!["d2".into(), "d3".into(), "d11".into()]),
            ]
        );
    }
}
