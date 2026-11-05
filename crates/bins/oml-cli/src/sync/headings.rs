//! The `headings` adapter (ADR-0057 d7): generic markdown, one item per
//! heading of level 2 to 4 that has body text before the next heading, its
//! fragment the heading's slug as GitHub renders it and its text the
//! document's title, the heading path and the body. It states no edges.

use super::adapter::{Adapter, Extraction, doc_key_prefix, fit_key, item, key_segment, utf8};
use super::markdown::{
    Slugs, collapse, content_mask, fence_marker, heading, lines, plain, plain_block,
};

pub struct HeadingsAdapter;

impl Adapter for HeadingsAdapter {
    fn name(&self) -> &'static str {
        "headings"
    }

    fn accepts(&self, path: &str) -> bool {
        path.ends_with(".md")
    }

    fn extract(&self, path: &str, bytes: &[u8]) -> Result<Extraction, String> {
        let doc = utf8(path, bytes)?;
        let lines = lines(&doc);
        let mask = content_mask(&lines);
        // Every heading outside fences and comments: (line index, level, text).
        let mut headings = Vec::new();
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
            if let Some((level, text)) = heading(line.text) {
                headings.push((i, level, collapse(&plain(text))));
            }
        }
        let title = headings
            .iter()
            .find(|h| h.1 == 1)
            .map(|h| h.2.clone())
            .unwrap_or_else(|| {
                let name = path.rsplit('/').next().unwrap_or(path);
                name.strip_suffix(".md").unwrap_or(name).to_owned()
            });
        let prefix = doc_key_prefix(path);
        let mut slugs = Slugs::default();
        let mut stack: Vec<(usize, String)> = Vec::new();
        let mut keys = std::collections::HashSet::new();
        let mut out = Extraction::default();
        for (n, (at, level, text)) in headings.iter().enumerate() {
            let slug = slugs.next(text);
            stack.retain(|(l, _)| l < level);
            stack.push((*level, text.clone()));
            if !(2..=4).contains(level) {
                continue;
            }
            let next = headings.get(n + 1).map_or(lines.len(), |h| h.0);
            let body: Vec<&str> = (at + 1..next)
                .filter(|&j| mask[j])
                .map(|j| lines[j].text)
                .collect();
            let body_text = plain_block(&body);
            if body_text.trim().is_empty() {
                continue;
            }
            let mut last = next;
            while last > at + 1 && lines[last - 1].text.trim().is_empty() {
                last -= 1;
            }
            let span: Vec<&str> = (*at..last).map(|j| lines[j].text).collect();
            let path_line = std::iter::once(title.as_str())
                .chain(
                    stack
                        .iter()
                        .filter(|(l, _)| *l >= 2)
                        .map(|(_, t)| t.as_str()),
                )
                .collect::<Vec<_>>()
                .join(" > ");
            let mut key = fit_key(&format!("{prefix}/{}", key_segment(&slug)));
            let mut k = 1;
            while !keys.insert(key.clone()) {
                key = fit_key(&format!("{prefix}/{}-{k}", key_segment(&slug)));
                k += 1;
            }
            out.items.push(item(
                key,
                Some(slug),
                &format!("{path_line}\n\n{body_text}"),
                &span.join("\n"),
                (lines[*at].number, lines[last - 1].number),
            ));
        }
        Ok(out)
    }
}
