#![doc = include_str!("../README.md")]

mod binary;
mod error;
mod notation;
mod value;

pub use binary::parse_llsd_binary;
pub use error::LlsdError;
pub use notation::Scan;
pub use value::{Llsd, parse_llsd_xml, push_escaped};
