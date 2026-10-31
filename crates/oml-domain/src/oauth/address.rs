//! The outbound-address allow-list of a client ID metadata document fetch
//! (ADR-0031 d5; `docs/api/http-api.md`, "Client identity"). It admits
//! rather than refuses: an AAAA answer only inside `2000::/3`, global
//! unicast, outside four blocks within it; an A answer only outside every
//! block the IANA IPv4 Special-Purpose Address Registry marks "Globally
//! Reachable: False" (fetched 2026-10-31, last updated 2025-10-09), outside
//! multicast and outside the deprecated 6to4 relay block. Every IPv6 form
//! that embeds an IPv4 address falls outside what is admitted, so it is
//! refused by construction and never unwrapped. A network-specific NAT64
//! prefix on the lead's own network lies inside `2000::/3` and cannot be
//! told apart here; the deployment's egress policy refuses it
//! (`docs/architecture/security.md`, "Connections: the OAuth surface").

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::refusal::{ClientInvalid, Refusal};

/// The A answers refused: the registry's not-globally-reachable blocks as
/// ADR-0031 d5 lists them, `224.0.0.0/4` and `192.88.99.0/24`.
const IPV4_REFUSED: [(Ipv4Addr, u32); 16] = [
    (Ipv4Addr::new(0, 0, 0, 0), 8),
    (Ipv4Addr::new(10, 0, 0, 0), 8),
    (Ipv4Addr::new(100, 64, 0, 0), 10),
    (Ipv4Addr::new(127, 0, 0, 0), 8),
    (Ipv4Addr::new(169, 254, 0, 0), 16),
    (Ipv4Addr::new(172, 16, 0, 0), 12),
    (Ipv4Addr::new(192, 0, 0, 0), 24),
    (Ipv4Addr::new(192, 0, 2, 0), 24),
    (Ipv4Addr::new(192, 88, 99, 0), 24),
    (Ipv4Addr::new(192, 168, 0, 0), 16),
    (Ipv4Addr::new(198, 18, 0, 0), 15),
    (Ipv4Addr::new(198, 51, 100, 0), 24),
    (Ipv4Addr::new(203, 0, 113, 0), 24),
    (Ipv4Addr::new(224, 0, 0, 0), 4),
    (Ipv4Addr::new(240, 0, 0, 0), 4),
    (Ipv4Addr::new(255, 255, 255, 255), 32),
];

/// The global unicast space, the only IPv6 space admitted.
const IPV6_GLOBAL_UNICAST: (Ipv6Addr, u32) = (Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 3);

/// The blocks inside `2000::/3` the IANA IPv6 Special-Purpose Address
/// Registry (fetched 2026-10-31, last updated 2025-10-09) marks not
/// globally reachable or that embed an IPv4 address: `2001::/23` (Teredo's
/// `2001::/32` within it), `2001:db8::/32`, `2002::/16` and `3fff::/20`.
const IPV6_REFUSED: [(Ipv6Addr, u32); 4] = [
    (Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23),
    (Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0), 32),
    (Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16),
    (Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0), 20),
];

/// Whether a resolved answer may be fetched from.
#[must_use]
pub fn admitted_fetch_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => !IPV4_REFUSED
            .iter()
            .any(|&(network, len)| prefix_v4(v4, len) == prefix_v4(network, len)),
        IpAddr::V6(v6) => {
            let inside =
                |(network, len): (Ipv6Addr, u32)| prefix_v6(v6, len) == prefix_v6(network, len);
            inside(IPV6_GLOBAL_UNICAST) && !IPV6_REFUSED.iter().any(|&block| inside(block))
        }
    }
}

/// Every A and AAAA answer for the document's host must be admitted, and
/// there must be one; else the document cannot be fetched within the
/// bounds, `oauth.client_invalid`. The connection is then made to a
/// checked address, with no second resolution (ADR-0031 d5).
pub fn admit_fetch_answers(answers: &[IpAddr]) -> Result<(), Refusal> {
    if !answers.is_empty() && answers.iter().all(|&a| admitted_fetch_address(a)) {
        Ok(())
    } else {
        Err(Refusal::ClientInvalid(ClientInvalid::Fetch))
    }
}

fn prefix_v4(address: Ipv4Addr, len: u32) -> u32 {
    u32::from(address) & u32::MAX.checked_shl(32 - len).unwrap_or(0)
}

fn prefix_v6(address: Ipv6Addr, len: u32) -> u128 {
    u128::from(address) & u128::MAX.checked_shl(128 - len).unwrap_or(0)
}
