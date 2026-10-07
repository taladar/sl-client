//! Conformance test harness for the `sl-client` workspace: one case per
//! feature, each logging in and exercising it against a grid.
//!
//! Most of these tests are *not* part of `cargo test`: they log in to a real
//! grid (the local OpenSim or Second Life Beta "aditi") and record a
//! git-stamped result into the `records/` tree. The exception is the fake grid
//! — an [`sl_fake_grid`] started inside the test process, as
//! [`Grid::FakeSl`] or [`Grid::FakeOpensim`] — which the cases listed in
//! [`fake::OFFLINE_CASES`] run against, once per flavour, on every `cargo
//! test`, with no network, no credentials and no record.
//!
//! The library half of the crate is split into independently testable pieces:
//!
//! - [`grid`] — the [`Grid`] a test can target.
//! - [`gitinfo`] — the behaviour-aware git-describe `-dirty` computation.
//! - [`record`] — the on-disk per-`(test, grid)` record with its bounded run
//!   history ([`Record`]).
//! - [`metrics`] — the [`Metrics`] collector a test writes to.
//! - [`measured`] — what each live grid was measured to answer
//!   ([`Measured`](measured::Measured)), and the check holding every grid,
//!   fake flavours included, to it.
//! - [`report`] — pure status classification and performance-delta computation
//!   used by the `sl-conformance-report` binary.
//! - [`registry`](mod@registry) — the [`GridTest`] trait and the curated test
//!   registry ([`registry()`]).
//! - [`context`] — the login + session-drive [`TestContext`](context::TestContext)
//!   handed to each test, and the per-avatar aditi cooldown guard.
//! - [`support`] — shared scaffolding (timeouts, combinators, assertion and
//!   metric-name helpers, well-known id fixtures) the cases build on.
//! - [`isolate`] — runs a case body under a caught unwind and an overall
//!   timeout, so a panicking or hung case fails only itself.
//! - [`cases`] — the concrete test implementations.

pub mod cases;
pub mod circuit;
pub mod context;
pub mod fake;
pub mod fixtures;
pub mod gitinfo;
pub mod grid;
pub mod isolate;
pub mod lure;
pub mod measured;
pub mod metrics;
pub mod record;
pub mod registry;
pub mod report;
pub mod support;
pub mod teleport_trace;
pub mod trace;

pub use grid::Grid;
pub use metrics::Metrics;
pub use record::{Completeness, MetricMeta, MetricValue, Outcome, Record, Run};
pub use registry::{GridTest, find, registry};
