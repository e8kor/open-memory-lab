//! The state chip names the stored state `promoted` as approved (ADR-0091
//! d16): the word a person reads changes, the stored state and the chip's
//! class and glyph do not.

use oml_page::web::state_tag;

#[test]
fn a_promoted_state_chip_reads_approved_and_keeps_its_class() {
    let chip = state_tag("promoted");
    assert!(chip.contains("is-promoted"), "{chip}");
    assert!(chip.ends_with(">approved</span>"), "{chip}");
    assert!(!chip.contains(">promoted<"), "{chip}");
    assert!(state_tag("held").ends_with(">held</span>"));
}
