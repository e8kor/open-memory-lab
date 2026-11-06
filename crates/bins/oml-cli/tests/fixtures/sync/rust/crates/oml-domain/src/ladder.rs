//! The workarea status ladder as pure functions (ADR-0009; workareas.md, the
//! status ladder): the three statuses (d1), the forward-only step that a
//! proposal and its ratification both test (d3), the child-status constraint
//! (d4), the second identity a ratification needs (R2 as errors.md words
//! `review.ratifier_is_proposer`), and the closed list of mutations a frozen
//! area admits (d5).
//!
//! The functions decide; the caller supplies the node as it is at that
//! moment. Ratification's caller reads the node and its children under the
//! tenant's chain lock in the transaction that writes the status, so nothing
//! arrives between the test and the write (workareas.md, readings the build
//! takes).

