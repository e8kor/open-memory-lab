//! The near-copy check measured on the labelled set (ADR-0050 d16, d17):
//! `eval/remember/near-duplicates/data.json`, built by rule from the recall
//! and remember evals. Every probe is checked as a write is: its query keys
//! looked up among the stored rows' keys, at most `MAX_CANDIDATES`
//! candidates verified by the exact relation, at most `MAX_LISTED` named.
//! The gates are d16's; the numbers print with `--nocapture`.

use std::collections::{BTreeMap, BTreeSet};

use oml_domain::similar::{Fingerprint, MAX_CANDIDATES, MAX_LISTED, Relation, relate};
use serde_json::Value;

struct Listed {
    id: String,
    relation: Relation,
    jaccard: f64,
}

/// What a write of `probe` names among `stored`, as the core would.
fn check(
    probe: &Fingerprint,
    stored: &[(String, Fingerprint)],
    index: &BTreeMap<i64, Vec<usize>>,
) -> Vec<Listed> {
    let mut shared: BTreeMap<usize, usize> = BTreeMap::new();
    for (_, key) in probe.query_keys() {
        for &i in index.get(&key).into_iter().flatten() {
            *shared.entry(i).or_default() += 1;
        }
    }
    let mut candidates: Vec<(usize, usize)> = shared.into_iter().collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then(stored[a.0].0.cmp(&stored[b.0].0)));
    candidates.truncate(MAX_CANDIDATES);
    let mut listed: Vec<Listed> = candidates
        .into_iter()
        .filter_map(|(i, _)| {
            relate(probe, &stored[i].1).map(|(relation, jaccard)| Listed {
                id: stored[i].0.clone(),
                relation,
                jaccard,
            })
        })
        .collect();
    listed.sort_by(|a, b| {
        a.relation
            .cmp(&b.relation)
            .then(b.jaccard.total_cmp(&a.jaccard))
            .then(a.id.cmp(&b.id))
    });
    listed.truncate(MAX_LISTED);
    listed
}

#[test]
fn the_near_copy_check_meets_its_gates_on_the_labelled_set() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../eval/remember/near-duplicates/data.json"
    );
    let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let stored: Vec<(String, Fingerprint)> = data["stored"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap().to_owned(),
                Fingerprint::of(r["text"].as_str().unwrap()),
            )
        })
        .collect();
    let mut index: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (i, (_, f)) in stored.iter().enumerate() {
        for (_, key) in f.stored_keys() {
            index.entry(key).or_default().push(i);
        }
    }
    let mut same_as: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for pair in data["same_as"].as_array().unwrap() {
        let (a, b) = (pair[0].as_str().unwrap(), pair[1].as_str().unwrap());
        same_as.entry(a.into()).or_default().insert(b.into());
        same_as.entry(b.into()).or_default().insert(a.into());
    }

    // label -> (probes, target named, target named as same_text)
    let mut by_label: BTreeMap<String, (u32, u32, u32)> = BTreeMap::new();
    let (mut true_listed, mut false_listed) = (0_u32, 0_u32);
    let mut false_examples = Vec::new();
    for p in data["probes"].as_array().unwrap() {
        let label = p["label"].as_str().unwrap().to_owned();
        let target = p["target"].as_str().map(str::to_owned);
        let listed = check(
            &Fingerprint::of(p["text"].as_str().unwrap()),
            &stored,
            &index,
        );
        let entry = by_label.entry(label.clone()).or_default();
        entry.0 += 1;
        let mut accepted: BTreeSet<String> = BTreeSet::new();
        if let Some(t) = &target {
            accepted.insert(t.clone());
            accepted.extend(same_as.get(t).cloned().unwrap_or_default());
        }
        let negative = label == "new" || label == "short";
        if let Some(hit) = listed.iter().find(|l| accepted.contains(&l.id)) {
            entry.1 += 1;
            if hit.relation == Relation::SameText {
                entry.2 += 1;
            }
        }
        for l in &listed {
            if !negative && accepted.contains(&l.id) {
                true_listed += 1;
            } else {
                false_listed += 1;
                false_examples.push(format!("{} -> {}", p["id"], l.id));
            }
        }
    }
    let rate = |label: &str| {
        let (n, found, _) = by_label[label];
        f64::from(found) / f64::from(n)
    };
    let precision = f64::from(true_listed) / f64::from(true_listed + false_listed);
    println!(
        "{}",
        serde_json::json!({
            "by_label": by_label.iter().map(|(k, (n, f, s))| (k.clone(), serde_json::json!({"probes": n, "named": f, "named_same_text": s}))).collect::<BTreeMap<_, _>>(),
            "listed": {"true": true_listed, "false": false_listed},
            "precision": precision,
            "false": false_examples,
        })
    );
    // d16's gates.
    assert!(
        (rate("copy") - 1.0).abs() < f64::EPSILON,
        "same-text recall"
    );
    assert_eq!(
        by_label["copy"].2, by_label["copy"].0,
        "every copy is named same_text"
    );
    assert!(
        rate("near_copy") >= 0.85,
        "near-copy recall {}",
        rate("near_copy")
    );
    assert!(
        precision >= 0.95,
        "precision {precision}: {false_examples:?}"
    );
    assert_eq!(by_label["short"].1, 0, "a short text is never named");
    assert_eq!(by_label["new"].1, 0, "a new fact names nothing");
}
