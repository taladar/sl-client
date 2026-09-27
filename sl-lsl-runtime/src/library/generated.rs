//! The library table, generated at build time by `build.rs` from the
//! vendored `keywords_lsl_default.xml` and `include!`d below.
//!
//! The content is machine-generated, so the module applies its lint
//! relaxations here, the one place inner attributes are permitted — the
//! `sl-wire` `messages` precedent.
#![allow(
    clippy::allow_attributes,
    reason = "generated library table applies blanket allows"
)]
#![allow(clippy::too_many_lines, reason = "generated from the library table")]
#![allow(
    clippy::indexing_slicing,
    reason = "`descriptor` indexes `BUILTINS` by an `index()` generated alongside it"
)]
#![allow(
    clippy::wildcard_imports,
    reason = "each signature module entry uses the parent's types"
)]
#![allow(
    clippy::module_name_repetitions,
    reason = "generated from the library table"
)]
#![allow(clippy::match_same_arms, reason = "generated from the library table")]
#![allow(
    clippy::unreadable_literal,
    reason = "floats are spelled by their exact bits"
)]
#![allow(
    clippy::decimal_literal_representation,
    reason = "integer constants keep the value the table states"
)]

include!(concat!(env!("OUT_DIR"), "/library.rs"));
