//! The host verbs' two halves the lead composes (ADR-0097 d19, d20; the
//! operator's answers 13 and 14 of 2026-11-27): the operator token each
//! start writes per tenant, and the SMTP client that hands the recovery and
//! settlement emails to the one local relay the host configures.
//!
//! The token: at each start, one per tenant, 32 random bytes in hex behind
//! `omlop_`, written to `<operator.token_dir>/<tenant id>.token`, mode 0600
//! in a 0700 directory, so root and the server's own user read it and no
//! one else. The lead keeps only its digest, so a restart replaces every
//! token and a token outlives no process. It is never printed, logged or
//! journalled.
//!
//! The mail: plain SMTP (RFC 5321) to `mail.host`:`mail.port`, from
//! `mail.from`, with no TLS and no authentication: the relay handles both,
//! so the lead holds no mail credential. A relay that refuses or cannot be
//! reached is named, and the act the email tells about stands.

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::TcpStream;
use uuid::Uuid;

use oml_app::domain::web::Email;
use oml_app::ports::Mailer;
use oml_app::types::OutboundMail;
use oml_transport_http::operator::OperatorTokens;

/// The `event` field of the log line a start that could not write its
/// operator tokens writes; `/readyz` then reports `operator_tokens:
/// failed`, which doctor names `operator.token_unwritten`.
pub const EVENT_OPERATOR_TOKENS_UNWRITTEN: &str = "operator_tokens_unwritten";

/// What the start composes for the host verbs: where the operator tokens
/// go, and the mail relay. The default writes no token and sends no mail,
/// as a test's lead does.
#[derive(Clone, Default)]
pub struct HostVerbs {
    pub token_dir: Option<PathBuf>,
    pub mailer: Option<Arc<dyn Mailer>>,
}

/// The file `tenant`'s operator token is written to under `dir`.
#[must_use]
pub fn token_path(dir: &Path, tenant: Uuid) -> PathBuf {
    dir.join(format!("{tenant}.token"))
}

/// Writes one fresh operator token per tenant under `dir` and answers the
/// tokens the lead admits. The directory is created and set to 0700, each
/// file written 0600 beside its final name and renamed over the token of
/// the previous start, so a reader never sees half a token.
///
/// # Errors
///
/// The directory or a file that cannot be written, named by path and the
/// I/O error's kind; no token value is in the reason.
pub fn write_operator_tokens(dir: &Path, tenants: &[Uuid]) -> Result<OperatorTokens, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e.kind()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("{}: {}", dir.display(), e.kind()))?;
    }
    let mut tokens = Vec::with_capacity(tenants.len());
    for tenant in tenants {
        let mut raw = [0_u8; 32];
        getrandom::getrandom(&mut raw).map_err(|e| format!("random token: {e}"))?;
        let token = format!(
            "omlop_{}",
            raw.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        let path = token_path(dir, *tenant);
        let staged = dir.join(format!("{tenant}.token.new"));
        let _ = std::fs::remove_file(&staged);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        {
            use std::io::Write as _;
            let mut file = options
                .open(&staged)
                .map_err(|e| format!("{}: {}", staged.display(), e.kind()))?;
            file.write_all(format!("{token}\n").as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|e| format!("{}: {}", staged.display(), e.kind()))?;
        }
        std::fs::rename(&staged, &path).map_err(|e| format!("{}: {}", path.display(), e.kind()))?;
        tokens.push((*tenant, token));
    }
    Ok(OperatorTokens::new(tokens))
}

/// The one local mail relay (`mail.host`, `mail.port`, `mail.from`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpRelay {
    pub host: String,
    pub port: u16,
    pub from: Email,
}

/// The longest reply line the client reads: RFC 5321 4.5.3.1.5's 512
/// octets, with room for a relay that sends more.
const REPLY_LINE_MAX: usize = 2048;
/// The most lines one multi-line reply may hold.
const REPLY_LINES_MAX: usize = 64;

impl Mailer for SmtpRelay {
    fn send<'a>(
        &'a self,
        mail: &'a OutboundMail,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(self.deliver(mail))
    }
}

impl SmtpRelay {
    /// One SMTP transaction: greeting, `EHLO`, `MAIL FROM`, a `RCPT TO` per
    /// address, `DATA`, the message, `QUIT`. Every refusal names the
    /// command and the relay's code, never an address.
    async fn deliver(&self, mail: &OutboundMail) -> Result<(), String> {
        let stream = TcpStream::connect((self.host.as_str(), self.port))
            .await
            .map_err(|e| {
                format!(
                    "the relay {}:{} cannot be reached: {}",
                    self.host,
                    self.port,
                    e.kind()
                )
            })?;
        let (read, mut write) = stream.into_split();
        let mut read = BufReader::new(read);
        expect(&mut read, "the greeting", &[220]).await?;
        command(&mut write, "EHLO localhost").await?;
        expect(&mut read, "EHLO", &[250]).await?;
        command(&mut write, &format!("MAIL FROM:<{}>", self.from.as_str())).await?;
        expect(&mut read, "MAIL FROM", &[250]).await?;
        for to in &mail.to {
            command(&mut write, &format!("RCPT TO:<{}>", to.as_str())).await?;
            expect(&mut read, "RCPT TO", &[250, 251]).await?;
        }
        command(&mut write, "DATA").await?;
        expect(&mut read, "DATA", &[354]).await?;
        write
            .write_all(message(&self.from, mail, &rfc5322_now()).as_bytes())
            .await
            .map_err(|e| format!("the relay closed during DATA: {}", e.kind()))?;
        expect(&mut read, "the message", &[250]).await?;
        // The message is taken; a QUIT that goes unanswered changes nothing.
        let _ = command(&mut write, "QUIT").await;
        Ok(())
    }
}

async fn command(write: &mut tokio::net::tcp::OwnedWriteHalf, line: &str) -> Result<(), String> {
    write
        .write_all(format!("{line}\r\n").as_bytes())
        .await
        .map_err(|e| format!("the relay closed the connection: {}", e.kind()))
}

/// Reads one reply, multi-line or not, and checks its code.
async fn expect(
    read: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    stage: &str,
    codes: &[u16],
) -> Result<(), String> {
    for _ in 0..REPLY_LINES_MAX {
        let mut line = String::new();
        let n = (&mut *read)
            .take(REPLY_LINE_MAX as u64)
            .read_line(&mut line)
            .await
            .map_err(|e| format!("the relay's reply to {stage} did not arrive: {}", e.kind()))?;
        if n == 0 {
            return Err(format!(
                "the relay closed the connection before answering {stage}"
            ));
        }
        let code = line
            .get(..3)
            .and_then(|c| c.parse::<u16>().ok())
            .ok_or_else(|| format!("the relay's reply to {stage} is not SMTP"))?;
        if line.as_bytes().get(3) == Some(&b'-') {
            continue;
        }
        return if codes.contains(&code) {
            Ok(())
        } else {
            Err(format!("the relay refused {stage} with {code}"))
        };
    }
    Err(format!(
        "the relay's reply to {stage} ran past {REPLY_LINES_MAX} lines"
    ))
}

/// The message as `DATA` carries it: headers, a blank line, the body with
/// CRLF line ends and dot-stuffing (RFC 5321 4.5.2), and the final dot.
/// Header values are the server's own text and stored addresses; any CR or
/// LF in them is dropped, so no value starts a header of its own.
fn message(from: &Email, mail: &OutboundMail, date: &str) -> String {
    let clean = |s: &str| {
        s.chars()
            .filter(|c| *c != '\r' && *c != '\n')
            .collect::<String>()
    };
    let to = mail
        .to
        .iter()
        .map(|e| clean(e.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = format!(
        "Date: {date}\r\nFrom: {}\r\nTo: {to}\r\nSubject: {}\r\nMIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
        clean(from.as_str()),
        clean(&mail.subject),
    );
    for line in mail.body.lines() {
        if line.starts_with('.') {
            out.push('.');
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str(".\r\n");
    out
}

/// Now as RFC 5322 3.3 writes a date, in UTC.
fn rfc5322_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc5322(i64::try_from(secs).unwrap_or(0))
}

/// `secs` since the epoch as an RFC 5322 date in UTC, from the civil date
/// of the day number (Howard Hinnant's `civil_from_days`).
fn rfc5322(secs: i64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{}, {day:02} {} {year} {:02}:{:02}:{:02} +0000",
        DAYS[usize::try_from(days.rem_euclid(7)).unwrap_or(0)],
        MONTHS[usize::try_from(month - 1).unwrap_or(0)],
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    fn email(s: &str) -> Email {
        Email::parse(s).expect("an email")
    }

    fn notice() -> OutboundMail {
        OutboundMail {
            to: vec![email("alice@example.com"), email("bob@example.com")],
            subject: "OML: a settlement".to_owned(),
            body: "line one\n.a dot line\nline three\n".to_owned(),
        }
    }

    /// A fake relay on an ephemeral loopback port: it answers each command
    /// with the next of `replies` and keeps what the client sent.
    async fn fake_relay(replies: Vec<&'static str>) -> (u16, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut seen = String::new();
            let mut replies = replies.into_iter();
            socket
                .write_all(replies.next().expect("a greeting").as_bytes())
                .await
                .expect("greet");
            let mut buf = [0_u8; 4096];
            let mut in_data = false;
            while let Some(reply) = replies.next() {
                // Read until a whole command, or the message's final dot.
                loop {
                    let n = socket.read(&mut buf).await.expect("read");
                    if n == 0 {
                        return seen;
                    }
                    seen.push_str(&String::from_utf8_lossy(&buf[..n]));
                    let done = if in_data {
                        seen.ends_with("\r\n.\r\n")
                    } else {
                        seen.ends_with("\r\n")
                    };
                    if done {
                        break;
                    }
                }
                in_data = reply.starts_with("354");
                socket.write_all(reply.as_bytes()).await.expect("reply");
            }
            seen
        });
        (port, task)
    }

    fn relay(port: u16) -> SmtpRelay {
        SmtpRelay {
            host: "127.0.0.1".to_owned(),
            port,
            from: email("oml@example.org"),
        }
    }

    #[tokio::test]
    async fn a_notice_email_is_one_smtp_transaction_to_the_local_relay() {
        let (port, relay_task) = fake_relay(vec![
            "220 relay ready\r\n",
            "250-relay\r\n250 8BITMIME\r\n",
            "250 sender ok\r\n",
            "250 rcpt ok\r\n",
            "251 rcpt forwarded\r\n",
            "354 go ahead\r\n",
            "250 queued\r\n",
            "221 bye\r\n",
        ])
        .await;
        relay(port).deliver(&notice()).await.expect("delivered");
        let seen = relay_task.await.expect("the relay");
        assert!(
            seen.starts_with("EHLO localhost\r\nMAIL FROM:<oml@example.org>\r\n"),
            "{seen}"
        );
        assert!(
            seen.contains("RCPT TO:<alice@example.com>\r\nRCPT TO:<bob@example.com>\r\nDATA\r\n")
        );
        assert!(seen.contains("To: alice@example.com, bob@example.com\r\n"));
        assert!(seen.contains("Subject: OML: a settlement\r\n"));
        assert!(
            seen.contains("\r\n\r\nline one\r\n..a dot line\r\nline three\r\n.\r\n"),
            "the body is dot-stuffed: {seen}"
        );
    }

    #[tokio::test]
    async fn a_relay_that_refuses_a_recipient_is_named_without_the_address() {
        let (port, _relay) = fake_relay(vec![
            "220 relay ready\r\n",
            "250 relay\r\n",
            "250 sender ok\r\n",
            "550 no such user\r\n",
        ])
        .await;
        let err = relay(port).deliver(&notice()).await.unwrap_err();
        assert_eq!(err, "the relay refused RCPT TO with 550");
        assert!(!err.contains("example.com"));
    }

    #[tokio::test]
    async fn a_relay_that_cannot_be_reached_is_named() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            listener.local_addr().expect("addr").port()
        };
        let err = relay(port).deliver(&notice()).await.unwrap_err();
        assert!(
            err.starts_with(&format!("the relay 127.0.0.1:{port} cannot be reached")),
            "{err}"
        );
    }

    #[test]
    fn a_date_is_written_as_rfc_5322_reads_it() {
        assert_eq!(rfc5322(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        // 2026-11-27 10:15:30 UTC.
        assert_eq!(rfc5322(1_795_774_530), "Fri, 27 Nov 2026 10:15:30 +0000");
    }

    #[test]
    fn a_header_value_starts_no_header_of_its_own() {
        let mail = OutboundMail {
            to: vec![email("alice@example.com")],
            subject: "a\r\nBcc: eve@example.com".to_owned(),
            body: String::new(),
        };
        let text = message(&email("oml@example.org"), &mail, "d");
        assert!(text.contains("Subject: aBcc: eve@example.com\r\n"));
        assert!(!text.contains("\r\nBcc:"));
    }

    #[test]
    fn each_start_writes_a_fresh_owner_only_token_per_tenant_and_admits_only_it() {
        let dir = std::env::temp_dir().join(format!("oml-operator-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        let first = write_operator_tokens(&dir, &[a, b]).expect("written");
        let token_a = std::fs::read_to_string(token_path(&dir, a)).expect("a's token");
        let token_a = token_a.trim();
        assert!(token_a.starts_with("omlop_") && token_a.len() == 70);
        assert!(first.admits(a, token_a));
        assert!(!first.admits(b, token_a), "a token is one tenant's");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(token_path(&dir, a))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
            let mode = std::fs::metadata(&dir).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let second = write_operator_tokens(&dir, &[a]).expect("written again");
        assert!(!second.admits(a, token_a), "a restart replaces the token");
        let again = std::fs::read_to_string(token_path(&dir, a)).expect("a's token");
        assert!(second.admits(a, again.trim()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
