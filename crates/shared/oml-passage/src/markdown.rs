//! The markdown an adapter reads, as plain text: links to their text,
//! emphasis and code marks dropped, a paragraph to one line, list items and
//! table rows to lines of their own (ADR-0057 d7). Hand-written and
//! deterministic: no adapter reads anything but its path and bytes.

/// One line of the source, numbered from 1.
#[derive(Clone, Copy, Debug)]
pub struct Line<'a> {
    pub number: usize,
    pub text: &'a str,
}

/// The lines of a document, CRLF read as LF.
#[must_use]
pub fn lines(doc: &str) -> Vec<Line<'_>> {
    doc.split('\n')
        .enumerate()
        .map(|(i, text)| Line {
            number: i + 1,
            text: text.strip_suffix('\r').unwrap_or(text),
        })
        .collect()
}

/// Which lines are content: false inside front matter at the top and inside
/// HTML comments outside code fences.
#[must_use]
pub fn content_mask(lines: &[Line<'_>]) -> Vec<bool> {
    let mut mask = vec![true; lines.len()];
    let mut start = 0;
    if lines.first().is_some_and(|l| l.text.trim_end() == "---")
        && let Some(end) = lines
            .iter()
            .skip(1)
            .position(|l| matches!(l.text.trim_end(), "---" | "..."))
    {
        for m in mask.iter_mut().take(end + 2) {
            *m = false;
        }
        start = end + 2;
    }
    let mut in_comment = false;
    let mut fence: Option<String> = None;
    for (i, line) in lines.iter().enumerate().skip(start) {
        let t = line.text.trim_start();
        if !in_comment {
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
            if t.starts_with("<!--") {
                in_comment = !t.contains("-->");
                mask[i] = false;
                continue;
            }
        } else {
            mask[i] = false;
            if t.contains("-->") {
                in_comment = false;
            }
        }
    }
    mask
}

/// The fence a line opens, three or more backticks or tildes.
#[must_use]
pub fn fence_marker(trimmed: &str) -> Option<String> {
    for c in ['`', '~'] {
        let n = trimmed.chars().take_while(|&x| x == c).count();
        if n >= 3 {
            return Some(std::iter::repeat_n(c, n).collect());
        }
    }
    None
}

/// The ATX heading a line is, as `(level, text)`.
#[must_use]
pub fn heading(text: &str) -> Option<(usize, &str)> {
    if text.starts_with(' ') {
        return None;
    }
    let level = text.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &text[level..];
    if !(rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')) {
        return None;
    }
    let rest = rest.trim();
    // A closing run of `#` is not part of the text.
    let rest = rest.trim_end_matches('#').trim_end();
    Some((level, rest))
}

/// Inline markdown as plain text: `[text](url)` and `![alt](url)` to their
/// text, `<url>` to the URL, code marks and `*`/`**` emphasis dropped,
/// backslash escapes resolved; code spans keep their content literally.
#[must_use]
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() && chars[i + 1].is_ascii_punctuation() => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '`' => {
                let run = chars[i..].iter().take_while(|&&x| x == '`').count();
                if let Some(end) = find_run(&chars, i + run, '`', run) {
                    let inner: String = chars[i + run..end].iter().collect();
                    out.push_str(inner.trim());
                    i = end + run;
                } else {
                    i += run;
                }
            }
            '!' if chars.get(i + 1) == Some(&'[') => {
                i += 1;
            }
            '[' => {
                if let Some((label, next)) = link(&chars, i) {
                    out.push_str(&plain(&label));
                    i = next;
                } else {
                    out.push('[');
                    i += 1;
                }
            }
            '<' => {
                if let Some(end) = chars[i + 1..].iter().position(|&x| x == '>') {
                    let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                    if inner.starts_with("http://") || inner.starts_with("https://") {
                        out.push_str(&inner);
                        i += end + 2;
                        continue;
                    }
                }
                out.push('<');
                i += 1;
            }
            '*' => {
                let before = i.checked_sub(1).map(|j| chars[j]);
                let run = chars[i..].iter().take_while(|&&x| x == '*').count();
                let after = chars.get(i + run).copied();
                // A lone `*` between spaces is a literal (a product, a glob).
                let literal = run == 1
                    && before.is_none_or(char::is_whitespace)
                    && after.is_none_or(char::is_whitespace);
                if literal {
                    out.push('*');
                }
                i += run;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// The index where a run of exactly `n` `c` starts at or after `from`.
fn find_run(chars: &[char], from: usize, c: char, n: usize) -> Option<usize> {
    let mut j = from;
    while j < chars.len() {
        if chars[j] == c {
            let run = chars[j..].iter().take_while(|&&x| x == c).count();
            if run == n {
                return Some(j);
            }
            j += run;
        } else {
            j += 1;
        }
    }
    None
}

/// A link at `[`: its label and the index after `(url)`, `[ref]` or `[]`.
fn link(chars: &[char], open: usize) -> Option<(String, usize)> {
    let mut depth = 0;
    let mut j = open;
    let close = loop {
        match chars.get(j)? {
            '\\' => j += 1,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    break j;
                }
            }
            _ => {}
        }
        j += 1;
    };
    let label: String = chars[open + 1..close].iter().collect();
    match chars.get(close + 1) {
        Some('(') => {
            let mut depth = 0;
            let mut k = close + 1;
            loop {
                match chars.get(k)? {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some((label, k + 1));
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
        }
        Some('[') => {
            let end = chars[close + 1..].iter().position(|&x| x == ']')?;
            Some((label, close + 1 + end + 1))
        }
        _ => None,
    }
}

/// Whether a trimmed line opens a list item; returns the text after the
/// marker.
#[must_use]
pub fn list_item(trimmed: &str) -> Option<&str> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            return Some(rest);
        }
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits < 10 {
        let rest = &trimmed[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some(r);
        }
    }
    None
}

/// Whether a trimmed line is a table row.
#[must_use]
pub fn is_table_row(trimmed: &str) -> bool {
    trimmed.starts_with('|')
}

/// Whether a trimmed table row is the delimiter row under the header.
#[must_use]
pub fn is_delimiter_row(trimmed: &str) -> bool {
    is_table_row(trimmed)
        && trimmed.contains('-')
        && trimmed
            .chars()
            .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

/// A table row's cells, raw, split at `|` outside code spans and escapes.
#[must_use]
pub fn cells(trimmed: &str) -> Vec<String> {
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut code = false;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '`' => {
                code = !code;
                cur.push(c);
            }
            '|' if !code => out.push(std::mem::take(&mut cur).trim().to_owned()),
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_owned());
    out
}

/// A block of markdown lines as plain text: a paragraph or a list item with
/// its lazy continuation to one line, each list item and table row a line
/// of its own (a row's cells joined by ` | `), paragraphs separated by one
/// empty line, fenced code kept verbatim, delimiter rows dropped.
#[must_use]
pub fn plain_block(raw: &[&str]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut cur: Option<String> = None;
    let mut fence: Option<String> = None;
    let mut blank = false;
    let flush = |cur: &mut Option<String>, out: &mut Vec<String>| {
        if let Some(text) = cur.take() {
            out.push(collapse(&plain(&text)));
        }
    };
    for line in raw {
        let t = line.trim();
        if let Some(f) = &fence {
            if t.starts_with(f.as_str()) {
                fence = None;
            } else {
                out.push((*line).trim_end().to_owned());
            }
            continue;
        }
        if let Some(f) = fence_marker(t) {
            flush(&mut cur, &mut out);
            fence = Some(f);
            continue;
        }
        if t.is_empty() {
            flush(&mut cur, &mut out);
            blank = true;
            continue;
        }
        if blank && !out.is_empty() && out.last().is_some_and(|l| !l.is_empty()) {
            out.push(String::new());
        }
        blank = false;
        if is_delimiter_row(t) {
            continue;
        }
        if is_table_row(t) {
            flush(&mut cur, &mut out);
            let row: Vec<String> = cells(t).iter().map(|c| collapse(&plain(c))).collect();
            out.push(row.join(" | "));
            continue;
        }
        if let Some(rest) = list_item(t) {
            flush(&mut cur, &mut out);
            let marker = &t[..t.len() - rest.len()];
            cur = Some(format!("{marker}{rest}"));
            continue;
        }
        if let Some((_, text)) = heading(t) {
            flush(&mut cur, &mut out);
            out.push(collapse(&plain(text)));
            continue;
        }
        match &mut cur {
            Some(text) => {
                text.push(' ');
                text.push_str(t);
            }
            None => cur = Some(t.to_owned()),
        }
    }
    flush(&mut cur, &mut out);
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out.join("\n")
}

/// Every run of whitespace to one space, trimmed.
#[must_use]
pub fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A heading's anchor as GitHub renders it: the plain text in lower case,
/// every character but a letter, a digit, `-`, `_` and a space dropped,
/// spaces to `-`. The repeat suffix is the caller's ([`Slugs`]).
#[must_use]
pub fn slug(heading_text: &str) -> String {
    plain(heading_text)
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c == ' ' {
                Some('-')
            } else if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c)
            } else {
                None
            }
        })
        .collect()
}

/// The slugs of one document, a repeated one suffixed `-1`, `-2` as GitHub
/// does.
#[derive(Default)]
pub struct Slugs {
    seen: std::collections::HashMap<String, usize>,
}

impl Slugs {
    /// The slug of the next heading, github-slugger's rule: a slug already
    /// taken gets the next free `-n` of its base.
    pub fn next(&mut self, heading_text: &str) -> String {
        let original = slug(heading_text);
        let mut candidate = original.clone();
        while self.seen.contains_key(&candidate) {
            let n = self.seen.entry(original.clone()).or_insert(0);
            *n += 1;
            candidate = format!("{original}-{n}");
        }
        self.seen.insert(candidate.clone(), 0);
        candidate
    }
}
