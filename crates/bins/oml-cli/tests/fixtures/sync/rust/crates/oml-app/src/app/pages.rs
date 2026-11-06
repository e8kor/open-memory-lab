//! The acts of the web pages through the person's session keys (ADR-0045
//! d6, d9–d13, d16, d21): the keys a first sign-in issues, the caller each
//! capacity acts as, the user page's own-key acts, the admin page's
//! capacities and queue, and the admin read of every memory's text and
//! attribution, journalled.
//!
//! A session key has no value: the transport never holds one. It asks for
//! the [`Caller`] of a capacity from a live session, re-checked on every
//! request, and acts through [`Bearer::Session`]; every act lands in the
//! journal under the session key, as a CLI act lands under its key.

