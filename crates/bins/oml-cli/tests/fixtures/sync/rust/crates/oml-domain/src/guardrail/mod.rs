//! Guardrails in the domain — proposed ADR-0037's server side: the key's
//! guardrail scope (d4), admission and its caps (d7, d10), the set's order,
//! conflicts and digest (d4, d5) and the held guardrail's load warning
//! (d10). The rule grammar, the matcher, the check and the rule cache live
//! in the leaf crate `oml-guardrail`, re-exported here whole (d18 Q10).

