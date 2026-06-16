#![doc = include_str!("../README.md")]

mod error;
mod session;
mod types;

pub use error::Error;
pub use session::Session;
pub use types::{
    DisconnectReason, Event, LoginHttpRequest, LoginParams, MapRegionInfo, Maturity, NeighborInfo,
    ParcelInfo, ParcelOverlayInfo, ProductType, RegionIdentity, RegionLimits, Reliability,
    Transmit, grid_to_handle, handle_to_global, handle_to_grid,
};

// Re-export the wire-level types a driver needs to build messages and parse
// login responses, so it can depend on `sl-proto` alone.
pub use sl_wire::{
    AnyMessage, EventQueueEvent, EventQueueResponse, Llsd, LoginRequest, LoginResponse,
    MfaChallenge, ParcelFlags, RegionFlags, WireError, build_event_queue_request,
    build_login_request, build_seed_request, parse_event_queue_response, parse_llsd_xml,
    parse_login_response, parse_seed_response, sim_access,
};
// Re-export the vector type used by the teleport API.
pub use sl_types::lsl::Vector;
