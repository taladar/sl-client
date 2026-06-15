#![doc = include_str!("../README.md")]

mod error;
mod session;
mod types;

pub use error::Error;
pub use session::Session;
pub use types::{DisconnectReason, Event, LoginHttpRequest, LoginParams, Reliability, Transmit};

// Re-export the wire-level types a driver needs to build messages and parse
// login responses, so it can depend on `sl-proto` alone.
pub use sl_wire::{
    AnyMessage, LoginRequest, LoginResponse, MfaChallenge, WireError, build_login_request,
    parse_login_response,
};
