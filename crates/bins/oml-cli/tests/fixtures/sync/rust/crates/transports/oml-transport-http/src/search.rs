//! The search settings' two routes (ADR-0049 d4): `GET /v2/search` and
//! `POST /v2/search/default`, admin by key role. A tenant's default search
//! pipeline changes here, journalled, with no restart.

