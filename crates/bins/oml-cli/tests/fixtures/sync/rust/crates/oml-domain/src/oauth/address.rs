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

