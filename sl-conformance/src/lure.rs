//! What a teleport offer looked like on the wire, and what else a grid said
//! around one.
//!
//! The `teleport-offer-*`, `teleport-request` and `teleport-lure-*` cases all
//! pass the same few instant messages between two avatars and differ in what
//! they then do with the lure. The shape of those messages — which dialog, what
//! the id is, what the binary bucket holds, whether the offerer hears anything
//! back — is what the two live grids disagree on
//! (`book/src/gridspec/teleport.md`, *Offers and requests*), so the reading of
//! them is here once.

use std::time::Duration;

use sl_client_tokio::{AgentKey, Command, Event, ImDialog, InstantMessage, LureId};
use sl_wire::FakeParcelId;

use crate::context::{Session, TestFailure};
use crate::metrics::Metrics;
use crate::support::REPLY_TIMEOUT;

/// How long a case watches an avatar for anything the grid says to it about a
/// lure it offered, accepted or declined. Both grids answer within a second
/// when they answer at all; the rest of the window is what makes "nothing" a
/// measurement.
pub const NOTICE_WINDOW: Duration = Duration::from_secs(10);

/// Something a grid said to an avatar while a case watched it: an instant
/// message of any dialog, or an alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// An `ImprovedInstantMessage`.
    Im {
        /// The dialog.
        dialog: ImDialog,
        /// Who it claims to be from.
        from: AgentKey,
        /// The dialog-dependent id.
        id: sl_client_tokio::Uuid,
        /// The text.
        message: String,
        /// Whether it was flagged as stored while the recipient was away.
        offline: bool,
    },
    /// An `AlertMessage` or `AgentAlertMessage`.
    Alert {
        /// The text.
        message: String,
        /// The keys of the `AlertInfo` blocks beside it.
        keys: Vec<String>,
    },
}

impl Notice {
    /// The notice on one line, for a record.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Im {
                dialog,
                message,
                offline,
                ..
            } => {
                let stored = if *offline { " (offline)" } else { "" };
                format!("im {dialog:?}{stored} {message:?}")
            }
            Self::Alert { message, keys } => format!("alert {message:?} [{}]", keys.join(",")),
        }
    }

    /// Whether this is an instant message of `dialog` from `from`.
    #[must_use]
    pub fn is_im(&self, dialog: ImDialog, from: AgentKey) -> bool {
        matches!(self, Self::Im { dialog: found, from: sender, .. }
            if *found == dialog && *sender == from)
    }
}

/// The notice an event is, when it is one.
///
/// Typing notifications are left out: they are neither an answer to a lure nor
/// rare, and a bystander starting to type would otherwise be counted as one.
fn notice_of(event: &Event) -> Option<Notice> {
    match event {
        Event::InstantMessageReceived(im)
            if !matches!(im.dialog, ImDialog::TypingStart | ImDialog::TypingStop) =>
        {
            Some(Notice::Im {
                dialog: im.dialog,
                from: im.from_agent_id,
                id: im.id,
                message: im.message.clone(),
                offline: im.offline,
            })
        }
        Event::AlertMessage {
            message,
            alert_info,
            ..
        } => Some(Notice::Alert {
            message: message.clone(),
            keys: alert_info.iter().map(|info| info.message.clone()).collect(),
        }),
        Event::AgentAlertMessage { message, .. } => Some(Notice::Alert {
            message: message.clone(),
            keys: Vec::new(),
        }),
        _ => None,
    }
}

/// Watches `session` for `window` and returns every instant message and alert
/// that arrived, in order.
///
/// A wait whose predicate never matches, so it always ends in its own timeout:
/// the point is what it sees on the way. Events the session's forwarder queued
/// before the call are seen too, so a watch placed after another avatar's step
/// still catches what that step provoked.
///
/// # Errors
///
/// Propagates an intervening disconnect; the window elapsing is the normal
/// exit.
pub async fn watch_notices(
    session: &mut Session,
    window: Duration,
) -> Result<Vec<Notice>, TestFailure> {
    let mut seen = Vec::new();
    match session
        .wait_for(window, |event| {
            seen.extend(notice_of(event));
            Option::<()>::None
        })
        .await
    {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(seen),
        Err(other) => Err(other),
    }
}

/// Records what a watch saw under `name`: the count, and each notice on a line
/// of its own.
pub fn record_notices(name: &str, notices: &[Notice], metrics: &mut Metrics) {
    metrics.set(
        &format!("{name}_count"),
        i64::try_from(notices.len()).unwrap_or(-1),
    );
    metrics.set(
        name,
        notices
            .iter()
            .map(Notice::describe)
            .collect::<Vec<_>>()
            .join(" | "),
    );
}

/// Waits for an instant message of `dialog` from `from` whose text is
/// `message`.
///
/// # Errors
///
/// Propagates a [`Session::wait_for`] timeout or disconnect.
pub async fn wait_for_dialog(
    session: &mut Session,
    from: AgentKey,
    dialog: ImDialog,
    message: &str,
) -> Result<InstantMessage, TestFailure> {
    session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::InstantMessageReceived(im)
                if im.from_agent_id == from && im.dialog == dialog && im.message == message =>
            {
                Some((**im).clone())
            }
            _ => None,
        })
        .await
}

/// Has `offerer` offer `target` a teleport with `message` and waits for the
/// offer to reach `target`.
///
/// # Errors
///
/// Propagates a closed command channel, or the wait's timeout or disconnect.
pub async fn offer_and_receive(
    offerer: &Session,
    target: &mut Session,
    message: &str,
) -> Result<InstantMessage, TestFailure> {
    let offerer_id = agent_id(offerer, "offerer")?;
    let target_id = agent_id(target, "target")?;
    offerer
        .send(Command::OfferTeleport {
            targets: vec![target_id],
            message: message.to_owned(),
        })
        .await?;
    wait_for_dialog(target, offerer_id, ImDialog::LureUser, message).await
}

/// The agent id `session` logged in with.
///
/// # Errors
///
/// [`TestFailure::Assertion`] when the login reported none.
pub fn agent_id(session: &Session, who: &str) -> Result<AgentKey, TestFailure> {
    session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion(format!("the {who}'s login reported no agent id")))
}

/// The lure id an offer carries.
#[must_use]
pub fn offered_id(offer: &InstantMessage) -> LureId {
    LureId::from(offer.id)
}

/// How a lure id reads: `place` when it has the layout of OpenSim's packed
/// region handle and position, `opaque` otherwise.
#[must_use]
pub const fn id_kind(offer: &InstantMessage) -> &'static str {
    if FakeParcelId::parse(offer.id).is_some() {
        "place"
    } else {
        "opaque"
    }
}

/// The binary bucket as text, without the terminator a grid may have put on
/// it.
#[must_use]
pub fn bucket_text(im: &InstantMessage) -> String {
    String::from_utf8_lossy(&im.binary_bucket)
        .trim_end_matches('\0')
        .to_owned()
}

/// Records every field of an offer or request under `prefix`.
pub fn record_im(prefix: &str, im: &InstantMessage, metrics: &mut Metrics) {
    let key = |name: &str| format!("{prefix}{name}");
    metrics.set(&key("dialog"), format!("{:?}", im.dialog));
    metrics.set(&key("from_name"), im.from_agent_name.clone());
    metrics.set(&key("message"), im.message.clone());
    metrics.set(&key("id"), im.id.to_string());
    metrics.set(&key("id_kind"), id_kind(im));
    if let Some(place) = FakeParcelId::parse(im.id) {
        metrics.set(
            &key("id_place"),
            format!(
                "{} {} {} {}",
                place.region_handle.0, place.x, place.y, place.z
            ),
        );
    }
    metrics.set(
        &key("region_id"),
        im.region_id
            .map_or_else(|| "none".to_owned(), |id| id.to_string()),
    );
    metrics.set(
        &key("position"),
        format!(
            "{:.2} {:.2} {:.2}",
            im.position.x(),
            im.position.y(),
            im.position.z()
        ),
    );
    metrics.set(&key("offline"), im.offline);
    metrics.set(&key("has_timestamp"), im.timestamp.is_some());
    metrics.set(&key("parent_estate_id"), i64::from(im.parent_estate_id));
    metrics.set(&key("from_group"), im.from_group);
    metrics.set(
        &key("bucket_len"),
        i64::try_from(im.binary_bucket.len()).unwrap_or(-1),
    );
    metrics.set(&key("bucket"), bucket_text(im));
}
