//! Outbound viewer reports to the grid: snapshot postcards.
//!
//! Abuse / bug reports live in `sl-wire`'s [`AbuseReport`](sl_wire::AbuseReport)
//! (shared by the legacy `UserReport` UDP message and the `SendUserReport`
//! capability); this module holds the [`Postcard`] payload of the `SendPostcard`
//! UDP message, which emails a snapshot.

use uuid::Uuid;

/// A snapshot postcard to email via the `SendPostcard` UDP message. The viewer
/// uploads the snapshot as a temporary asset first, then sends this referencing
/// that `asset_id`; the simulator renders and emails it. Fire-and-forget — there
/// is no reply.
///
/// `pos_global` is the global position the snapshot was taken at (so the email
/// can carry an SLurl back to the spot). `to` / `from` are email addresses (the
/// grid may restrict `to` to a single address); `name` is the sender's display
/// name; `subject` and `message` are the email subject and body. `allow_publish`
/// asks the grid to allow the snapshot on its web gallery, and `mature_publish`
/// marks that gallery entry mature.
#[derive(Debug, Clone, PartialEq)]
pub struct Postcard {
    /// The uploaded snapshot asset id to email.
    pub asset_id: Uuid,
    /// The global position the snapshot was taken at (metres).
    pub pos_global: [f64; 3],
    /// The destination email address(es).
    pub to: String,
    /// The source email address.
    pub from: String,
    /// The sender's name.
    pub name: String,
    /// The email subject line.
    pub subject: String,
    /// The email body text.
    pub message: String,
    /// Whether to allow publishing the snapshot on the grid's web gallery.
    pub allow_publish: bool,
    /// Whether that gallery entry is marked mature.
    pub mature_publish: bool,
}

impl Default for Postcard {
    fn default() -> Self {
        Self {
            asset_id: Uuid::nil(),
            pos_global: [0.0, 0.0, 0.0],
            to: String::new(),
            from: String::new(),
            name: String::new(),
            subject: String::new(),
            message: String::new(),
            allow_publish: false,
            mature_publish: false,
        }
    }
}
