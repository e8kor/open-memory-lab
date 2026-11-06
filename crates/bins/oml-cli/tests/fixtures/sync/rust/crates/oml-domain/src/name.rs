//! Names — the unique, immutable key a workarea, crew, key or tenant carries
//! beside its id: the grammar a name may spell, the parse-time
//! classification of a reference as an id or a name, the derived name, the
//! unresolved value an unknown name is replaced by, and the naming of rows
//! that predate names (ADR-0026 d1, d2, d3, d4, d6, d11).
//!
//! The rules are declared on `docs/api/http-api.md`, "Names and ids" and
//! "Rows created before names"; this module mirrors them and adds nothing.
//! Looking a name up needs the store and is not here (ADR-0026 d4).

