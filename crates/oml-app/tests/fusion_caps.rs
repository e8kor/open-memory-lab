//! Each retriever's list is capped in the core before fusion (ADR-0049 d13;
//! ADR-0057 d22): whatever an adapter returns, no list contributes more
//! candidates than the depth the core asked for.

use oml_app::search::{Candidate, Fusion, RRF_K, Retrieved, RetrieverKind, fuse, fuse_capped};
use uuid::Uuid;

fn list(ids: std::ops::Range<u128>) -> Retrieved {
    Retrieved {
        candidates: ids
            .map(|i| Candidate {
                memory_id: Uuid::from_u128(i),
                body_bytes: 10,
                #[allow(clippy::cast_precision_loss, reason = "small test ids")]
                score: 1000.0 - i as f32,
                matched: vec!["word".to_owned()],
                tokens: Vec::new(),
            })
            .collect(),
        ..Retrieved::default()
    }
}

#[test]
fn no_retriever_contributes_more_candidates_than_its_cap() {
    // A flooding arm returns 500 rows for a depth of 50; the other returns
    // 20 rows of its own.
    let flood = list(1000..1500);
    let modest = list(1..21);
    let lists = vec![
        (RetrieverKind::Bm25Postings, flood),
        (RetrieverKind::Trigram, modest),
    ];
    let fused = fuse_capped(lists, Fusion::Rrf { k: RRF_K }, 50);
    let from_flood = fused
        .iter()
        .filter(|f| f.memory_id.as_u128() >= 1000)
        .count();
    assert_eq!(from_flood, 50, "the flooding list is cut at its cap");
    assert_eq!(fused.len(), 70);
    for kind in [
        Fusion::Single,
        Fusion::Cc {
            weights: &[0.5, 0.5],
        },
    ] {
        let lists = vec![(RetrieverKind::Bm25, list(1..300))];
        assert_eq!(fuse_capped(lists, kind, 8).len(), 8, "{kind:?}");
    }
}

#[test]
fn a_list_within_its_cap_fuses_as_before() {
    let lists = vec![
        (RetrieverKind::Bm25Postings, list(1..30)),
        (RetrieverKind::Trigram, list(10..40)),
    ];
    assert_eq!(
        fuse_capped(lists.clone(), Fusion::Rrf { k: RRF_K }, 50),
        fuse(&lists, Fusion::Rrf { k: RRF_K })
    );
}
