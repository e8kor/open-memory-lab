//! The client address behind a proxy: `X-Forwarded-For` is read only when
//! the socket peer is in `server.trusted_proxies`, right to left, skipping
//! every listed address (ADR-0042 d7).

use std::net::IpAddr;

use axum::http::HeaderMap;

/// One entry of `server.trusted_proxies`: an address, or a CIDR range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Net {
    addr: IpAddr,
    prefix: u8,
}

impl Net {
    fn parse(entry: &str) -> Result<Self, String> {
        let refused = || {
            format!(
                "server.trusted_proxies entry {entry:?} is not an address or a CIDR range (ADR-0042 d7)"
            )
        };
        let (addr, prefix) = match entry.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (entry, None),
        };
        let addr: IpAddr = addr.trim().parse().map_err(|_| refused())?;
        let addr = addr.to_canonical();
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => max,
            Some(p) => p
                .trim()
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= max)
                .ok_or_else(refused)?,
        };
        Ok(Self { addr, prefix })
    }

    fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip.to_canonical()) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

/// Where the logged client address came from, `client_ip_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientIpSource {
    /// The socket peer.
    Peer,
    /// `X-Forwarded-For`, through a trusted peer.
    Forwarded,
}

impl ClientIpSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Peer => "peer",
            Self::Forwarded => "forwarded",
        }
    }
}

/// `server.trusted_proxies`, empty by default: no peer is trusted, so a
/// client that sets `X-Forwarded-For` itself gains nothing (ADR-0042 d7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<Net>);

impl TrustedProxies {
    /// Whether no proxy is trusted, so every client address is the
    /// socket peer's (ADR-0068 d9's doctor line).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The list from its entries; one that does not parse is refused, and
    /// the lead stops at start (ADR-0042 d7).
    pub fn parse<S: AsRef<str>>(entries: &[S]) -> Result<Self, String> {
        entries
            .iter()
            .map(|e| Net::parse(e.as_ref()))
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    fn trusts(&self, ip: IpAddr) -> bool {
        self.0.iter().any(|net| net.contains(ip))
    }

    /// The client address the request line logs: the peer, or, when the
    /// peer is trusted, the first address from the right of
    /// `X-Forwarded-For` outside the list. A header that does not parse
    /// leaves the peer. `None` when the serve call recorded no peer.
    #[must_use]
    pub fn client_ip(
        &self,
        peer: Option<IpAddr>,
        headers: &HeaderMap,
    ) -> Option<(IpAddr, ClientIpSource)> {
        let peer = peer?.to_canonical();
        let by_peer = Some((peer, ClientIpSource::Peer));
        if !self.trusts(peer) {
            return by_peer;
        }
        let mut hops = Vec::new();
        for value in headers.get_all("x-forwarded-for") {
            let Ok(value) = value.to_str() else {
                return by_peer;
            };
            for hop in value.split(',') {
                let Ok(ip) = hop.trim().parse::<IpAddr>() else {
                    return by_peer;
                };
                hops.push(ip.to_canonical());
            }
        }
        hops.into_iter()
            .rev()
            .find(|ip| !self.trusts(*ip))
            .map_or(by_peer, |ip| Some((ip, ClientIpSource::Forwarded)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn xff(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_str(value).expect("ASCII"),
        );
        headers
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("an address")
    }

    #[test]
    fn a_header_from_an_untrusted_peer_leaves_client_ip_the_peer() {
        let none = TrustedProxies::default();
        assert_eq!(
            none.client_ip(Some(ip("203.0.113.9")), &xff("198.51.100.1")),
            Some((ip("203.0.113.9"), ClientIpSource::Peer))
        );
        let loopback = TrustedProxies::parse(&["127.0.0.1/32", "::1/128"]).expect("parses");
        assert_eq!(
            loopback.client_ip(Some(ip("10.0.0.5")), &xff("198.51.100.1")),
            Some((ip("10.0.0.5"), ClientIpSource::Peer))
        );
    }

    #[test]
    fn through_a_trusted_peer_the_first_untrusted_address_from_the_right_is_the_client() {
        let trusted = TrustedProxies::parse(&["127.0.0.1/32", "10.0.0.0/8"]).expect("parses");
        assert_eq!(
            trusted.client_ip(
                Some(ip("127.0.0.1")),
                &xff("192.0.2.66, 198.51.100.7, 10.1.2.3")
            ),
            Some((ip("198.51.100.7"), ClientIpSource::Forwarded))
        );
        assert_eq!(
            trusted.client_ip(Some(ip("::ffff:127.0.0.1")), &xff("garbage, 10.1.2.3")),
            Some((ip("127.0.0.1"), ClientIpSource::Peer)),
            "a header that does not parse leaves the peer"
        );
    }

    #[test]
    fn an_entry_that_does_not_parse_is_refused() {
        for bad in ["localhost", "10.0.0.0/33", "::1/129", "1.2.3"] {
            assert!(TrustedProxies::parse(&[bad]).is_err(), "{bad}");
        }
    }
}
