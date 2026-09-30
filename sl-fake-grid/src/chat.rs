//! Local chat: who hears a line said in a region.
//!
//! A viewer's `ChatFromViewer` and an object's `llSay` are one thing to a
//! simulator — a line, from a source, at a position, on a channel — and both
//! are routed here the same way ([`Line`], published as
//! [`RegionChange::Chat`](crate::world::RegionChange::Chat)):
//!
//! - **Avatars** hear channel 0 and the debug channel, by the line's type:
//!   a whisper within 10 m, a say within 20 m, a shout within 100 m of the
//!   speaker, a region-say anywhere in the region, an owner-say only its
//!   owner ([`Line::heard_by`]). The speaker is an avatar in range of
//!   itself: that echo is how a viewer shows its own line. OpenSim's
//!   `ChatModule.DeliverChatToAvatars` / `TrySendChatMessage`, including its
//!   rule that the typing indicators and the other types carry no distance.
//! - **Scripts** hear through a listen ([`Listens`]), on any channel — and a
//!   non-zero channel reaches nothing else, which is the whole point of the
//!   hidden channel a dialog reply comes back on. A listen hears what an avatar
//!   at the listening object would hear, minus its own object's lines, owner-
//!   says and the typing indicators, and only what its filter admits.
//!
//! Distance is measured from each hearer's position in the region. The fake
//! grid tracks no walking, so an avatar is where it last arrived — its login
//! or its last teleport.
//!
//! Not modelled: chat across a region border (a neighbour's child agents
//! hearing a shout near the edge), parcel privacy (`SeeAVs`), and
//! `llRegionSayTo`'s directed type, which needs a destination no line carries
//! yet.

use std::collections::BTreeMap;

use sl_proto::{ChatSource, ChatType};
use sl_types::chat::ChatChannel;
use sl_types::key::{AgentKey, ObjectKey};
use sl_types::lsl::Vector;
use tokio::sync::mpsc;

/// How far a whisper carries, in metres.
pub const WHISPER_RANGE_M: f32 = 10.0;

/// How far a normal say carries, in metres.
pub const SAY_RANGE_M: f32 = 20.0;

/// How far a shout carries, in metres.
pub const SHOUT_RANGE_M: f32 = 100.0;

/// The public channel: what an avatar types into local chat.
pub const PUBLIC_CHANNEL: ChatChannel = ChatChannel(0);

/// The debug channel script errors are reported on, which avatars hear too.
pub const DEBUG_CHANNEL: ChatChannel = ChatChannel(0x7FFF_FFFF);

/// Where the agent of `sim` stands, region-local metres: where it last
/// arrived — its login or its last teleport — since the fake grid tracks no
/// walking.
#[must_use]
pub(crate) const fn agent_position(sim: &sl_proto::SimSession) -> Vector {
    let at = &sim.arrival_position().position;
    Vector {
        x: at.x(),
        y: at.y(),
        z: at.z(),
    }
}

/// One line said in a region: everything a `ChatFromSimulator` carries, and
/// the channel it was said on.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Who or what said it.
    pub source: ChatSource,
    /// The name the line is shown under.
    pub from_name: String,
    /// The speaker's owner: the agent itself for an avatar.
    pub owner_id: sl_proto::Uuid,
    /// Whisper, say, shout, region-say, owner-say, typing.
    pub chat_type: ChatType,
    /// The channel it was said on.
    pub channel: ChatChannel,
    /// Where it was said, region-local metres.
    pub position: Vector,
    /// What was said.
    pub message: String,
}

impl Line {
    /// How far the line carries, or [`None`] when distance does not decide
    /// who hears it (OpenSim's `TrySendChatMessage`: only whisper, say and
    /// shout have a range).
    #[must_use]
    pub const fn range_m(&self) -> Option<f32> {
        match self.chat_type {
            ChatType::Whisper => Some(WHISPER_RANGE_M),
            ChatType::Normal => Some(SAY_RANGE_M),
            ChatType::Shout => Some(SHOUT_RANGE_M),
            // The typing indicators, region-say, owner-say, the debug channel,
            // a directed line, and any type a newer viewer sends.
            _other => None,
        }
    }

    /// Whether `position` is within the line's range of where it was said.
    fn carries_to(&self, position: &Vector) -> bool {
        self.range_m().is_none_or(|range| {
            let (dx, dy, dz) = (
                position.x - self.position.x,
                position.y - self.position.y,
                position.z - self.position.z,
            );
            dx * dx + dy * dy + dz * dz <= range * range
        })
    }

    /// Whether the avatar of `agent`, standing at `position`, hears the line.
    #[must_use]
    pub fn heard_by(&self, agent: AgentKey, position: &Vector) -> bool {
        if self.channel != PUBLIC_CHANNEL && self.channel != DEBUG_CHANNEL {
            return false;
        }
        match self.chat_type {
            ChatType::Owner => agent.uuid() == self.owner_id,
            // A directed line names its one hearer, and nothing says one yet.
            ChatType::Direct => false,
            _other => self.carries_to(position),
        }
    }

    /// Whether a listen of an object at `position`, on this line's channel,
    /// could hear it — before its filter is asked.
    fn reaches_listen_at(&self, listener: ObjectKey, position: &Vector) -> bool {
        if self.source == ChatSource::Object(listener) {
            return false;
        }
        match self.chat_type {
            ChatType::Owner | ChatType::Direct | ChatType::StartTyping | ChatType::StopTyping => {
                false
            }
            _other => self.carries_to(position),
        }
    }
}

/// What a listen admits: its channel, and optionally one speaker name, one
/// speaker key and one exact message — `llListen`'s four arguments, with the
/// empty string and the null key as [`None`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenFilter {
    /// The channel listened on.
    pub channel: ChatChannel,
    /// Only lines shown under this name.
    pub name: Option<String>,
    /// Only lines from this speaker.
    pub key: Option<sl_proto::Uuid>,
    /// Only this exact message.
    pub message: Option<String>,
}

impl ListenFilter {
    /// Everything said on `channel`.
    #[must_use]
    pub const fn channel(channel: ChatChannel) -> Self {
        Self {
            channel,
            name: None,
            key: None,
            message: None,
        }
    }

    /// Whether the filter admits `line`.
    fn admits(&self, line: &Line) -> bool {
        line.channel == self.channel
            && self
                .name
                .as_ref()
                .is_none_or(|name| *name == line.from_name)
            && self.key.is_none_or(|key| key == line.source.source_id())
            && self
                .message
                .as_ref()
                .is_none_or(|message| *message == line.message)
    }
}

/// A listen's handle: what `llListen` returns and `llListenRemove` /
/// `llListenControl` take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ListenId(u64);

/// One registered listen.
#[derive(Debug, Clone)]
struct Listen {
    /// The object listening; where it stands is where it hears from.
    listener: ObjectKey,
    /// What it admits.
    filter: ListenFilter,
    /// Whether it is on (`llListenControl`).
    active: bool,
    /// Where what it hears goes.
    heard: mpsc::UnboundedSender<Line>,
}

/// The region's listens: what `llListen` registers and every line said in the
/// region is offered to. A region's, not a session's, because the object
/// listening is.
///
/// The fake grid runs no scripts yet, so whoever registers a listen is its
/// consumer: it receives each line the listen hears on the channel
/// [`listen`](Self::listen) returns.
#[derive(Debug, Clone, Default)]
pub struct Listens {
    /// The last id handed out.
    last: u64,
    /// The listens, by id.
    listens: BTreeMap<ListenId, Listen>,
}

impl Listens {
    /// Register a listen of the object `listener` for what `filter` admits;
    /// its id, and the lines it hears.
    pub fn listen(
        &mut self,
        listener: ObjectKey,
        filter: ListenFilter,
    ) -> (ListenId, mpsc::UnboundedReceiver<Line>) {
        self.last = self.last.saturating_add(1);
        let id = ListenId(self.last);
        let (heard, receiver) = mpsc::unbounded_channel();
        drop(self.listens.insert(
            id,
            Listen {
                listener,
                filter,
                active: true,
                heard,
            },
        ));
        (id, receiver)
    }

    /// Remove a listen (`llListenRemove`); whether there was one.
    pub fn remove(&mut self, id: ListenId) -> bool {
        self.listens.remove(&id).is_some()
    }

    /// Turn a listen off or back on (`llListenControl`); whether there was
    /// one.
    pub fn set_active(&mut self, id: ListenId, active: bool) -> bool {
        self.listens
            .get_mut(&id)
            .map(|listen| listen.active = active)
            .is_some()
    }

    /// Offer `line` to every listen, each at the position `where_is` gives
    /// for its object; a listen whose object is not in the region hears
    /// nothing. A listen whose consumer has gone is dropped.
    pub(crate) fn hear(&mut self, line: &Line, where_is: impl Fn(ObjectKey) -> Option<Vector>) {
        self.listens.retain(|_id, listen| {
            let hears = listen.active
                && listen.filter.admits(line)
                && where_is(listen.listener)
                    .is_some_and(|position| line.reaches_listen_at(listen.listener, &position));
            !hears || listen.heard.send(line.clone()).is_ok()
        });
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A speaker at the region's middle.
    fn speaker() -> AgentKey {
        AgentKey::from(sl_proto::Uuid::from_u128(0x5))
    }

    /// A line of `chat_type` on `channel`, said by [`speaker`] at the region's
    /// middle.
    fn line(chat_type: ChatType, channel: ChatChannel) -> Line {
        Line {
            source: ChatSource::Agent(speaker()),
            from_name: "Speaker Resident".to_owned(),
            owner_id: speaker().uuid(),
            chat_type,
            channel,
            position: at(0.0),
            message: "hello".to_owned(),
        }
    }

    /// A point `metres` east of the region's middle.
    const fn at(metres: f32) -> Vector {
        Vector {
            x: 128.0 + metres,
            y: 128.0,
            z: 25.0,
        }
    }

    /// Someone other than the speaker.
    fn hearer() -> AgentKey {
        AgentKey::from(sl_proto::Uuid::from_u128(0x6))
    }

    /// The acceptance's range test: at 15 m a whisper is not heard and a shout
    /// is; the speaker hears itself.
    #[test]
    fn a_whisper_at_15_m_is_unheard_and_a_shout_is_heard() {
        assert!(!line(ChatType::Whisper, PUBLIC_CHANNEL).heard_by(hearer(), &at(15.0)));
        assert!(line(ChatType::Normal, PUBLIC_CHANNEL).heard_by(hearer(), &at(15.0)));
        assert!(line(ChatType::Shout, PUBLIC_CHANNEL).heard_by(hearer(), &at(15.0)));
        assert!(!line(ChatType::Normal, PUBLIC_CHANNEL).heard_by(hearer(), &at(60.0)));
        assert!(line(ChatType::Shout, PUBLIC_CHANNEL).heard_by(hearer(), &at(60.0)));
        assert!(!line(ChatType::Shout, PUBLIC_CHANNEL).heard_by(hearer(), &at(101.0)));
        assert!(line(ChatType::Whisper, PUBLIC_CHANNEL).heard_by(speaker(), &at(0.0)));
    }

    /// A region-say carries anywhere, an owner-say only to the owner, and a
    /// line on any channel but 0 and the debug channel to no avatar at all.
    #[test]
    fn region_owner_and_hidden_channels() {
        assert!(line(ChatType::Region, PUBLIC_CHANNEL).heard_by(hearer(), &at(120.0)));
        let owner_say = line(ChatType::Owner, PUBLIC_CHANNEL);
        assert!(owner_say.heard_by(speaker(), &at(120.0)));
        assert!(!owner_say.heard_by(hearer(), &at(1.0)));
        assert!(!line(ChatType::Normal, ChatChannel(7)).heard_by(hearer(), &at(1.0)));
        assert!(!line(ChatType::Normal, ChatChannel(-42)).heard_by(speaker(), &at(0.0)));
        assert!(line(ChatType::DebugChannel, DEBUG_CHANNEL).heard_by(hearer(), &at(120.0)));
    }

    /// A listen hears its channel within range and through its filter, not
    /// its own object, and nothing while it is off or once it is removed.
    #[test]
    fn a_listen_hears_its_channel_through_its_filter() {
        let listener = ObjectKey::from(sl_proto::Uuid::from_u128(0x0B));
        let elsewhere = |key: ObjectKey| (key == listener).then(|| at(5.0));
        let mut listens = Listens::default();
        let (id, mut heard) = listens.listen(listener, ListenFilter::channel(ChatChannel(7)));
        let (_named, mut named) = listens.listen(
            listener,
            ListenFilter {
                name: Some("Somebody Else".to_owned()),
                ..ListenFilter::channel(ChatChannel(7))
            },
        );

        let on_seven = line(ChatType::Normal, ChatChannel(7));
        listens.hear(&on_seven, elsewhere);
        listens.hear(&line(ChatType::Normal, ChatChannel(8)), elsewhere);
        assert_eq!(heard.try_recv().ok(), Some(on_seven.clone()));
        assert!(heard.try_recv().is_err(), "channel 8 is not channel 7");
        assert!(
            named.try_recv().is_err(),
            "the name filter admits nobody here"
        );

        let mut own = on_seven.clone();
        own.source = ChatSource::Object(listener);
        listens.hear(&own, elsewhere);
        assert!(
            heard.try_recv().is_err(),
            "a listen does not hear its own object"
        );

        assert!(listens.set_active(id, false));
        listens.hear(&on_seven, elsewhere);
        assert!(
            heard.try_recv().is_err(),
            "an inactive listen hears nothing"
        );
        assert!(listens.set_active(id, true));
        let mut far = on_seven.clone();
        far.position = at(40.0);
        listens.hear(&far, elsewhere);
        assert!(heard.try_recv().is_err(), "a say 35 m away is out of range");
        assert!(listens.remove(id));
        listens.hear(&on_seven, elsewhere);
        assert!(heard.try_recv().is_err(), "a removed listen hears nothing");
    }
}
