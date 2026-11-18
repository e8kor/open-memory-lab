//! The passage rule (ADR-0081 d3, the build's readings R1 and R2 of
//! 2026-11-18), proved on the shared vector file that `oml-cli`'s
//! `headings` adapter is held to as well.

use oml_passage::{
    Format, MAX_PASSAGES, Passage, TooManyPassages, passage_text, passages, search_text, slug,
};
use serde_json::Value;

const VECTORS: &str = include_str!("vectors/passages.json");

fn passage(v: &Value) -> Passage {
    Passage {
        ordinal: v["ordinal"].as_u64().unwrap() as usize,
        start_line: v["start_line"].as_u64().unwrap() as usize,
        end_line: v["end_line"].as_u64().unwrap() as usize,
        heading_path: v["heading_path"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h.as_str().unwrap().to_owned())
            .collect(),
        fragment: v["fragment"].as_str().unwrap().to_owned(),
    }
}

#[test]
fn the_server_and_sync_cut_the_same_passages_for_the_same_bytes() {
    let vectors: Vec<Value> = serde_json::from_str(VECTORS).unwrap();
    assert!(!vectors.is_empty());
    for v in &vectors {
        let format = match v["format"].as_str().unwrap() {
            "markdown" => Format::Markdown,
            _ => Format::Plain,
        };
        let want: Vec<Passage> = v["passages"]
            .as_array()
            .unwrap()
            .iter()
            .map(passage)
            .collect();
        assert_eq!(
            passages(v["text"].as_str().unwrap(), format).unwrap(),
            want,
            "{}",
            v["name"]
        );
    }
}

#[test]
fn a_markdown_document_is_cut_at_headings_one_to_four() {
    let text = "# A\nIntro.\n## B\nb.\n### C\nc.\n#### D\nd.\n##### E\ne.\n";
    let cut = passages(text, Format::Markdown).unwrap();
    let fragments: Vec<&str> = cut.iter().map(|p| p.fragment.as_str()).collect();
    assert_eq!(fragments, ["a", "b", "c", "d"]);
    // A level-5 heading is text of the level-4 passage that holds it.
    assert_eq!((cut[3].start_line, cut[3].end_line), (7, 10));
    assert_eq!(cut[3].heading_path, ["A", "B", "C", "D"]);
}

#[test]
fn a_heading_with_no_content_line_is_no_passage_and_joins_the_path_below() {
    let text = "# Title\n\n## Only\nBody.\n";
    let cut = passages(text, Format::Markdown).unwrap();
    assert_eq!(cut.len(), 1);
    assert_eq!(cut[0].fragment, "only");
    assert_eq!(cut[0].heading_path, ["Title", "Only"]);
}

#[test]
fn a_section_over_forty_lines_is_split_at_blank_lines() {
    let mut text = String::from("## Big\n");
    for i in 1..=50 {
        text.push_str(if i == 30 { "\n" } else { "word\n" });
    }
    let cut = passages(&text, Format::Markdown).unwrap();
    let spans: Vec<(usize, usize, &str)> = cut
        .iter()
        .map(|p| (p.start_line, p.end_line, p.fragment.as_str()))
        .collect();
    assert_eq!(spans, [(1, 30, "L1-L30"), (32, 51, "L32-L51")]);
    assert!(cut.iter().all(|p| p.end_line - p.start_line < 40));
}

#[test]
fn a_document_over_4096_passages_is_refused_too_many_passages() {
    let mut text = String::new();
    for i in 0..=MAX_PASSAGES {
        text.push_str(&format!("## H{i}\nx\n"));
    }
    assert_eq!(
        passages(&text, Format::Markdown),
        Err(TooManyPassages {
            count: MAX_PASSAGES + 1
        })
    );
    let fits: String = text
        .lines()
        .take(2 * MAX_PASSAGES)
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(
        passages(&fits, Format::Markdown).unwrap().len(),
        MAX_PASSAGES
    );
}

#[test]
fn a_passage_read_is_the_same_span_of_a_full_read() {
    let text = "intro\r\n\r\n## Two\r\nline a\r\nline b\r\n\r\n## Three\r\nend";
    for p in passages(text, Format::Markdown).unwrap() {
        let span: Vec<&str> =
            text.split('\n').collect::<Vec<_>>()[p.start_line - 1..p.end_line].to_vec();
        let want = span.join("\n");
        let want = want.strip_suffix('\r').unwrap_or(&want);
        assert_eq!(passage_text(text, &p), want);
        assert!(text.contains(passage_text(text, &p)));
    }
}

#[test]
fn a_passage_is_searched_by_its_title_and_heading_path_and_served_by_its_lines() {
    let text = "# Deploy guide\n## Install\nRun it.\n";
    let cut = passages(text, Format::Markdown).unwrap();
    assert_eq!(
        search_text("Deploy guide", text, &cut[0]),
        "Deploy guide > Install\n\n## Install\nRun it."
    );
    assert_eq!(passage_text(text, &cut[0]), "## Install\nRun it.");
}

#[test]
fn a_heading_slug_reads_a_link_by_its_text() {
    assert_eq!(slug("[Guide](https://x.y)"), "guide");
    assert_eq!(slug("The `oml` *CLI*: a tour"), "the-oml-cli-a-tour");
}
