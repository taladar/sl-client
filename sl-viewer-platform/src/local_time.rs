//! The system's **local time zone**, resolved once and shared.
//!
//! Resolving a zone reads the `TZ` environment variable, and reading the
//! environment is only sound while the process is still single-threaded —
//! before Bevy's task pools spawn. So the viewer captures it in `main` and
//! inserts it as a resource; every surface that renders a stored timestamp in
//! local time (snapshot filenames, the derender blacklist's Date column, the
//! avatar render-settings list) then reads the cached zone and the thread-safe
//! monotonic clock, and never the environment again.

use bevy::prelude::Resource;

/// The system's local time zone, resolved **once at startup** and reused to
/// stamp anything the user reads as a local date or time.
///
/// Captured with [`LocalTimeZone::capture`] before Bevy's task pools exist —
/// see the module docs for why that timing is not incidental.
#[derive(Resource, Clone)]
pub struct LocalTimeZone(jiff::tz::TimeZone);

impl LocalTimeZone {
    /// Resolve the system time zone now. Call this **early**, while the process
    /// is still single-threaded (see the module docs).
    #[must_use]
    pub fn capture() -> Self {
        Self(jiff::tz::TimeZone::system())
    }

    /// The captured zone, for a surface rendering a stored timestamp in local
    /// time.
    #[must_use]
    pub const fn zone(&self) -> &jiff::tz::TimeZone {
        &self.0
    }
}

impl std::fmt::Debug for LocalTimeZone {
    /// Print the zone by its IANA name, which is the only part of a
    /// `jiff::tz::TimeZone` worth reading in a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LocalTimeZone")
            .field(&self.0.iana_name().unwrap_or("<unnamed>"))
            .finish()
    }
}
