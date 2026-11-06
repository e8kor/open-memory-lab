//! Search as a pipeline of retrievers, fused in the core (ADR-0049).
//!
//! A recall's hit candidates come from one or more [`Retriever`] adapters,
//! each producing a ranked list over the candidate scope the core computes;
//! a pure [`Fusion`] strategy here merges the lists; the core then applies
//! the `k` and budget cuts it always applied. The adapters rank and never
//! decide visibility: the scope is the core's, and the core re-checks every
//! served row (ADR-0001 d3, R3; ADR-0049 d2).
//!
//! A deployment names the pipelines it serves and its default
//! ([`SearchConfig`]); a tenant may pick its default among them, journalled;
//! a request may name one among them. The names form a closed set, so a
//! metric labelled by pipeline stays bounded (ADR-0049 d4).
//!
//! [`Retriever`]: crate::ports::Retriever

