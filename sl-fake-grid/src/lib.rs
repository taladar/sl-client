#![doc = include_str!("../README.md")]

pub mod accounts;
mod caps_endpoint;
mod driver;
mod economy_endpoint;
pub mod economy_policy;
pub mod error;
mod http_answer;
mod http_service;
mod login_endpoint;
mod map_tiles;
pub mod runtime;
pub mod scenario;

pub use accounts::AccountConfig;
pub use economy_policy::{EconomyConfig, EconomyEvent};
pub use error::Error;
pub use map_tiles::STOCK_TILE_JPEG;
pub use runtime::{FakeAgent, FakeGrid, FakeGridBuilder, GridIdentity, LoginNotice, RegionConfig};
pub use scenario::{Scenario, SimHook};
