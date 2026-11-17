//! ADR-0084 d1, d4, d5, d7 and d13: what a bundle is, and every refusal a
//! reader names.

mod common;

use common::*;
use oml_bundle::crypt;
use oml_bundle::{VerifyOptions, VersionEntry, key_fingerprint, read_bundle, write_bundle};

#[test]
fn a_bundle_written_and_read_back_holds_the_same_records() {
    let verified = read(&tar_bytes()).expect("the fixture verifies");
    let mut expected = contents();
    let written = &verified.contents.manifest;
    expected.manifest.files = written.files.clone();
    expected.manifest.signature = written.signature.clone();
    assert_eq!(verified.contents, expected);
    assert_eq!(
        verified.signer_fingerprint,
        key_fingerprint(&test_key().verifying_key())
    );
    assert!(
        !verified.authenticated,
        "without --expect-key the signature proves integrity only"
    );
}

#[test]
fn the_entries_follow_the_fixed_layout_with_bodies_by_ascending_digest() {
    let names: Vec<String> = entries(&tar_bytes()).into_iter().map(|(p, _)| p).collect();
    let mut bodies: Vec<String> = contents()
        .bodies
        .keys()
        .map(|h| format!("bodies/{h}"))
        .collect();
    bodies.sort();
    let mut expected: Vec<String> = [
        "manifest.json",
        "workspaces.jsonl",
        "versions.jsonl",
        "verdicts.jsonl",
        "events.jsonl",
        "edges.jsonl",
    ]
    .map(str::to_owned)
    .to_vec();
    expected.extend(bodies);
    assert_eq!(names, expected);
}

#[test]
fn the_same_contents_always_make_the_same_tar() {
    assert_eq!(tar_bytes(), tar_bytes());
}

#[test]
fn an_encrypted_bundle_opens_with_each_recipients_identity_and_verifies() {
    let one = age::x25519::Identity::generate();
    let two = age::x25519::Identity::generate();
    let r1 = one.to_public().to_string();
    let r2 = two.to_public().to_string();
    let sealed = crypt::encrypt(&tar_bytes(), &[&r1, &r2]).expect("encrypts");
    assert_ne!(&sealed[..], &tar_bytes()[..]);
    for identity in [&one, &two] {
        use age::secrecy::ExposeSecret;
        let secret = identity.to_string();
        let opened = crypt::decrypt(&sealed, secret.expose_secret()).expect("opens");
        assert_eq!(opened, tar_bytes());
        read(&opened).expect("verifies after decryption");
    }
}

#[test]
fn encrypting_to_no_recipient_is_refused_as_unencrypted() {
    let err = crypt::encrypt(&tar_bytes(), &[]).expect_err("refused");
    assert_eq!(err.code(), "bundle.unencrypted");
}

#[test]
fn a_recipient_that_is_not_an_age_key_is_refused_as_unencrypted() {
    let err = crypt::encrypt(&tar_bytes(), &["not-a-key"]).expect_err("refused");
    assert_eq!(err.code(), "bundle.unencrypted");
}

#[test]
fn a_plaintext_tar_handed_to_decryption_is_refused_as_unencrypted() {
    use age::secrecy::ExposeSecret;
    let identity = age::x25519::Identity::generate().to_string();
    let err = crypt::decrypt(&tar_bytes(), identity.expose_secret()).expect_err("refused");
    assert_eq!(err.code(), "bundle.unencrypted");
}

#[test]
fn an_identity_that_is_no_recipient_is_refused_and_not_named() {
    use age::secrecy::ExposeSecret;
    let recipient = age::x25519::Identity::generate().to_public().to_string();
    let sealed = crypt::encrypt(&tar_bytes(), &[&recipient]).expect("encrypts");
    let stranger = age::x25519::Identity::generate().to_string();
    let err = crypt::decrypt(&sealed, stranger.expose_secret()).expect_err("refused");
    assert_eq!(err.code(), "bundle.decrypt_failed");
    assert!(!err.to_string().contains(stranger.expose_secret()));
    assert!(!format!("{err:?}").contains("AGE-SECRET-KEY"));
}

#[test]
fn a_recipient_fingerprint_is_the_digest_of_the_recipient() {
    let recipient = age::x25519::Identity::generate().to_public().to_string();
    let fp = crypt::recipient_fingerprint(&recipient).expect("parses");
    assert_eq!(fp, oml_bundle::sha256_digest(recipient.as_bytes()));
}

#[test]
fn an_unknown_format_is_refused_as_format_unknown() {
    let tar = with_entry("manifest.json", |b| {
        String::from_utf8(b)
            .unwrap()
            .replace("oml-bundle/1", "oml-bundle/2")
            .into_bytes()
    });
    assert_eq!(code(read(&tar)), "bundle.format_unknown");
}

#[test]
fn a_member_out_of_order_is_refused_as_layout() {
    let mut e = entries(&tar_bytes());
    e.swap(1, 2);
    assert_eq!(code(read(&repack(&e))), "bundle.layout");
}

#[test]
fn an_entry_outside_the_layout_is_refused_as_layout() {
    let mut e = entries(&tar_bytes());
    e.push(("notes.txt".to_owned(), b"hello".to_vec()));
    assert_eq!(code(read(&repack(&e))), "bundle.layout");
}

#[test]
fn a_missing_line_file_is_refused_as_layout() {
    let e: Vec<_> = entries(&tar_bytes()).into_iter().take(3).collect();
    assert_eq!(code(read(&repack(&e))), "bundle.layout");
}

#[test]
fn a_path_traversal_is_refused_as_path_unsafe() {
    let mut e = entries(&tar_bytes());
    e[1].0 = "../workspaces.jsonl".to_owned();
    assert_eq!(code(read(&repack(&e))), "bundle.path_unsafe");
}

#[test]
fn an_absolute_path_is_refused_as_path_unsafe() {
    let mut e = entries(&tar_bytes());
    e[0].0 = "/manifest.json".to_owned();
    assert_eq!(code(read(&repack(&e))), "bundle.path_unsafe");
}

#[test]
fn a_symlink_or_a_hard_link_is_refused_as_path_unsafe() {
    for kind in [tar::EntryType::Symlink, tar::EntryType::Link] {
        let mut e: Vec<_> = entries(&tar_bytes())
            .into_iter()
            .map(|(p, b)| (p, b, tar::EntryType::Regular))
            .collect();
        e[1].2 = kind;
        e[1].1.clear();
        assert_eq!(code(read(&raw_tar(&e))), "bundle.path_unsafe");
    }
}

#[test]
fn an_oversized_member_is_refused_as_too_large() {
    let options = VerifyOptions {
        limits: oml_bundle::Limits {
            max_entry_bytes: 64 * 1024,
            ..oml_bundle::Limits::default()
        },
        expect_key: None,
    };
    let err = read_bundle(&tar_bytes()[..], &options).expect_err("refused");
    assert_eq!(err.code(), "bundle.too_large");
}

#[test]
fn a_bundle_over_its_total_size_or_entry_count_is_refused_as_too_large() {
    for limits in [
        oml_bundle::Limits {
            max_total_bytes: 10_000,
            ..oml_bundle::Limits::default()
        },
        oml_bundle::Limits {
            max_entries: 7,
            ..oml_bundle::Limits::default()
        },
    ] {
        let options = VerifyOptions {
            limits,
            expect_key: None,
        };
        let err = read_bundle(&tar_bytes()[..], &options).expect_err("refused");
        assert_eq!(err.code(), "bundle.too_large");
    }
}

#[test]
fn a_line_over_two_mebibytes_is_refused_as_line_too_long() {
    let tar = with_entry("edges.jsonl", |mut b| {
        b.extend_from_slice(format!("\"{}\"\n", "y".repeat(2 * 1024 * 1024 + 1)).as_bytes());
        b
    });
    assert_eq!(code(read(&tar)), "bundle.line_too_long");
}

#[test]
fn nesting_deeper_than_thirty_two_is_refused_as_depth() {
    let tar = with_entry("edges.jsonl", |mut b| {
        b.extend_from_slice(format!("{}{}\n", "[".repeat(33), "]".repeat(33)).as_bytes());
        b
    });
    assert_eq!(code(read(&tar)), "bundle.depth");
}

#[test]
fn a_duplicate_version_is_refused_as_duplicate_by_the_writer_and_the_reader() {
    let mut c = contents();
    c.versions.push(c.versions[2].clone());
    let err = write_bundle(&c, &test_key(), Vec::new()).expect_err("the writer refuses");
    assert_eq!(err.code(), "bundle.duplicate");

    let tar = with_entry("versions.jsonl", |b| {
        let first = b.split(|&c| c == b'\n').next().unwrap().to_vec();
        let mut out = b.clone();
        out.extend_from_slice(&first);
        out.push(b'\n');
        out
    });
    assert_eq!(code(read(&tar)), "bundle.duplicate");
}

#[test]
fn a_duplicate_body_is_refused_as_duplicate() {
    let mut e = entries(&tar_bytes());
    let last = e.last().unwrap().clone();
    e.push(last);
    assert_eq!(code(read(&repack(&e))), "bundle.duplicate");
}

#[test]
fn a_body_no_line_references_is_refused_as_unreferenced_body() {
    let stray = b"nobody points here".to_vec();
    let mut c = contents();
    c.bodies
        .insert(hex_of(&oml_bundle::sha256_digest(&stray)), stray.clone());
    let err = write_bundle(&c, &test_key(), Vec::new()).expect_err("the writer refuses");
    assert_eq!(err.code(), "bundle.unreferenced_body");

    let mut e = entries(&tar_bytes());
    e.push((format!("bodies/{}", "f".repeat(64)), stray));
    assert_eq!(code(read(&repack(&e))), "bundle.unreferenced_body");
}

#[test]
fn a_tampered_member_is_refused_as_digest_mismatch() {
    let tar = with_entry("versions.jsonl", |b| {
        String::from_utf8(b)
            .unwrap()
            .replace("Tuesdays", "Fridays!")
            .into_bytes()
    });
    assert_eq!(code(read(&tar)), "bundle.digest_mismatch");
}

#[test]
fn a_tampered_body_is_refused_as_digest_mismatch() {
    let mut e = entries(&tar_bytes());
    let last = e.last_mut().unwrap();
    last.1[0] ^= 1;
    assert_eq!(code(read(&repack(&e))), "bundle.digest_mismatch");
}

#[test]
fn a_tampered_manifest_is_refused_as_signature_invalid() {
    let tar = with_entry("manifest.json", |b| {
        String::from_utf8(b)
            .unwrap()
            .replace("\"acme\"", "\"acmf\"")
            .into_bytes()
    });
    assert_eq!(code(read(&tar)), "bundle.signature_invalid");
}

#[test]
fn a_tampered_signature_is_refused_as_signature_invalid() {
    let tar = with_entry("manifest.json", |b| {
        let mut m: serde_json::Value = serde_json::from_slice(&b).unwrap();
        let sig = m["signature"]["sig"].as_str().unwrap().to_owned();
        let flipped = if sig.ends_with('0') { "1" } else { "0" };
        m["signature"]["sig"] = format!("{}{flipped}", &sig[..sig.len() - 1]).into();
        serde_json::to_vec(&m).unwrap()
    });
    assert_eq!(code(read(&tar)), "bundle.signature_invalid");
}

#[test]
fn a_manifest_re_signed_by_another_key_is_refused_only_under_expect_key() {
    let other = ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32]);
    let mut c = contents();
    c.manifest.event_keys = vec![hex::encode(test_key().verifying_key().as_bytes())];
    let mut tar = Vec::new();
    write_bundle(&c, &other, &mut tar).expect("writes");
    read(&tar).expect("integrity alone verifies under any key");

    let expected = key_fingerprint(&test_key().verifying_key());
    let options = VerifyOptions {
        expect_key: Some(expected.clone()),
        ..VerifyOptions::default()
    };
    let err = read_bundle(&tar[..], &options).expect_err("refused");
    assert_eq!(err.code(), "bundle.signature_unexpected");

    let options = VerifyOptions {
        expect_key: Some(expected.to_uppercase()),
        ..VerifyOptions::default()
    };
    let verified = read_bundle(&tar_bytes()[..], &options).expect("the expected key verifies");
    assert!(verified.authenticated);
}

#[test]
fn an_event_of_another_tenant_is_refused_as_signature_invalid() {
    let mut c = contents();
    c.events[0].tenant_id = "00000000-0000-4000-8000-0000000000bb".to_owned();
    let mut tar = Vec::new();
    write_bundle(&c, &test_key(), &mut tar).expect("writes");
    assert_eq!(code(read(&tar)), "bundle.signature_invalid");
}

#[test]
fn an_event_signed_by_no_event_key_is_refused_as_signature_invalid() {
    let mut c = contents();
    let VersionEntry::Version(v) = &c.versions[0] else {
        unreachable!()
    };
    c.events[0] = event_for(
        v,
        &ed25519_dalek::SigningKey::from_bytes(&[9_u8; 32]),
        "evt-1",
    );
    let mut tar = Vec::new();
    write_bundle(&c, &test_key(), &mut tar).expect("writes");
    assert_eq!(code(read(&tar)), "bundle.signature_invalid");
}

#[test]
fn a_version_whose_event_signed_other_text_is_refused_as_digest_mismatch() {
    let mut c = contents();
    let mut other = match &c.versions[0] {
        VersionEntry::Version(v) => (**v).clone(),
        VersionEntry::Erased(_) => unreachable!(),
    };
    other.plaintext_digest = oml_bundle::sha256_digest(b"what the source signed");
    c.events[0] = event_for(&other, &test_key(), "evt-1");
    let mut tar = Vec::new();
    write_bundle(&c, &test_key(), &mut tar).expect("writes");
    assert_eq!(code(read(&tar)), "bundle.digest_mismatch");
}

#[test]
fn an_event_on_another_persons_version_is_refused_as_malformed() {
    let mut c = contents();
    if let VersionEntry::Version(v) = &mut c.versions[0] {
        v.author = author("key-bo");
    }
    let err = write_bundle(&c, &test_key(), Vec::new()).expect_err("refused");
    assert_eq!(err.code(), "bundle.malformed");
    assert!(err.to_string().contains("event_on_foreign_version"));
}

#[test]
fn an_erasure_marker_carrying_a_digest_is_refused_as_malformed() {
    let tar = with_entry("versions.jsonl", |b| {
        String::from_utf8(b)
            .unwrap()
            .replace(
                "\"erased\":true",
                "\"erased\":true,\"plaintext_digest\":\"sha256:00\"",
            )
            .into_bytes()
    });
    let err = read(&tar).expect_err("refused");
    assert_eq!(err.code(), "bundle.malformed");
    assert!(err.to_string().contains("erasure_marker_content"));
}

#[test]
fn an_erasure_marker_carries_only_its_artifact_version_and_erasure() {
    let versions = entries(&tar_bytes())
        .into_iter()
        .find(|(p, _)| p == "versions.jsonl")
        .unwrap()
        .1;
    let marker = String::from_utf8(versions)
        .unwrap()
        .lines()
        .find(|l| l.contains("erased"))
        .unwrap()
        .to_owned();
    assert_eq!(
        marker,
        r#"{"artifact_id":"art-4","erased":true,"version":1}"#
    );
}

#[test]
fn an_unknown_member_is_refused_as_malformed() {
    let tar = with_entry("edges.jsonl", |b| {
        String::from_utf8(b)
            .unwrap()
            .replace("\"label\"", "\"weight\":1,\"label\"")
            .into_bytes()
    });
    let err = read(&tar).expect_err("refused");
    assert_eq!(err.code(), "bundle.malformed");
    assert!(err.to_string().contains("unknown field"));
}

#[test]
fn a_malformed_line_names_no_content() {
    let tar = with_entry("edges.jsonl", |b| {
        String::from_utf8(b)
            .unwrap()
            .replace("\"supports\"", "987654321")
            .into_bytes()
    });
    let err = read(&tar).expect_err("refused");
    assert_eq!(err.code(), "bundle.malformed");
    assert!(!err.to_string().contains("987654321"), "{err}");
}

#[test]
fn a_release_bundle_holding_a_held_version_is_refused_as_malformed() {
    let mut c = contents();
    c.manifest.mode = oml_bundle::Mode::Release;
    c.manifest.release = Some(oml_bundle::ReleaseRef {
        label: "1.0.0".to_owned(),
        manifest_digest: oml_bundle::sha256_digest(b"release"),
    });
    if let VersionEntry::Version(v) = &mut c.versions[1] {
        v.state = oml_bundle::State::Held;
    }
    let err = write_bundle(&c, &test_key(), Vec::new()).expect_err("refused");
    assert_eq!(err.code(), "bundle.malformed");
}

#[test]
fn text_over_sixty_four_kibibytes_inline_is_refused_as_malformed() {
    let mut c = contents();
    if let VersionEntry::Version(v) = &mut c.versions[2] {
        let text = "z".repeat(64 * 1024 + 1);
        v.plaintext_digest = oml_bundle::sha256_digest(text.as_bytes());
        v.text = Some(text);
    }
    let err = write_bundle(&c, &test_key(), Vec::new()).expect_err("refused");
    assert_eq!(err.code(), "bundle.malformed");
}

#[test]
fn a_pseudonym_is_stable_inside_one_bundle_and_differs_across_bundles() {
    let a = oml_bundle::pseudonym(b"salt one", "key-ada");
    assert_eq!(a, oml_bundle::pseudonym(b"salt one", "key-ada"));
    assert_ne!(a, oml_bundle::pseudonym(b"salt two", "key-ada"));
    assert_ne!(a, oml_bundle::pseudonym(b"salt one", "key-bo"));
    assert!(!a.contains("key-ada"));
}

#[test]
fn the_import_idempotency_key_names_target_source_artifact_and_version() {
    let key = oml_bundle::import_idempotency_key("ws-9", &manifest().source, "art-1", 3);
    assert_eq!(
        key,
        "import:ws-9:00000000-0000-4000-8000-0000000000f1:00000000-0000-4000-8000-0000000000aa:art-1:3"
    );
}
