//! The admin read's two lab routes (ADR-0045 d21): `POST
//! /v2/admin/memories/search` and `GET /v2/admin/memories/{artifact_id}`,
//! admin key role alone, every answer journalled as `memory.admin_read`.
//! The search is a `POST` so its query travels in a body.

