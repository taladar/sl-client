//! Manual, live-grid conformance test harness for the `sl-client` workspace.
//!
//! These tests are *not* part of `cargo test`: each one logs in to a real grid
//! (the local OpenSim or Second Life Beta "aditi") and exercises one library
//! feature, recording a git-stamped result into the committed `records/` tree.
//!
//! The library half of the crate is split into independently testable pieces:
//!
//! - [`grid`] — the [`Grid`] a test can target.
//! - [`gitinfo`] — the behaviour-aware git-describe `-dirty` computation.
//! - [`record`] — the on-disk per-`(test, grid)` record with its bounded run
//!   history ([`Record`]).
//! - [`metrics`] — the [`Metrics`] collector a test writes to.
//! - [`report`] — pure status classification and performance-delta computation
//!   used by the `sl-conformance-report` binary.
//! - [`registry`](mod@registry) — the [`GridTest`] trait and the curated test
//!   registry ([`registry()`]).
//! - [`context`] — the login + session-drive [`TestContext`](context::TestContext)
//!   handed to each test, and the per-avatar aditi cooldown guard.
//! - [`support`] — shared scaffolding (timeouts, combinators, assertion and
//!   metric-name helpers, well-known id fixtures) the cases build on.
//! - [`cases`] — the concrete test implementations.

pub mod cases;
pub mod context;
pub mod fixtures;
pub mod gitinfo;
pub mod grid;
pub mod metrics;
pub mod record;
pub mod registry;
pub mod report;
pub mod support;

pub use grid::Grid;
pub use metrics::Metrics;
pub use record::{Completeness, MetricMeta, MetricValue, Outcome, Record, Run};
pub use registry::{GridTest, find, registry};
