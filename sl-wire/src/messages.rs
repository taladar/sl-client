//! Generated LLUDP message types and their (de)serialization.
//!
//! The body of this module is produced at build time by `build.rs` from the
//! vendored `message_template.msg` and `include!`d below. Because the content
//! is machine-generated, the module applies blanket lint relaxations here (the
//! one place inner attributes are permitted) rather than satisfying every
//! pedantic crate lint in the generated output.
#![allow(
    clippy::allow_attributes,
    reason = "generated message code applies blanket allows"
)]
#![allow(
    clippy::struct_field_names,
    reason = "generated from the message template"
)]
#![allow(
    clippy::module_name_repetitions,
    reason = "generated from the message template"
)]
#![allow(clippy::too_many_lines, reason = "generated from the message template")]
#![allow(clippy::unreadable_literal, reason = "generated message ids")]
#![allow(
    clippy::missing_errors_doc,
    reason = "generated from the message template"
)]
#![allow(
    clippy::must_use_candidate,
    reason = "generated from the message template"
)]
#![allow(clippy::empty_structs_with_brackets, reason = "generated empty blocks")]
#![allow(
    clippy::derive_partial_eq_without_eq,
    reason = "generated from the message template"
)]
#![allow(clippy::match_same_arms, reason = "generated dispatch")]
#![allow(
    clippy::semicolon_if_nothing_returned,
    reason = "generated from the message template"
)]
#![allow(
    clippy::used_underscore_binding,
    reason = "generated from the message template"
)]
#![allow(
    clippy::unused_trait_names,
    reason = "trait is used for method resolution"
)]
#![allow(
    clippy::wildcard_imports,
    reason = "generated from the message template"
)]
#![allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "generated from the message template"
)]
#![allow(
    clippy::missing_const_for_fn,
    reason = "generated from the message template"
)]
#![allow(clippy::ref_option, reason = "generated from the message template")]
#![allow(
    clippy::large_enum_variant,
    reason = "generated AnyMessage variants vary in size"
)]
#![allow(
    clippy::enum_variant_names,
    reason = "generated from the message template"
)]
#![allow(
    clippy::struct_excessive_bools,
    reason = "generated from the message template"
)]
#![allow(
    clippy::fn_params_excessive_bools,
    reason = "generated from the message template"
)]
#![allow(
    unused_variables,
    reason = "messages without blocks do not touch the cursor"
)]
#![allow(missing_docs, reason = "generated from the message template")]
#![allow(
    missing_debug_implementations,
    reason = "generated from the message template"
)]
#![allow(dead_code, reason = "not every generated message is used yet")]

include!(concat!(env!("OUT_DIR"), "/messages.rs"));
