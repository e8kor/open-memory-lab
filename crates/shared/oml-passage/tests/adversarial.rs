//! A crafted document cannot crash or stall the cut (the review's finding
//! S1, S2 of 2026-11-18): each 1 MiB input is cut, or refused, in linear
//! time and bounded memory, on a thread with a small stack.

use oml_passage::{Format, markdown::plain, passages, search_text};
use std::time::{Duration, Instant};

/// Generous for a debug build on a shared host; a quadratic cut of 1 MiB
/// takes minutes, and a recursive one overflows the stack.
const BUDGET: Duration = Duration::from_secs(5);

/// Runs `f` on a thread with a 256 KiB stack and asserts it ends within
/// [`BUDGET`].
fn within_budget(what: &str, f: impl FnOnce() + Send + 'static) {
    let started = Instant::now();
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(f)
        .expect("spawn")
        .join()
        .unwrap_or_else(|_| panic!("{what}: the cut panicked"));
    let took = started.elapsed();
    assert!(took < BUDGET, "{what}: {took:?}");
}

#[test]
fn links_nested_10k_deep_are_read_without_recursion() {
    let nested = format!("{}x{}", "[".repeat(10_000), "](u)".repeat(10_000));
    let doc = format!("# {nested}\n\nbody\n");
    within_budget("nested links", move || {
        assert_eq!(plain(&nested), "x");
        let cut = passages(&doc, Format::Markdown).expect("cut");
        assert_eq!(cut.len(), 1);
    });
}

#[test]
fn a_heading_of_a_million_open_brackets_is_cut_in_linear_time() {
    let doc = format!("# {}\n\nbody\n", "[".repeat(1_000_000));
    within_budget("open brackets", move || {
        let cut = passages(&doc, Format::Markdown).expect("cut");
        assert_eq!(cut.len(), 1);
        assert!(cut[0].heading_path[0].chars().count() <= 200);
    });
}

#[test]
fn a_million_unclosed_angle_brackets_are_read_in_linear_time() {
    let line = "<".repeat(1_000_000);
    let doc = format!("# {line}\n\nbody\n");
    within_budget("unclosed <", move || {
        assert_eq!(plain(&line).len(), 1_000_000);
        passages(&doc, Format::Markdown).expect("cut");
    });
}

#[test]
fn half_a_million_empty_headings_are_cut_in_linear_time() {
    let doc = "#\n".repeat(524_288);
    within_budget("empty headings", move || {
        // Past MAX_HEADINGS the cut stops and refuses the document.
        let err = passages(&doc, Format::Markdown).unwrap_err();
        assert_eq!(err.count, oml_passage::MAX_HEADINGS + 1, "refused past the headings bound");
    });
}

#[test]
fn a_million_backtick_runs_of_rising_length_are_read_in_linear_time() {
    let mut line = String::new();
    for n in 1..=1_400 {
        line.push_str(&"`".repeat(n));
        line.push('x');
    }
    within_budget("backtick runs", move || {
        let _ = plain(&line);
    });
}

#[test]
fn a_500_kb_title_heading_over_4000_sections_keeps_each_path_entry_to_200_characters() {
    let mut doc = format!("# {}\n\nIntro.\n\n", "w".repeat(500_000));
    for n in 0..4_000 {
        doc.push_str(&format!("## Part {n}\n\nText {n}.\n\n"));
    }
    within_budget("long title", move || {
        let cut = passages(&doc, Format::Markdown).expect("cut");
        assert_eq!(cut.len(), 4_001);
        let mut total = 0;
        for p in &cut {
            for h in &p.heading_path {
                assert!(h.chars().count() <= 200, "{}", h.len());
            }
            total += search_text("Title", &doc, p).len();
        }
        assert!(total < 8 * 1024 * 1024, "search texts total {total} bytes");
    });
}

#[test]
fn a_cut_past_the_passage_cap_stops_there() {
    let doc = "## h\nx\n".repeat(300_000);
    within_budget("passage cap", move || {
        let err = passages(&doc, Format::Markdown).unwrap_err();
        assert!(err.count > oml_passage::MAX_PASSAGES);
    });
}
