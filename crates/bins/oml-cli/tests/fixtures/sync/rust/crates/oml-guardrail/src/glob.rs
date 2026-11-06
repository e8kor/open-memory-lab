//! The `oml-gr/1` glob: `*` matches any run and `?` one character; in a
//! path glob `*` and `?` stop at `/` and `**` does not, and each matches a
//! leading `.`; every other character is literal (proposed ADR-0037 d8).
//!
//! The matcher is a simulation of the glob's automaton over the set of live
//! pattern positions, one input character at a time: O(pattern × input)
//! time and O(pattern) space, with no regular expression and no
//! backtracking, so no pattern a rule can spell runs away.

