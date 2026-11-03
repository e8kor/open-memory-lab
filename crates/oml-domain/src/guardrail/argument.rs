//! Argument normalisation for the `edit`, `read` and `fetch` classes: a
//! path made absolute against the input's `cwd` and normalised lexically,
//! and the WHATWG host of a URL, lower-cased, without a trailing dot
//! (proposed ADR-0037 d8).

/// `path` made absolute against `cwd` and normalised lexically: `.` and
/// empty components dropped, `..` removing the component before it (never
/// past the root), no trailing `/`. Symlinks are not resolved: nothing here
/// reads the file system. `None` when the path is empty, or relative with
/// no absolute `cwd` to resolve it against.
#[must_use]
pub fn normalise_path(path: &str, cwd: Option<&str>) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let joined;
    let absolute = if path.starts_with('/') {
        path
    } else {
        let cwd = cwd.filter(|c| c.starts_with('/'))?;
        joined = format!("{cwd}/{path}");
        &joined
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in absolute.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    Some(format!("/{}", parts.join("/")))
}

/// The schemes the WHATWG URL Standard calls special, whose host is a
/// domain or an IP address.
const SPECIAL: [&str; 6] = ["http", "https", "ws", "wss", "ftp", "file"];

/// The host of `url` as the WHATWG URL Standard parses it, lower-cased and
/// without one trailing dot: userinfo and port removed, percent-decoded,
/// an IPv4 address in any of the standard's numeric spellings written as
/// four decimals. `None` when the URL has no host or does not parse, and
/// for a host this matcher cannot spell exactly: a non-ASCII (IDNA) domain,
/// which the check then asks on as an argument it lacks.
#[must_use]
pub fn url_host(url: &str) -> Option<String> {
    let trimmed = url.trim_matches(|c: char| c <= ' ');
    let url: String = trimmed
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let colon = url.find(':')?;
    let scheme = &url[..colon];
    let mut scheme_chars = scheme.chars();
    if !scheme_chars.next()?.is_ascii_alphabetic()
        || !scheme_chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();
    let rest = &url[colon + 1..];
    let special = SPECIAL.contains(&scheme.as_str());
    let authority_end = |c: char| matches!(c, '/' | '?' | '#') || (special && c == '\\');
    let after = if scheme == "file" {
        // A file URL's host sits between exactly two slashes, and is
        // usually empty (`file:///etc`).
        let slash = |c: char| c == '/' || c == '\\';
        rest.strip_prefix(slash)?.strip_prefix(slash)?
    } else if special {
        rest.trim_start_matches(['/', '\\'])
    } else {
        rest.strip_prefix("//")?
    };
    let authority = &after[..after.find(authority_end).unwrap_or(after.len())];
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host_port.starts_with('[') {
        &host_port[..=host_port.find(']')?]
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    if host.is_empty() {
        return None;
    }
    if host.starts_with('[') {
        return Some(host.to_ascii_lowercase());
    }
    let host = if special {
        percent_decode(host)?
    } else {
        host.to_owned()
    };
    if !host.is_ascii() || host.chars().any(forbidden_host_char) {
        return None;
    }
    let host = host.to_ascii_lowercase();
    if special && ends_in_a_number(&host) {
        return ipv4(&host);
    }
    let host = host.strip_suffix('.').unwrap_or(&host);
    (!host.is_empty()).then(|| host.to_owned())
}

fn forbidden_host_char(c: char) -> bool {
    c.is_ascii_control()
        || matches!(
            c,
            ' ' | '#' | '%' | '/' | ':' | '<' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' | '|'
        )
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let decoded = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match decoded {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// The WHATWG "ends in a number" test: the last label, after one empty
/// trailing label is dropped, is all digits or a `0x` hexadecimal number.
fn ends_in_a_number(host: &str) -> bool {
    let mut labels: Vec<&str> = host.split('.').collect();
    if labels.last() == Some(&"") && labels.len() > 1 {
        labels.pop();
    }
    let last = labels.last().copied().unwrap_or_default();
    (!last.is_empty() && last.bytes().all(|b| b.is_ascii_digit())) || ipv4_number(last).is_some()
}

fn ipv4_number(part: &str) -> Option<u64> {
    if part.is_empty() {
        return None;
    }
    let (digits, radix) = if let Some(hex) = part.strip_prefix("0x").or(part.strip_prefix("0X")) {
        (hex, 16)
    } else if part.len() > 1 && part.starts_with('0') {
        (&part[1..], 8)
    } else {
        (part, 10)
    };
    if digits.is_empty() {
        return Some(0);
    }
    if !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    // Longer than any IPv4 part can be: the address fails.
    if digits.len() > 32 {
        return None;
    }
    u64::from_str_radix(digits, radix).ok()
}

/// The WHATWG IPv4 parser, written back as four decimals.
fn ipv4(host: &str) -> Option<String> {
    let mut parts: Vec<&str> = host.split('.').collect();
    if parts.last() == Some(&"") && parts.len() > 1 {
        parts.pop();
    }
    if parts.len() > 4 {
        return None;
    }
    let numbers: Vec<u64> = parts
        .iter()
        .map(|p| ipv4_number(p))
        .collect::<Option<_>>()?;
    let (last, init) = numbers.split_last()?;
    if init.iter().any(|&n| n > 255) {
        return None;
    }
    let width = 4 - init.len() as u32;
    if *last >= 256_u64.pow(width) {
        return None;
    }
    let mut address = *last;
    for (i, &n) in init.iter().enumerate() {
        address += n << (8 * (3 - i));
    }
    let o = address.to_be_bytes();
    Some(format!("{}.{}.{}.{}", o[4], o[5], o[6], o[7]))
}
