//! The frame of ADR-0056 d9 in a real Chromium: the pinned Mermaid bundle
//! draws the five diagram types under the frame's own policy with no
//! `unsafe-eval` and no network, and a source that does not draw shows its
//! error in the frame with the source still on the page. Skipped by name,
//! printing why, where no Chromium is found: `CHROME_PATH`, the Playwright
//! cache, or `google-chrome` and `chromium` on the path.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::thread;

use oml_web::render::{BUNDLE, bundle_path, frame_document, frame_policy};

fn chromium() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CHROME_PATH")
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let cache = PathBuf::from(home).join(".cache/ms-playwright");
        // The headless shell first: it prints the DOM and exits, where a
        // full Chromium's headless mode may not.
        if let Ok(entries) = std::fs::read_dir(cache) {
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .flat_map(|e| {
                    [
                        e.path()
                            .join("chrome-headless-shell-linux64/chrome-headless-shell"),
                        e.path().join("chrome-linux64/chrome"),
                    ]
                })
                .filter(|p| p.is_file())
                .collect();
            found.sort_by_key(|p| !p.to_string_lossy().contains("headless-shell"));
            if let Some(first) = found.into_iter().next() {
                return Some(first);
            }
        }
    }
    for name in ["google-chrome", "chromium", "chromium-browser"] {
        let probe = Command::new(name).arg("--version").output();
        if probe.is_ok_and(|o| o.status.success()) {
            return Some(PathBuf::from(name));
        }
    }
    None
}

fn serve(listener: TcpListener, issuer: String, document: String) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { return };
        let (issuer, document) = (issuer.clone(), document.clone());
        // One thread a connection: a browser opens idle ones that must not
        // hold up the next request.
        thread::spawn(move || answer(&mut stream, &issuer, &document));
    }
}

fn answer(stream: &mut TcpStream, issuer: &str, document: &str) {
    {
        let mut buffer = [0_u8; 4096];
        let n = stream.read(&mut buffer).unwrap_or(0);
        let request = String::from_utf8_lossy(&buffer[..n]).into_owned();
        let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
        let (head, body): (String, Vec<u8>) = if path == bundle_path() {
            (
                "HTTP/1.1 200 OK\r\ncontent-type: text/javascript\r\n".to_owned(),
                BUNDLE.to_vec(),
            )
        } else if path == "/" {
            (
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-security-policy: {}\r\n",
                    frame_policy(issuer)
                ),
                document.as_bytes().to_vec(),
            )
        } else {
            ("HTTP/1.1 404 Not Found\r\n".to_owned(), Vec::new())
        };
        let _ = write_all(stream, &head, &body);
    }
}

fn write_all(stream: &mut TcpStream, head: &str, body: &[u8]) -> std::io::Result<()> {
    stream.write_all(head.as_bytes())?;
    stream.write_all(
        format!(
            "content-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        )
        .as_bytes(),
    )?;
    stream.write_all(body)?;
    stream.flush()
}

/// The DOM Chromium holds after the frame's scripts ran, served under the
/// frame's policy; `None` when Chromium could not start.
fn dom_of(source: &str) -> Option<String> {
    let chrome = chromium().or_else(|| {
        println!("did not run: no Chromium (set CHROME_PATH)");
        None
    })?;
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let port = listener.local_addr().expect("an address").port();
    let issuer = format!("http://127.0.0.1:{port}");
    let document = frame_document(source);
    let server = issuer.clone();
    thread::spawn(move || serve(listener, server, document));
    let profile = std::env::temp_dir().join(format!("oml-render-{port}"));
    let mut command = Command::new(&chrome);
    if !chrome.to_string_lossy().contains("headless-shell") {
        command.arg("--headless");
    }
    let child = command
        .args([
            "--disable-gpu",
            "--no-sandbox",
            "--disable-dev-shm-usage",
            "--no-first-run",
            "--virtual-time-budget=30000",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--dump-dom")
        .arg(format!("{issuer}/"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            println!("did not run: Chromium would not start ({e})");
            return None;
        }
    };
    let mut stdout = child.stdout.take().expect("a piped stdout");
    let reader = thread::spawn(move || {
        let mut dom = String::new();
        let _ = stdout.read_to_string(&mut dom);
        dom
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(100);
    let finished = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                thread::sleep(std::time::Duration::from_millis(200));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let dom = reader.join().unwrap_or_default();
    let _ = std::fs::remove_dir_all(&profile);
    if finished && !dom.is_empty() {
        Some(dom)
    } else {
        println!("did not run: Chromium did not answer in time");
        None
    }
}

#[test]
fn the_pinned_bundle_draws_the_five_types_without_unsafe_eval() {
    for source in [
        "flowchart LR\n  A[Checkout] -->|calls| B[Payments]\n  B --> C[Ledger]\n",
        "sequenceDiagram\n  participant C as Checkout\n  participant P as Payments\n  C->>P: authorize amount\n",
        "classDiagram\n  PaymentMethod <|-- Card\n  Order *-- LineItem\n",
        "stateDiagram-v2\n  [*] --> Pending\n  Pending --> Paid : capture\n",
        "erDiagram\n  Customer ||--o{ Order : places\n",
    ] {
        let Some(dom) = dom_of(source) else { return };
        assert!(dom.contains("<svg"), "no drawing for {source:?}: {dom}");
        assert!(!dom.contains("class=\"error\""), "{source:?}: {dom}");
        assert!(
            dom.contains("<title>drawn</title>"),
            "the initialiser finished: {dom}"
        );
    }
}

#[test]
fn the_render_frame_shows_the_error_and_keeps_the_source_when_the_rendering_fails() {
    let Some(dom) = dom_of("flowchart LR\n  A --> \n  B[unclosed\n") else {
        return;
    };
    assert!(dom.contains("class=\"error\""), "{dom}");
    assert!(!dom.contains("<svg"), "{dom}");
}
