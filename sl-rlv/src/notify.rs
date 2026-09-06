//! `@notify` — the objects that asked to be told about every change.
//!
//! `@notify:<channel>[;<filter>]=n` is a subscription, not a restriction: the
//! object is not asking for anything to be forbidden, it is asking to be told
//! whenever anything is. Every add, every remove and every `@clear` the state
//! machine sees is broadcast to the subscriptions whose filter matches it, as a
//! line to chat on the channel they named
//! (`RlvBehaviourNotifyHandler`, `rlvhelper.cpp:1876`).
//!
//! Two decisions here are the reference's, and both matter to scripts:
//!
//! - the broadcast hangs off the **command**, not off the enforcement families.
//!   One choke point sees every transition, so a family that forgets to emit
//!   cannot exist — and a command that *failed* is reported too, because RLV
//!   reports invalid commands and a script that stopped hearing about them
//!   would conclude the viewer had stopped listening;
//! - the filter is matched against the `behaviour[:option]` half only, never
//!   against the `=n` the line ends with, so `@notify:2222;detach=n` hears
//!   `@detach=n` *and* `@detach=y` — which is what makes it useful for watching
//!   one restriction rather than one direction.

use std::collections::BTreeMap;

use uuid::Uuid;

/// One line for the consumer to chat back on a channel.
///
/// Producing the line is this crate's job; saying it is not. The reference
/// shouts it on `channel` (`RlvUtil::sendChatReply`, `rlvcommon.cpp:726`), and
/// truncates at the 1023-byte chat limit as any other outgoing chat line is
/// truncated.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RlvNotification {
    /// The channel the subscription named.
    pub channel: i32,
    /// What to say, `/`-prefixed exactly as the reference sends it.
    pub message: String,
}

/// One `@notify` subscription: where to report, and what to report.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RlvNotifySubscription {
    /// The channel this object listens on.
    channel: i32,
    /// The substring the reported text has to contain; empty means everything.
    filter: String,
}

/// Who asked to be told about restriction changes.
///
/// Keyed by object with each object's subscriptions in the order they arrived,
/// which is the iteration order of the reference's
/// `std::multimap<LLUUID, notifyData>` and therefore the order the
/// notifications go out in.
#[derive(Debug, Clone, Default)]
pub(crate) struct RlvNotifyRegistry {
    /// The live subscriptions, by issuing object.
    subscriptions: BTreeMap<Uuid, Vec<RlvNotifySubscription>>,
}

impl RlvNotifyRegistry {
    /// Record `@notify:<channel>[;<filter>]=n` from `object`
    /// (`RlvBehaviourNotifyHandler::addNotify`, `rlvhelper.h:604`).
    pub(crate) fn add(&mut self, object: Uuid, channel: i32, filter: &str) {
        self.subscriptions
            .entry(object)
            .or_default()
            .push(RlvNotifySubscription {
                channel,
                filter: filter.to_owned(),
            });
    }

    /// Drop what `@notify:<channel>[;<filter>]=y` names
    /// (`RlvBehaviourNotifyHandler::removeNotify`, `rlvhelper.h:608`).
    ///
    /// One remove drops one subscription, as the reference's `break` does. It
    /// never has more than one to choose from in practice — the state machine
    /// answers a repeat of a command an object already holds with
    /// [`RlvOutcome::SuccessDuplicate`](crate::RlvOutcome::SuccessDuplicate)
    /// and never gets here — but the removal stays a removal of one so that
    /// this registry cannot be the place two of them disagree.
    pub(crate) fn remove(&mut self, object: Uuid, channel: i32, filter: &str) {
        let Some(held) = self.subscriptions.get_mut(&object) else {
            return;
        };
        if let Some(index) = held
            .iter()
            .position(|entry| entry.channel == channel && entry.filter == filter)
        {
            held.remove(index);
        }
        if held.is_empty() {
            self.subscriptions.remove(&object);
        }
    }

    /// Whether nobody is listening — the cheap check before building the text
    /// of an event that would go nowhere.
    pub(crate) fn is_empty(&self) -> bool {
        self.subscriptions.is_empty()
    }

    /// What one event says to whom.
    ///
    /// The line is the two halves concatenated with nothing between them
    /// (`/` + `text` + `suffix`), and only `text` is offered to the filter
    /// (`RlvBehaviourNotifyHandler::sendNotification`, `rlvhelper.cpp:1904`).
    /// Two subscriptions naming the same channel are two notifications: the
    /// reference does not merge them, and an object that asked twice is
    /// answered twice.
    pub(crate) fn notifications(&self, text: &str, suffix: &str) -> Vec<RlvNotification> {
        self.subscriptions
            .values()
            .flatten()
            .filter(|entry| contains_ignore_case(text, &entry.filter))
            .map(|entry| RlvNotification {
                channel: entry.channel,
                message: format!("/{text}{suffix}"),
            })
            .collect()
    }
}

/// Case-insensitive substring test (`boost::icontains`), with an empty needle
/// matching everything — an unfiltered subscription hears the lot.
///
/// Command text reaches here already lower-cased by
/// [`RlvCommand::parse_field`](crate::RlvCommand::parse_field), so this only
/// earns its keep for a hand-built command; it is written for one anyway,
/// because a filter that quietly stopped matching would look like a viewer that
/// stopped notifying.
fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    needle.is_empty() || haystack.to_lowercase().contains(&needle.to_lowercase())
}
