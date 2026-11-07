//! `oml sync`: read a source at one commit, diff it against a workarea and
//! write the plan, or check it, or propose and ratify its edges (ADR-0057
//! d6–d11, d14). Every call goes through [`SyncApi`], so a run is tested
//! against a fake.

