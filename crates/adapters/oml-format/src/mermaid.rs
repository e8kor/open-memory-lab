//! `text/vnd.oml.mermaid`: OML's own parser of a strict subset of Mermaid, five
//! diagram types, search text as sentences (ADR-0056 d5). Directives, click
//! handlers, styling and HTML labels are refused by name; the parser is
//! stricter than Mermaid's, so a claim can pass it and still fail to draw.

use std::collections::BTreeMap;

use oml_app::ports::{
    Format, FormatChange, FormatDiff, FormatRefusal, Parsed, RenderHint, ScanView,
};

use crate::common::check_source;

pub struct Mermaid;

const MAX_NODES: usize = 200;
const MAX_RELATIONS: usize = 400;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Flow,
    Sequence,
    Class,
    State,
    Er,
}

/// A line of the source with its 1-based number.
struct Line<'a> {
    no: u32,
    text: &'a str,
}

fn refusal(reason: &'static str, message: &str, line: u32) -> FormatRefusal {
    FormatRefusal::new(reason, message, line, 1)
}

/// What the parse collected, in source order.
#[derive(Default)]
struct Model {
    title: Option<String>,
    /// Node ids in order of first appearance, with their labels.
    nodes: Vec<(String, Option<String>)>,
    /// Ids that take part in a relation.
    related: Vec<String>,
    sentences: Vec<Vec<Part>>,
    relations: usize,
}

/// A piece of a sentence: fixed text, or a node written by its name once the
/// whole diagram is read, so a label given later is used by every sentence.
enum Part {
    T(String),
    N(String),
}

fn t(s: &str) -> Part {
    Part::T(s.to_owned())
}

fn n(s: &str) -> Part {
    Part::N(s.to_owned())
}

impl Model {
    fn node(&mut self, id: &str, label: Option<String>, line: u32) -> Result<(), FormatRefusal> {
        if let Some(slot) = self.nodes.iter_mut().find(|(i, _)| i == id) {
            if slot.1.is_none() {
                slot.1 = label;
            }
            return Ok(());
        }
        if self.nodes.len() >= MAX_NODES {
            return Err(refusal(
                "mermaid.too_many_nodes",
                &format!("a diagram holds at most {MAX_NODES} nodes"),
                line,
            ));
        }
        self.nodes.push((id.to_owned(), label));
        Ok(())
    }

    /// The name a sentence uses: the label, with the id beside it when the
    /// two differ.
    fn name(&self, id: &str) -> String {
        match self.nodes.iter().find(|(i, _)| i == id) {
            Some((_, Some(l))) if l != id => format!("{l} ({id})"),
            _ => id.to_owned(),
        }
    }

    fn relate(&mut self, from: &str, to: &str, line: u32) -> Result<(), FormatRefusal> {
        self.relations += 1;
        if self.relations > MAX_RELATIONS {
            return Err(refusal(
                "mermaid.too_many_relations",
                &format!("a diagram holds at most {MAX_RELATIONS} relations"),
                line,
            ));
        }
        for id in [from, to] {
            if !self.related.iter().any(|r| r == id) {
                self.related.push(id.to_owned());
            }
        }
        Ok(())
    }

    fn say(&mut self, parts: Vec<Part>) {
        self.sentences.push(parts);
    }

    fn finish(mut self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(title) = self.title.take() {
            out.push(title);
        }
        for parts in &self.sentences {
            out.push(
                parts
                    .iter()
                    .map(|p| match p {
                        Part::T(x) => x.clone(),
                        Part::N(id) => self.name(id),
                    })
                    .collect(),
            );
        }
        for (id, _) in &self.nodes {
            if !self.related.contains(id) {
                out.push(format!("{}.", self.name(id)));
            }
        }
        out
    }
}

/// Splits the source into numbered lines, drops `%%` comments and refuses
/// directives and the front matter's other keys.
fn lines(source: &str) -> Result<(Vec<Line<'_>>, Option<String>), FormatRefusal> {
    let all: Vec<Line<'_>> = source
        .lines()
        .enumerate()
        .map(|(i, text)| Line {
            no: u32::try_from(i + 1).unwrap_or(u32::MAX),
            text,
        })
        .collect();
    let mut title = None;
    let mut start = 0;
    let first = all.iter().position(|l| !l.text.trim().is_empty());
    if let Some(f) = first
        && all[f].text.trim() == "---"
    {
        let close = all[f + 1..]
            .iter()
            .position(|l| l.text.trim() == "---")
            .map(|c| c + f + 1)
            .ok_or_else(|| {
                refusal(
                    "mermaid.syntax_error",
                    "the front matter is never closed",
                    all[f].no,
                )
            })?;
        for l in &all[f + 1..close] {
            let t = l.text.trim();
            if t.is_empty() {
                continue;
            }
            match t.strip_prefix("title:") {
                Some(v) if title.is_none() => title = Some(unquote(v.trim()).to_owned()),
                _ => {
                    return Err(refusal(
                        "mermaid.directive_refused",
                        "front matter configures the renderer; only a title is accepted",
                        l.no,
                    ));
                }
            }
        }
        start = close + 1;
    }
    let mut out = Vec::new();
    for l in all.into_iter().skip(start) {
        let t = l.text.trim();
        if t.starts_with("%%{") || t.contains("%%{") {
            return Err(refusal(
                "mermaid.directive_refused",
                "a directive can change the security level the frame relies on",
                l.no,
            ));
        }
        if t.is_empty() || t.starts_with("%%") {
            continue;
        }
        out.push(l);
    }
    Ok((out, title))
}

fn unquote(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(s)
}

fn first_word(t: &str) -> &str {
    t.split(|c: char| c.is_whitespace() || c == ':' || c == ';')
        .next()
        .unwrap_or("")
}

/// The construct refusals that hold in every diagram type.
fn refuse_constructs(l: &Line<'_>, kind: Kind) -> Result<(), FormatRefusal> {
    let t = l.text.trim();
    let word = first_word(t);
    if matches!(word, "click" | "href" | "callback" | "link" | "links") && t.len() > word.len() {
        return Err(refusal(
            "mermaid.interaction_refused",
            "a click handler is script",
            l.no,
        ));
    }
    let styling = matches!(word, "style" | "classDef" | "linkStyle" | "cssClass")
        || (word == "class" && kind != Kind::Class);
    if styling || t.contains(":::") {
        return Err(refusal(
            "mermaid.styling_refused",
            "styling can hide a node in the drawing while the source holds it",
            l.no,
        ));
    }
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            match chars.get(i + 1) {
                Some('<') => {
                    i += 2;
                    continue;
                }
                Some(c) if c.is_ascii_alphabetic() || *c == '/' || *c == '!' || *c == '?' => {
                    return Err(refusal(
                        "mermaid.html_label_refused",
                        "HTML in a label is markup from claim text",
                        l.no,
                    ));
                }
                _ => {}
            }
        }
        i += 1;
    }
    Ok(())
}

fn header(lines: &[Line<'_>]) -> Result<(Kind, usize), FormatRefusal> {
    let Some(first) = lines.first() else {
        return Err(refusal(
            "mermaid.syntax_error",
            "the source holds no diagram",
            1,
        ));
    };
    let word = first_word(first.text.trim());
    let kind = match word {
        "flowchart" | "graph" => Kind::Flow,
        "sequenceDiagram" => Kind::Sequence,
        "classDiagram" | "classDiagram-v2" => Kind::Class,
        "stateDiagram-v2" | "stateDiagram" => Kind::State,
        "erDiagram" => Kind::Er,
        other => {
            return Err(refusal(
                "mermaid.unsupported_diagram",
                &format!(
                    "{other:?} is not one of flowchart, sequenceDiagram, classDiagram, stateDiagram-v2, erDiagram"
                ),
                first.no,
            ));
        }
    };
    Ok((kind, 1))
}

fn parse(source: &str) -> Result<Vec<String>, FormatRefusal> {
    check_source(source)?;
    let (all, title) = lines(source)?;
    let (kind, body_from) = header(&all)?;
    for l in &all {
        refuse_constructs(l, kind)?;
    }
    let mut model = Model {
        title,
        ..Model::default()
    };
    let body = &all[body_from..];
    match kind {
        Kind::Flow => flow(body, &mut model)?,
        Kind::Sequence => sequence(body, &mut model)?,
        Kind::Class => class(body, &mut model)?,
        Kind::State => state(body, &mut model)?,
        Kind::Er => er(body, &mut model)?,
    }
    Ok(model.finish())
}

fn syntax(l: &Line<'_>, what: &str) -> FormatRefusal {
    refusal("mermaid.syntax_error", what, l.no)
}

// ---------------------------------------------------------------------------
// flowchart
// ---------------------------------------------------------------------------

fn is_id(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

struct Cursor<'a> {
    s: &'a [char],
    i: usize,
}

impl Cursor<'_> {
    fn skip_ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_whitespace()) {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn rest(&self) -> String {
        self.s[self.i..].iter().collect()
    }
}

/// `id` and, when written, the label of its shape.
fn flow_node(c: &mut Cursor<'_>) -> Option<(String, Option<String>)> {
    c.skip_ws();
    let start = c.i;
    while c.peek().is_some_and(is_id) {
        c.i += 1;
    }
    if c.i == start {
        return None;
    }
    let id: String = c.s[start..c.i].iter().collect();
    let mut label = None;
    if c.peek().is_some_and(|ch| matches!(ch, '[' | '(' | '{')) {
        while c
            .peek()
            .is_some_and(|ch| matches!(ch, '[' | '(' | '{' | '/' | '\\'))
        {
            c.i += 1;
        }
        if c.peek() == Some('"') {
            c.i += 1;
            let from = c.i;
            while c.peek().is_some_and(|ch| ch != '"') {
                c.i += 1;
            }
            label = Some(c.s[from..c.i].iter().collect::<String>());
            c.i += 1;
        } else {
            let from = c.i;
            while c.peek().is_some_and(|ch| !matches!(ch, ']' | ')' | '}')) {
                c.i += 1;
            }
            let mut text: String = c.s[from..c.i].iter().collect();
            while text.ends_with('/') || text.ends_with('\\') {
                text.pop();
            }
            label = Some(text);
        }
        while c
            .peek()
            .is_some_and(|ch| matches!(ch, ']' | ')' | '}' | '/' | '\\'))
        {
            c.i += 1;
        }
        label = label.map(|l| l.trim().to_owned()).filter(|l| !l.is_empty());
    }
    Some((id, label))
}

/// An arrow with its optional label; `None` when no arrow starts here.
fn flow_arrow(c: &mut Cursor<'_>) -> Result<Option<Option<String>>, ()> {
    c.skip_ws();
    let start = c.i;
    let run = |c: &mut Cursor<'_>| {
        let from = c.i;
        while c
            .peek()
            .is_some_and(|ch| matches!(ch, '-' | '=' | '.' | '<' | '>' | '~'))
        {
            c.i += 1;
        }
        c.s[from..c.i].iter().collect::<String>()
    };
    let body = run(c);
    if body.is_empty() {
        return Ok(None);
    }
    let strokes = body
        .chars()
        .filter(|ch| matches!(ch, '-' | '=' | '.' | '~'))
        .count();
    if strokes < 2 {
        c.i = start;
        return Ok(None);
    }
    let mut label = None;
    let text_form = matches!(body.as_str(), "--" | "==" | "-." | "<--" | "<==" | "<-.")
        && c.peek().is_some_and(char::is_whitespace);
    if text_form {
        let tail = c.rest();
        let closers = ["-->", "---", "==>", "===", ".->", "-.-", "--x", "--o"];
        let cut = closers
            .iter()
            .filter_map(|p| tail.find(p))
            .min()
            .ok_or(())?;
        label = Some(tail[..cut].trim().to_owned());
        c.i += tail[..cut].chars().count();
        let _ = run(c);
    }
    if matches!(c.peek(), Some('o' | 'x'))
        && c.s
            .get(c.i + 1)
            .is_none_or(|n| n.is_whitespace() || *n == '|')
    {
        c.i += 1;
    }
    if c.peek() == Some('|') {
        c.i += 1;
        let from = c.i;
        while c.peek().is_some_and(|ch| ch != '|') {
            c.i += 1;
        }
        if c.peek() != Some('|') {
            return Err(());
        }
        label = Some(c.s[from..c.i].iter().collect::<String>().trim().to_owned());
        c.i += 1;
    }
    Ok(Some(
        label
            .map(|l| unquote(&l).to_owned())
            .filter(|l| !l.is_empty()),
    ))
}

fn flow(body: &[Line<'_>], m: &mut Model) -> Result<(), FormatRefusal> {
    for l in body {
        let trimmed = l.text.trim();
        let word = first_word(trimmed);
        if matches!(word, "subgraph" | "end" | "direction") {
            continue;
        }
        for statement in trimmed.split(';') {
            let chars: Vec<char> = statement.trim().chars().collect();
            if chars.is_empty() {
                continue;
            }
            let mut c = Cursor { s: &chars, i: 0 };
            let mut previous: Option<Vec<String>> = None;
            let mut previous_label: Option<String> = None;
            loop {
                let mut group = Vec::new();
                loop {
                    let (id, label) =
                        flow_node(&mut c).ok_or_else(|| syntax(l, "a node was expected"))?;
                    m.node(&id, label, l.no)?;
                    group.push(id);
                    c.skip_ws();
                    if c.peek() == Some('&') {
                        c.i += 1;
                    } else {
                        break;
                    }
                }
                if let Some(from) = previous.take() {
                    let label = previous_label.take();
                    for f in &from {
                        for to in &group {
                            m.relate(f, to, l.no)?;
                            m.say(vec![
                                n(f),
                                t(&format!(" {} ", label.as_deref().unwrap_or("leads to"))),
                                n(to),
                                t("."),
                            ]);
                        }
                    }
                }
                match flow_arrow(&mut c)
                    .map_err(|()| syntax(l, "an arrow's label is not closed"))?
                {
                    Some(label) => {
                        previous_label = label;
                        previous = Some(group);
                    }
                    None => break,
                }
            }
            c.skip_ws();
            if c.peek().is_some() {
                return Err(syntax(
                    l,
                    "text the subset does not read follows the statement",
                ));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// sequenceDiagram
// ---------------------------------------------------------------------------

const MESSAGE_ARROWS: [&str; 8] = ["-->>", "->>", "--x", "-x", "--)", "-)", "-->", "->"];

fn sequence(body: &[Line<'_>], m: &mut Model) -> Result<(), FormatRefusal> {
    for l in body {
        let t_ = l.text.trim();
        let word = first_word(t_);
        match word {
            "participant" | "actor" => {
                let rest = t_[word.len()..].trim();
                let (id, alias) = match rest.split_once(" as ") {
                    Some((id, alias)) => (id.trim(), Some(alias.trim().to_owned())),
                    None => (rest, None),
                };
                if id.is_empty() {
                    return Err(syntax(l, "a participant needs a name"));
                }
                m.node(id, alias, l.no)?;
                continue;
            }
            "autonumber" | "activate" | "deactivate" | "loop" | "alt" | "else" | "opt" | "par"
            | "and" | "end" | "rect" | "critical" | "option" | "break" | "box" | "title" => {
                continue;
            }
            "Note" | "note" => {
                let text = t_.split_once(':').map_or("", |(_, x)| x.trim());
                if !text.is_empty() {
                    m.say(vec![t(&format!("Note: {text}."))]);
                }
                continue;
            }
            _ => {}
        }
        let (head, text) = match t_.split_once(':') {
            Some((h, x)) => (h.trim(), x.trim()),
            None => (t_, ""),
        };
        let mut found = None;
        for arrow in MESSAGE_ARROWS {
            if let Some(at) = head.find(arrow) {
                found = Some((at, arrow));
                break;
            }
        }
        let (at, arrow) =
            found.ok_or_else(|| syntax(l, "a participant line or a message was expected"))?;
        let from = head[..at].trim().trim_end_matches(['+', '-']);
        let to = head[at + arrow.len()..]
            .trim()
            .trim_start_matches(['+', '-']);
        if from.is_empty() || to.is_empty() {
            return Err(syntax(l, "a message names its sender and its receiver"));
        }
        m.node(from, None, l.no)?;
        m.node(to, None, l.no)?;
        m.relate(from, to, l.no)?;
        m.say(vec![
            n(from),
            t(" sends "),
            n(to),
            t(if text.is_empty() { "." } else { ": " }),
            t(text),
            t(if text.is_empty() { "" } else { "." }),
        ]);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// classDiagram
// ---------------------------------------------------------------------------

/// Operator, whether the left name is the one the sentence starts with, and the
/// verb it says.
const CLASS_OPS: [(&str, bool, &str); 12] = [
    ("<|--", false, "is a"),
    ("--|>", true, "is a"),
    ("*--", true, "has"),
    ("--*", false, "has"),
    ("o--", true, "has"),
    ("--o", false, "has"),
    ("<|..", false, "implements"),
    ("..|>", true, "implements"),
    ("..>", true, "depends on"),
    ("<..", false, "depends on"),
    ("-->", true, "is associated with"),
    ("<--", false, "is associated with"),
];

fn class_name(raw: &str) -> &str {
    unquote(raw.trim())
}

fn class(body: &[Line<'_>], m: &mut Model) -> Result<(), FormatRefusal> {
    let mut depth = 0_usize;
    for l in body {
        let t_ = l.text.trim();
        if depth > 0 {
            if t_.starts_with('}') {
                depth -= 1;
            }
            continue;
        }
        let word = first_word(t_);
        if matches!(word, "direction" | "note" | "namespace" | "callback") {
            if t_.ends_with('{') {
                depth += 1;
            }
            continue;
        }
        if word == "class" {
            let rest = t_["class".len()..].trim().trim_end_matches('{').trim();
            let name = rest.split_whitespace().next().unwrap_or("");
            if name.is_empty() {
                return Err(syntax(l, "a class needs a name"));
            }
            m.node(name, None, l.no)?;
            if t_.ends_with('{') {
                depth += 1;
            }
            continue;
        }
        if t_.starts_with("<<") {
            continue;
        }
        let (head, label) = match t_.split_once(" : ") {
            Some((h, x)) => (h.trim(), Some(x.trim())),
            None => (t_, None),
        };
        let op = CLASS_OPS
            .iter()
            .filter_map(|(op, forward, verb)| head.find(op).map(|at| (at, *op, *forward, *verb)))
            .min_by_key(|(at, ..)| *at);
        let Some((at, op, forward, verb)) = op else {
            // `Name : member` and a bare `Name` declare a class.
            let name = head.split_whitespace().next().unwrap_or("");
            if name.is_empty() || head.contains("--") || head.contains("..") {
                return Err(syntax(l, "a class, a member or a relation was expected"));
            }
            m.node(name, None, l.no)?;
            continue;
        };
        let strip = |s: &str| -> String {
            let s = s.trim();
            let s = match s.rfind('"') {
                Some(end) if s.starts_with('"') && end == 0 => s,
                _ => s,
            };
            // A cardinality in quotes beside the name is not part of it.
            let words: Vec<&str> = s
                .split_whitespace()
                .filter(|w| !w.starts_with('"') && !w.ends_with('"'))
                .collect();
            class_name(words.last().copied().unwrap_or("")).to_owned()
        };
        let (left, right) = (strip(&head[..at]), strip(&head[at + op.len()..]));
        if left.is_empty() || right.is_empty() {
            return Err(syntax(l, "a relation names two classes"));
        }
        m.node(&left, None, l.no)?;
        m.node(&right, None, l.no)?;
        m.relate(&left, &right, l.no)?;
        let (a, b) = if forward {
            (&left, &right)
        } else {
            (&right, &left)
        };
        let verb = match (label, verb) {
            (Some(w), "is associated with") => w.to_owned(),
            _ => verb.to_owned(),
        };
        m.say(vec![n(a), t(&format!(" {verb} ")), n(b), t(".")]);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// stateDiagram-v2
// ---------------------------------------------------------------------------

fn state(body: &[Line<'_>], m: &mut Model) -> Result<(), FormatRefusal> {
    for l in body {
        let t_ = l.text.trim();
        let word = first_word(t_);
        if matches!(word, "direction" | "note" | "end" | "--") || t_ == "}" || t_ == "--" {
            continue;
        }
        if word == "state" {
            let rest = t_["state".len()..].trim().trim_end_matches('{').trim();
            if let Some((desc, id)) = rest.rsplit_once(" as ") {
                m.node(id.trim(), Some(unquote(desc.trim()).to_owned()), l.no)?;
            } else if let Some(id) = rest.split_whitespace().next() {
                m.node(id, None, l.no)?;
            }
            continue;
        }
        if let Some((from, rest)) = t_.split_once("-->") {
            let (to, label) = match rest.split_once(':') {
                Some((to, w)) => (to.trim(), Some(w.trim())),
                None => (rest.trim(), None),
            };
            let from = from.trim();
            if from.is_empty() || to.is_empty() {
                return Err(syntax(l, "a transition names two states"));
            }
            let mut ids = Vec::new();
            for (i, raw) in [from, to].into_iter().enumerate() {
                if raw == "[*]" {
                    let id = if i == 0 { "start" } else { "end" };
                    m.node(id, None, l.no)?;
                    ids.push(id.to_owned());
                } else {
                    m.node(raw, None, l.no)?;
                    ids.push(raw.to_owned());
                }
            }
            m.relate(&ids[0], &ids[1], l.no)?;
            let mut s = vec![t("From "), n(&ids[0]), t(" to "), n(&ids[1])];
            match label {
                Some(w) if !w.is_empty() => s.push(t(&format!(" on {w}."))),
                _ => s.push(t(".")),
            }
            m.say(s);
            continue;
        }
        if let Some((id, desc)) = t_.split_once(':') {
            let id = id.trim();
            if id.is_empty() {
                return Err(syntax(l, "a state description names its state"));
            }
            m.node(id, None, l.no)?;
            m.say(vec![n(id), t(&format!(": {}.", desc.trim()))]);
            continue;
        }
        if t_.chars().all(|c| is_id(c) || c == '-') && !t_.is_empty() {
            m.node(t_, None, l.no)?;
            continue;
        }
        return Err(syntax(
            l,
            "a state, a description or a transition was expected",
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// erDiagram
// ---------------------------------------------------------------------------

fn er_left(s: &str) -> Option<&'static str> {
    Some(match s {
        "||" => "one",
        "|o" => "zero or one",
        "}o" => "many",
        "}|" => "one or more",
        _ => return None,
    })
}

fn er_right(s: &str) -> Option<&'static str> {
    Some(match s {
        "||" => "one",
        "o|" => "zero or one",
        "o{" => "many",
        "|{" => "one or more",
        _ => return None,
    })
}

fn er(body: &[Line<'_>], m: &mut Model) -> Result<(), FormatRefusal> {
    let mut inside = false;
    for l in body {
        let t_ = l.text.trim();
        if inside {
            if t_.starts_with('}') {
                inside = false;
            }
            continue;
        }
        if t_.ends_with('{') && !t_.contains(':') {
            let name = t_.trim_end_matches('{').trim();
            m.node(unquote(name), None, l.no)?;
            inside = true;
            continue;
        }
        let parts: Vec<&str> = t_.split_whitespace().collect();
        if parts.len() == 1 {
            m.node(unquote(parts[0]), None, l.no)?;
            continue;
        }
        let (Some(rel), true) = (parts.get(1), parts.get(3) == Some(&":")) else {
            return Err(syntax(
                l,
                "an entity or a relation `A ||--o{ B : label` was expected",
            ));
        };
        let (a, b) = (unquote(parts[0]), unquote(parts[2]));
        let ok = rel.len() == 6 && rel.is_char_boundary(2) && rel.is_char_boundary(4);
        let (lc, mid, rc) = if ok {
            (&rel[..2], &rel[2..4], &rel[4..])
        } else {
            ("", "", "")
        };
        let (Some(left), Some(right), true) =
            (er_left(lc), er_right(rc), matches!(mid, "--" | ".."))
        else {
            return Err(syntax(
                l,
                "the cardinality symbols are not ||, |o, }o, }| and o|, o{, |{",
            ));
        };
        let label = unquote(&parts[4..].join(" ")).to_owned();
        m.node(a, None, l.no)?;
        m.node(b, None, l.no)?;
        m.relate(a, b, l.no)?;
        m.say(vec![
            n(a),
            t(&format!(" {label} ")),
            n(b),
            t(&format!(", {left} to {right}.")),
        ]);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// the adapter
// ---------------------------------------------------------------------------

impl Format for Mermaid {
    fn id(&self) -> &'static str {
        "mermaid/1"
    }
    fn validate(&self, source: &str) -> Result<Parsed, FormatRefusal> {
        let sentences = parse(source)?;
        Ok(Parsed {
            search_text: sentences.join("\n"),
            ..Parsed::default()
        })
    }
    fn search_text(&self, parsed: &Parsed) -> String {
        parsed.search_text.clone()
    }
    fn scan_views(&self, _parsed: &Parsed) -> Vec<ScanView> {
        Vec::new()
    }
    fn render_hint(&self) -> RenderHint {
        RenderHint::Diagram
    }
    fn diff(&self, old: &str, new: &str) -> Result<FormatDiff, FormatRefusal> {
        let count = |v: Vec<String>| {
            let mut m: BTreeMap<String, usize> = BTreeMap::new();
            for s in v {
                *m.entry(s).or_default() += 1;
            }
            m
        };
        let (a, b) = (count(parse(old)?), count(parse(new)?));
        let mut changes = Vec::new();
        for (s, k) in &a {
            for _ in 0..k.saturating_sub(b.get(s).copied().unwrap_or(0)) {
                changes.push(FormatChange {
                    op: "remove",
                    path: String::new(),
                    before: Some(s.clone()),
                    after: None,
                });
            }
        }
        for (s, k) in &b {
            for _ in 0..k.saturating_sub(a.get(s).copied().unwrap_or(0)) {
                changes.push(FormatChange {
                    op: "add",
                    path: String::new(),
                    before: None,
                    after: Some(s.clone()),
                });
            }
        }
        Ok(FormatDiff {
            equivalent: changes.is_empty(),
            changes,
        })
    }
}
