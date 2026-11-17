//! The fixture every suite builds on: a small history-mode bundle with an
//! inline version, a body over 64 KiB, an attachment, an erasure marker, a
//! verdict, an edge and the exporter's own source event, signed by a test key
//! derived here from a fixed seed. Never a real key.
#![allow(dead_code)]

use std::collections::BTreeMap;

use ed25519_dalek::{Signer, SigningKey};
use oml_bundle::{
    Attachment, Author, BundleContents, EdgeKind, EdgeLine, ErasureMarker, EventLine, Manifest,
    Mode, Source, State, Verdict, VerdictLine, VersionEntry, VersionLine, WorkspaceLine, pseudonym,
    sha256_digest,
};
use serde_json::json;

pub const SALT: &[u8] = b"test salt, drawn fresh per bundle in production";
pub const TENANT: &str = "00000000-0000-4000-8000-0000000000aa";

/// The test journal key: a fixed seed, so the committed vector reproduces.
pub fn test_key() -> SigningKey {
    SigningKey::from_bytes(&[42_u8; 32])
}

pub fn author(key_id: &str) -> Author {
    Author {
        used_by: "person".to_owned(),
        same_user: false,
        pseudonym: pseudonym(SALT, key_id),
    }
}

pub fn version(artifact_id: &str, version: u64, claim_key: &str, text: &str) -> VersionLine {
    VersionLine {
        artifact_id: artifact_id.to_owned(),
        version,
        claim_key: claim_key.to_owned(),
        workspace: "universe.ada.notes".to_owned(),
        kind: "memory".to_owned(),
        format: None,
        title: None,
        state: State::Promoted,
        retirement: false,
        replaces: None,
        simulation_id: None,
        text: Some(text.to_owned()),
        body: None,
        plaintext_digest: sha256_digest(text.as_bytes()),
        anchor: serde_json::Value::Null,
        valid_from: None,
        valid_until: None,
        skill: None,
        document: None,
        test: None,
        derives_from: Vec::new(),
        origin: json!({"kind": "manual"}),
        attachments: Vec::new(),
        author: author("key-ada"),
        created_at: "2026-11-01T10:00:00.000000Z".to_owned(),
        promoted_at: Some("2026-11-01T11:00:00.000000Z".to_owned()),
        source_event: None,
    }
}

/// A source event over `v`, signed by `key` as the chain signs it.
pub fn event_for(v: &VersionLine, key: &SigningKey, event_id: &str) -> EventLine {
    let mut event = EventLine {
        tenant_id: TENANT.to_owned(),
        seq: 2,
        event_id: event_id.to_owned(),
        event_type: "submission.remember".to_owned(),
        prev_hash: format!("sha256:{}", "0".repeat(64)),
        principal: "ada/laptop".to_owned(),
        api_key_id: Some("00000000-0000-4000-8000-0000000000c2".to_owned()),
        at: "2026-11-01T10:00:00.000000Z".to_owned(),
        payload: json!({
            "artifact_id": v.artifact_id,
            "version": v.version,
            "claim_key": v.claim_key,
            "plaintext_digest": v.plaintext_digest,
        }),
        hash: String::new(),
        sig: String::new(),
    };
    let bytes = event.signed_bytes().expect("canonical");
    event.hash = sha256_digest(&bytes);
    event.sig = format!("ed25519:{}", hex::encode(key.sign(&bytes).to_bytes()));
    event
}

pub fn manifest() -> Manifest {
    Manifest {
        format: oml_bundle::FORMAT.to_owned(),
        source: Source {
            server_id: "00000000-0000-4000-8000-0000000000f1".to_owned(),
            tenant_id: TENANT.to_owned(),
            tenant_name: "acme".to_owned(),
        },
        workspaces: vec!["universe.ada.notes".to_owned()],
        mode: Mode::History,
        release: None,
        since: None,
        dropped: Vec::new(),
        exported_at: "2026-11-17T09:00:00.000000Z".to_owned(),
        exporter_pseudonym: pseudonym(SALT, "key-ada"),
        event_keys: vec![hex::encode(test_key().verifying_key().as_bytes())],
        files: Vec::new(),
        signature: None,
    }
}

/// The fixture's contents.
pub fn contents() -> BundleContents {
    let key = test_key();
    let mut own = version("art-1", 1, "deploy-runbook", "Deploys go out on Tuesdays.");
    own.source_event = Some("evt-1".to_owned());
    let event = event_for(&own, &key, "evt-1");

    let big = "x".repeat(70 * 1024);
    let mut long = version("art-2", 1, "long-design-note", "");
    long.text = None;
    long.plaintext_digest = sha256_digest(big.as_bytes());
    long.body = Some(long.plaintext_digest.clone());
    long.author = author("key-bo");

    let pdf = b"%PDF-1.7 a small attachment".to_vec();
    let mut with_file = version("art-3", 2, "supplier-quote", "Quote attached.");
    with_file.attachments.push(Attachment {
        digest: sha256_digest(&pdf),
        media_type: "application/pdf".to_owned(),
        size_bytes: pdf.len() as u64,
    });
    with_file.author = author("key-bo");

    let mut bodies = BTreeMap::new();
    bodies.insert(hex_of(&long.plaintext_digest), big.into_bytes());
    bodies.insert(hex_of(&sha256_digest(&pdf)), pdf);

    BundleContents {
        manifest: manifest(),
        workspaces: vec![WorkspaceLine {
            name: "notes".to_owned(),
            path: "universe.ada.notes".to_owned(),
            status: "development".to_owned(),
            parent: None,
        }],
        versions: vec![
            VersionEntry::Version(Box::new(own)),
            VersionEntry::Version(Box::new(long)),
            VersionEntry::Version(Box::new(with_file)),
            VersionEntry::Erased(ErasureMarker {
                artifact_id: "art-4".to_owned(),
                version: 1,
                erased: true,
            }),
        ],
        verdicts: vec![VerdictLine {
            artifact_id: "art-1".to_owned(),
            version: 1,
            verdict: Verdict::Approve,
            reviewer: author("key-cy"),
            approved_at: Some("2026-11-01T11:00:00.000000Z".to_owned()),
            at: "2026-11-01T11:00:00.000000Z".to_owned(),
        }],
        events: vec![event],
        edges: vec![EdgeLine {
            kind: EdgeKind::Claim,
            src: "art-1".to_owned(),
            dst: "art-3".to_owned(),
            label: Some("supports".to_owned()),
        }],
        bodies,
    }
}

pub fn hex_of(digest: &str) -> String {
    digest.trim_start_matches("sha256:").to_owned()
}

/// The fixture written as a signed, plaintext tar.
pub fn tar_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    oml_bundle::write_bundle(&contents(), &test_key(), &mut out).expect("the fixture writes");
    out
}

/// The tar's entries, in order.
pub fn entries(tar: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = tar::Archive::new(tar);
    archive
        .entries()
        .expect("entries")
        .raw(true)
        .map(|e| {
            let mut e = e.expect("entry");
            let path = String::from_utf8_lossy(&e.header().path_bytes()).into_owned();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut e, &mut bytes).expect("read");
            (path, bytes)
        })
        .collect()
}

/// A tar of exactly these entries, the name written raw into the header so
/// that a test can make what the writer never would.
pub fn raw_tar(entries: &[(String, Vec<u8>, tar::EntryType)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes, kind) in entries {
        let mut header = tar::Header::new_ustar();
        let name = &mut header.as_old_mut().name;
        name[..path.len()].copy_from_slice(path.as_bytes());
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_entry_type(*kind);
        if kind.is_symlink() {
            header.set_link_name("manifest.json").expect("link name");
        }
        header.set_cksum();
        builder.append(&header, bytes.as_slice()).expect("append");
    }
    builder.into_inner().expect("finish")
}

/// Re-packs entries as regular files.
pub fn repack(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let regular: Vec<_> = entries
        .iter()
        .map(|(p, b)| (p.clone(), b.clone(), tar::EntryType::Regular))
        .collect();
    raw_tar(&regular)
}

/// The fixture with one entry's bytes replaced, its manifest untouched.
pub fn with_entry(name: &str, edit: impl Fn(Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    let entries: Vec<_> = entries(&tar_bytes())
        .into_iter()
        .map(|(p, b)| if p == name { (p, edit(b)) } else { (p, b) })
        .collect();
    repack(&entries)
}

pub fn read(tar: &[u8]) -> Result<oml_bundle::VerifiedBundle, oml_bundle::BundleError> {
    oml_bundle::read_bundle(tar, &oml_bundle::VerifyOptions::default())
}

pub fn code(result: Result<oml_bundle::VerifiedBundle, oml_bundle::BundleError>) -> &'static str {
    result.expect_err("the bundle is refused").code()
}
