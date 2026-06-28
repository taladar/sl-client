#![doc = include_str!("../README.md")]

mod error;
mod notation;
mod value;

pub use error::LlsdError;
pub use notation::Scan;
pub use value::{Llsd, parse_llsd_xml, push_escaped};
