#![doc = include_str!("../README.md")]

pub mod accounts;
mod caps_endpoint;
mod driver;
pub mod error;
mod http_service;
mod login_endpoint;
pub mod runtime;
pub mod scenario;

pub use accounts::AccountConfig;
pub use error::Error;
pub use runtime::{FakeAgent, FakeGrid, FakeGridBuilder, LoginNotice, RegionConfig};
pub use scenario::{Scenario, SimHook};
