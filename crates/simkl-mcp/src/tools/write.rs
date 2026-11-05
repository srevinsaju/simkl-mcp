//! Writes (`media:write`): `set_status`, `mark_watched`, `unmark_watched`,
//! `rate`, `unrate`.
//!
//! Every write names its titles by ID; a name alone is searched first, so
//! nothing is guessed. Titles Simkl can't match come back as `not_found`.
