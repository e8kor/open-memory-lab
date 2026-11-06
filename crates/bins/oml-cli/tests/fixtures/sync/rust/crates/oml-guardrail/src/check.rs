//! The check before a tool call: the cached rules against one call, with
//! no request, answering deny, ask or nothing, never allow; and the cache's
//! state deciding first, so a cache that cannot be checked denies and no
//! cache at all warns (proposed ADR-0037 d8, d9).

