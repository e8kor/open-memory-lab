//! Doctor's `oauth_discovery` check (observability.md, the doctor contract;
//! ADR-0031 d14): the issuer the lead names on `/readyz`, a route that does
//! not depend on the discovery documents, then both discovery documents
//! fetched from that issuer's own origin, wherever the CLI runs, so a
//! reverse proxy that does not pass `/.well-known/oauth-*` through, or a
//! node serving another issuer, shows as `failing` with the URL that
//! failed, and a lead naming no issuer is `off` whatever the route answers.

