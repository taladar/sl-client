//! Display-name change pushes (`DisplayNameUpdate`, `SetDisplayNameReply`).
//!
//! Second Life's mutable, user-chosen *display names* are resolved in bulk over
//! the `GetDisplayNames` capability (see [`DisplayName`]), but the simulator
//! also *pushes* two display-name events over the CAPS event queue:
//!
//! - `DisplayNameUpdate` — a cached display name changed (for this agent or
//!   another), carrying the previous display name and the full new record.
//! - `SetDisplayNameReply` — the result of *this* agent's own
//!   set-display-name request.
//!
//! Both are SL-only: OpenSim resolves display names but never pushes these.
//! They surface as typed [`Event`](super::Event)s instead of being dropped to a
//! `Diagnostic::UnknownCapsEvent`. Field names and shapes are cross-checked
//! against the Firestorm viewer's `indra/newview/llviewerdisplayname.cpp`.

use sl_wire::DisplayName;

/// A pushed display-name change (`DisplayNameUpdate`): an avatar's display name
/// changed, so a client mirroring the name cache can refresh its entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayNameUpdate {
    /// The avatar's previous display name (`old_display_name`), useful for a
    /// "X is now known as Y" notification. Empty when the sim omits it.
    pub old_display_name: String,
    /// The avatar's new, full display-name record (the push's `agent` block).
    pub name: DisplayName,
}

/// The result of this agent's own set-display-name request
/// (`SetDisplayNameReply`).
///
/// The display-name change is asynchronous: the viewer POSTs the new name to
/// the `SetDisplayName` capability, which returns immediately, and this push
/// later reports whether the change was accepted. A [`status`](Self::status) of
/// `200` means success; `409` (conflict) means the viewer's cached name was
/// stale and should be re-fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetDisplayNameReply {
    /// The HTTP-like status code of the change request (`status`): `200` on
    /// success, `409` on a stale-name conflict, etc.
    pub status: i32,
    /// A short machine reason phrase (`reason`), e.g. `"OK"`.
    pub reason: String,
    /// On success, the newly set display name (`content.display_name`).
    pub new_display_name: Option<String>,
    /// On failure, a tag identifying the error (`content.error_tag`), which the
    /// reference viewer maps to a localized notification.
    pub error_tag: Option<String>,
}

impl SetDisplayNameReply {
    /// Whether the set-display-name request succeeded (HTTP `200`), mirroring
    /// the reference viewer's `success = (status == HTTP_OK)`.
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        self.status == 200
    }
}
