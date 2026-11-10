//! Argument normalisation for the `edit`, `read` and `fetch` classes: a
//! path made absolute against the input's `cwd` and normalised lexically,
//! and the WHATWG host of a URL, serialised as the standard serialises it,
//! lower-cased, without a trailing dot (ADR-0037 d8).

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
/// four decimals, an IPv6 address in the standard's compressed form. `None` when the URL has no host or does not parse, and
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
    if let Some(bracketed) = host_port.strip_prefix('[') {
        let (address, port) = bracketed.split_once(']')?;
        if !(port.is_empty() || port.starts_with(':')) {
            return None;
        }
        return ipv6(address).map(|a| format!("[{a}]"));
    }
    let host = host_port.split(':').next().unwrap_or_default();
    if host.is_empty() {
        return None;
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

/// The WHATWG IPv6 parser over the text between a host's brackets, written
/// back by the WHATWG IPv6 serializer: lower-case hexadecimal pieces without
/// leading zeros, the first longest run of two or more zero pieces written
/// `::`, an embedded IPv4 address written as two pieces. `None` when the
/// standard's parser fails.
pub(crate) fn ipv6(input: &str) -> Option<String> {
    let c: Vec<char> = input.chars().collect();
    let at = |i: usize| c.get(i).copied();
    let mut address = [0_u16; 8];
    let mut piece = 0;
    let mut compress: Option<usize> = None;
    let mut p = 0;
    if at(0) == Some(':') {
        if at(1) != Some(':') {
            return None;
        }
        p = 2;
        piece = 1;
        compress = Some(1);
    }
    while let Some(ch) = at(p) {
        if piece == 8 {
            return None;
        }
        if ch == ':' {
            if compress.is_some() {
                return None;
            }
            p += 1;
            piece += 1;
            compress = Some(piece);
            continue;
        }
        let mut value: u16 = 0;
        let mut length = 0;
        while length < 4 {
            let Some(d) = at(p).and_then(|h| h.to_digit(16)) else {
                break;
            };
            value = value * 16 + d as u16;
            p += 1;
            length += 1;
        }
        match at(p) {
            Some('.') => {
                if length == 0 || piece > 6 {
                    return None;
                }
                p -= length;
                let mut seen = 0;
                while at(p).is_some() {
                    if seen > 0 {
                        if at(p) == Some('.') && seen < 4 {
                            p += 1;
                        } else {
                            return None;
                        }
                    }
                    let mut number: Option<u16> = None;
                    while let Some(d) = at(p).and_then(|d| d.to_digit(10)) {
                        number = match number {
                            None => Some(d as u16),
                            Some(0) => return None,
                            Some(n) => Some(n * 10 + d as u16),
                        };
                        if number > Some(255) {
                            return None;
                        }
                        p += 1;
                    }
                    address[piece] = address[piece] * 0x100 + number?;
                    seen += 1;
                    if seen == 2 || seen == 4 {
                        piece += 1;
                    }
                }
                if seen != 4 {
                    return None;
                }
                break;
            }
            Some(':') => {
                p += 1;
                at(p)?;
            }
            Some(_) => return None,
            None => {}
        }
        address[piece] = value;
        piece += 1;
    }
    if let Some(compress) = compress {
        let mut swaps = piece - compress;
        piece = 7;
        while piece != 0 && swaps > 0 {
            address.swap(piece, compress + swaps - 1);
            piece -= 1;
            swaps -= 1;
        }
    } else if piece != 8 {
        return None;
    }
    // The first longest run of two or more zero pieces.
    let mut run: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < 8 {
        let start = i;
        while i < 8 && address[i] == 0 {
            i += 1;
        }
        let length = i - start;
        if length > 1 && run.is_none_or(|(_, longest)| length > longest) {
            run = Some((start, length));
        }
        i = i.max(start + 1);
    }
    let mut out = String::new();
    let mut i = 0;
    while i < 8 {
        if let Some((start, length)) = run
            && i == start
        {
            out.push_str(if i == 0 { "::" } else { ":" });
            i += length;
            continue;
        }
        out.push_str(&format!("{:x}", address[i]));
        if i != 7 {
            out.push(':');
        }
        i += 1;
    }
    Some(out)
}

/// The WHATWG "ends in a number" test: the last label, after one empty
/// trailing label is dropped, is all digits or a `0x` hexadecimal number.
pub(crate) fn ends_in_a_number(host: &str) -> bool {
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
pub(crate) fn ipv4(host: &str) -> Option<String> {
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
