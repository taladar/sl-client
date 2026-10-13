//! Local chat: who hears a line said in a region.
//!
//! A viewer's `ChatFromViewer` and an object's `llSay` are one thing to a
//! simulator — a line, from a source, at a position, on a channel — and both
//! are routed here the same way ([`Line`], published as
//! [`RegionChange::Chat`](crate::world::RegionChange::Chat)):
//!
//! - **Avatars** hear channel 0 and the debug channel, by the line's type:
//!   a whisper within 10 m, a say within 20 m, a shout within 100 m of the
//!   speaker, an owner-say only its owner ([`ChatPolicy::hears`]). The
//!   speaker is an avatar in range of itself: that echo is how a viewer
//!   shows its own line. On that much the two live grids agree; on what a
//!   viewer may say at all, how long a line may be, and how far the typing
//!   notices and the debug channel carry, they do not, and the grid does as
//!   the flavour it imitates ([`ChatPolicy`]).
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
//! A line carries **over a border**: an avatar standing in a neighbouring
//! region hears it by the same ranges, measured to where it stands there.
//! Which region tells it so is the flavour's ([`OverBorders`]; the
//! `chat-border` conformance case, `book/src/gridspec/chat.md` § Across a
//! border).
//!
//! Not modelled: parcel privacy (`SeeAVs`), and `llRegionSayTo`'s directed
//! type, which needs a destination no line carries yet.

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

/// How far a typing notice is marked fully audible on Second Life, in metres:
/// a say's reach. Further off, and out to a shout's, it is marked barely
/// audible.
pub const TYPING_FULLY_AUDIBLE_M: f32 = SAY_RANGE_M;

/// How many bytes the text of a line and the name it is said under may come
/// to together on Second Life. A longer text is cut — to what fits one
/// `ChatFromSimulator` packet, by the look of it.
pub const SECOND_LIFE_TEXT_AND_NAME_BYTES: usize = 1151;

/// How many characters of a line's text OpenSim keeps
/// (`ChatModule.DeliverChatToAvatars`).
pub const OPENSIM_TEXT_CHARACTERS: usize = 1000;

/// How many bytes of a line's text OpenSim then sends (`Utils.StringToBytes`
/// stops at 1024 with the terminator).
pub const OPENSIM_TEXT_BYTES: usize = 1023;

/// The `Audible` byte of a line heard in full.
pub const FULLY_AUDIBLE: u8 = 1;

/// The `Audible` byte of a line at the edge of earshot.
pub const BARELY_AUDIBLE: u8 = 0;

/// How a grid passes local chat on
/// ([`ImitatedGrid::chat_policy`](crate::ImitatedGrid::chat_policy)) —
/// measured by the `local-chat` conformance case on aditi and the local
/// OpenSim (2026-10-13, `book/src/gridspec/chat.md`):
///
/// | | Second Life | OpenSim |
/// | --- | --- | --- |
/// | a viewer's line of a type other than whisper, say, shout and the typing notices | dropped | passed on as sent, to the whole region |
/// | a viewer's line on the debug channel | passed on as the type it was sent as, as far as that type carries | passed on as a debug line, to the whole region |
/// | a viewer's line on a negative channel | reaches nothing | reaches the scripts listening there |
/// | a typing notice | fully audible within 20 m, barely audible within 100 m, not sent beyond | sent to the whole region, fully audible |
/// | a script's line on the debug channel | within 20 m | the whole region |
/// | a long text | cut so that it and the speaker's name come to 1151 bytes | cut to 1000 characters, and then to 1023 bytes |
/// | `RegionInfo` | states the three ranges | states none |
/// | a line heard over a border | said by the hearer's region, in its coordinates | said by the speaker's region down the hearer's child circuit, in the speaker's region's coordinates |
#[expect(
    clippy::module_name_repetitions,
    reason = "named beside the grid's other policies, where `Policy` alone would say nothing"
)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChatPolicy {
    /// What becomes of a viewer's line whose type is none of whisper, say,
    /// shout and the two typing notices.
    pub odd_viewer_types: OddViewerTypes,
    /// What a viewer's line on the debug channel is passed on as.
    pub viewer_debug_channel: ViewerDebugChannel,
    /// Whether a viewer's line on a negative channel reaches the scripts
    /// that listen there.
    pub viewer_negative_channels: bool,
    /// How far a typing notice goes.
    pub typing: TypingReach,
    /// How far a line of the debug type carries; [`None`] for the whole
    /// region.
    pub debug_range_m: Option<f32>,
    /// How long a line's text may be.
    pub text: TextLimit,
    /// Whether the region's `RegionInfo` states its chat ranges.
    pub states_ranges: bool,
    /// Which region tells an avatar standing over a border of a line it is
    /// in earshot of.
    pub over_borders: OverBorders,
}

/// How a grid carries a line over a region border
/// ([`ChatPolicy::over_borders`]). Both live grids carry it, by the same
/// ranges as within a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverBorders {
    /// The region the hearer stands in says it, where it was said in that
    /// region's own coordinates — a point outside it: Second Life.
    FromTheHearersRegion,
    /// The speaker's region says it down the child circuit the hearer's
    /// viewer holds there, in the speaker's region's coordinates: OpenSim.
    DownTheChildCircuit,
}

/// What a grid does with a viewer's line of a type no viewer sends
/// ([`ChatPolicy::odd_viewer_types`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OddViewerTypes {
    /// Nobody is sent it: Second Life.
    Dropped,
    /// Everybody in the region is sent it as it came, the owner-say and the
    /// directed type included: OpenSim.
    PassedOn,
}

/// What a grid passes a viewer's line on the debug channel on as
/// ([`ChatPolicy::viewer_debug_channel`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerDebugChannel {
    /// The type the viewer sent it as, as far as that type carries: Second
    /// Life.
    KeepsItsType,
    /// A debug line, whatever it was sent as: OpenSim.
    SaidAsDebug,
}

/// How far a grid sends a typing notice ([`ChatPolicy::typing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypingReach {
    /// As far as a shout, marked barely audible past a say's reach: Second
    /// Life.
    ToAShout,
    /// The whole region, fully audible: OpenSim.
    Everywhere,
}

/// How long a grid lets a line's text be ([`ChatPolicy::text`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextLimit {
    /// The text and the name it is said under come to
    /// [`SECOND_LIFE_TEXT_AND_NAME_BYTES`] at most: Second Life.
    WithTheName,
    /// [`OPENSIM_TEXT_CHARACTERS`] characters, and of those
    /// [`OPENSIM_TEXT_BYTES`] bytes: OpenSim.
    CharactersThenBytes,
}

impl Default for ChatPolicy {
    /// Second Life's, as the grid's own default flavour is.
    fn default() -> Self {
        crate::ImitatedGrid::SecondLife.chat_policy()
    }
}

/// The longest prefix of `text` of at most `bytes` bytes that ends between
/// two characters.
fn prefix_of_bytes(text: &str, bytes: usize) -> &str {
    let mut end = bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.get(..end).unwrap_or(text)
}

impl TextLimit {
    /// What the grid keeps of `text` said under `name`.
    #[must_use]
    pub fn kept<'text>(self, text: &'text str, name: &str) -> &'text str {
        match self {
            Self::WithTheName => prefix_of_bytes(
                text,
                SECOND_LIFE_TEXT_AND_NAME_BYTES.saturating_sub(name.len()),
            ),
            Self::CharactersThenBytes => {
                let characters = text
                    .char_indices()
                    .nth(OPENSIM_TEXT_CHARACTERS)
                    .map_or(text.len(), |(at, _character)| at);
                prefix_of_bytes(text, characters.min(OPENSIM_TEXT_BYTES))
            }
        }
    }
}

impl ChatPolicy {
    /// A viewer's line as the grid says it in the region: retyped and cut as
    /// the grid does, or [`None`] where the grid passes nothing of it on.
    #[must_use]
    pub fn viewer_line(&self, mut line: Line) -> Option<Line> {
        if line.channel.0 < 0 && !self.viewer_negative_channels {
            return None;
        }
        let ordinary = matches!(
            line.chat_type,
            ChatType::Whisper
                | ChatType::Normal
                | ChatType::Shout
                | ChatType::StartTyping
                | ChatType::StopTyping
        );
        if !ordinary && self.odd_viewer_types == OddViewerTypes::Dropped {
            return None;
        }
        if line.channel == DEBUG_CHANNEL
            && self.viewer_debug_channel == ViewerDebugChannel::SaidAsDebug
        {
            line.chat_type = ChatType::DebugChannel;
        }
        line.message = self.text.kept(&line.message, &line.from_name).to_owned();
        Some(line)
    }

    /// Whether the avatar of `agent`, standing at `position`, is sent `line`
    /// — and the `Audible` byte it is sent with.
    #[must_use]
    pub fn hears(&self, line: &Line, agent: AgentKey, position: &Vector) -> Option<u8> {
        if line.channel != PUBLIC_CHANNEL && line.channel != DEBUG_CHANNEL {
            return None;
        }
        let from_a_viewer = matches!(line.source, ChatSource::Agent(_));
        let within = |range: f32| line.distance_squared_to(position) <= range * range;
        let heard = match line.chat_type {
            ChatType::Whisper => within(WHISPER_RANGE_M),
            ChatType::Normal => within(SAY_RANGE_M),
            ChatType::Shout => within(SHOUT_RANGE_M),
            ChatType::StartTyping | ChatType::StopTyping => match self.typing {
                TypingReach::Everywhere => true,
                TypingReach::ToAShout => {
                    if within(TYPING_FULLY_AUDIBLE_M) {
                        true
                    } else {
                        return within(SHOUT_RANGE_M).then_some(BARELY_AUDIBLE);
                    }
                }
            },
            ChatType::DebugChannel => self.debug_range_m.is_none_or(within),
            // A viewer's line of a type only a simulator gives a line — an
            // owner-say, a directed line — goes, where it goes at all, to
            // everybody.
            _other if from_a_viewer => true,
            ChatType::Owner => agent.uuid() == line.owner_id,
            // A directed line names its one hearer, and nothing says one yet.
            ChatType::Direct => false,
            _other => true,
        };
        heard.then_some(FULLY_AUDIBLE)
    }
}

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
        self.range_m()
            .is_none_or(|range| self.distance_squared_to(position) <= range * range)
    }

    /// The square of the distance from where the line was said to
    /// `position`.
    fn distance_squared_to(&self, position: &Vector) -> f32 {
        let (dx, dy, dz) = (
            position.x - self.position.x,
            position.y - self.position.y,
            position.z - self.position.z,
        );
        dz.mul_add(dz, dx.mul_add(dx, dy * dy))
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
    use crate::ImitatedGrid;

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

    /// Whether a hearer `metres` away is sent `line` under `policy`, and how
    /// audible.
    fn heard(policy: &ChatPolicy, line: &Line, metres: f32) -> Option<u8> {
        policy.hears(line, hearer(), &at(metres))
    }

    /// The same line as an object's: said by a prim the speaker owns.
    fn of_an_object(mut line: Line) -> Line {
        line.source = ChatSource::Object(ObjectKey::from(sl_proto::Uuid::from_u128(0x0B)));
        line
    }

    /// The ranges both grids agree on: at 15 m a whisper is not heard and a
    /// shout is; the speaker hears itself.
    #[test]
    fn a_whisper_at_15_m_is_unheard_and_a_shout_is_heard() {
        for grid in [ImitatedGrid::SecondLife, ImitatedGrid::OpenSim] {
            let policy = grid.chat_policy();
            let said = |chat_type| line(chat_type, PUBLIC_CHANNEL);
            assert_eq!(heard(&policy, &said(ChatType::Whisper), 15.0), None);
            assert_eq!(heard(&policy, &said(ChatType::Normal), 15.0), Some(1));
            assert_eq!(heard(&policy, &said(ChatType::Shout), 15.0), Some(1));
            assert_eq!(heard(&policy, &said(ChatType::Normal), 60.0), None);
            assert_eq!(heard(&policy, &said(ChatType::Shout), 60.0), Some(1));
            assert_eq!(heard(&policy, &said(ChatType::Shout), 101.0), None);
            assert_eq!(
                policy.hears(&said(ChatType::Whisper), speaker(), &at(0.0)),
                Some(1)
            );
            // An object's owner-say only to the owner, a line on any channel
            // but 0 and the debug channel to no avatar at all.
            let owner_say = of_an_object(said(ChatType::Owner));
            assert_eq!(policy.hears(&owner_say, speaker(), &at(120.0)), Some(1));
            assert_eq!(heard(&policy, &owner_say, 1.0), None);
            assert_eq!(
                heard(&policy, &line(ChatType::Normal, ChatChannel(7)), 1.0),
                None
            );
        }
    }

    /// Where the grids part: the typing notices and the debug channel.
    #[test]
    fn typing_and_the_debug_channel_carry_as_far_as_the_flavour_says() {
        let second_life = ImitatedGrid::SecondLife.chat_policy();
        let opensim = ImitatedGrid::OpenSim.chat_policy();
        let typing = line(ChatType::StartTyping, PUBLIC_CHANNEL);
        assert_eq!(heard(&second_life, &typing, 15.0), Some(FULLY_AUDIBLE));
        assert_eq!(heard(&second_life, &typing, 36.0), Some(BARELY_AUDIBLE));
        assert_eq!(heard(&second_life, &typing, 121.0), None);
        assert_eq!(heard(&opensim, &typing, 121.0), Some(FULLY_AUDIBLE));
        let debug = of_an_object(line(ChatType::DebugChannel, DEBUG_CHANNEL));
        assert_eq!(heard(&second_life, &debug, 15.0), Some(FULLY_AUDIBLE));
        assert_eq!(heard(&second_life, &debug, 36.0), None);
        assert_eq!(heard(&opensim, &debug, 121.0), Some(FULLY_AUDIBLE));
    }

    /// What each grid makes of a viewer's line before anybody hears it.
    #[test]
    fn a_viewers_line_as_each_flavour_says_it() {
        let second_life = ImitatedGrid::SecondLife.chat_policy();
        let opensim = ImitatedGrid::OpenSim.chat_policy();
        // A type no viewer sends.
        let owner_say = line(ChatType::Owner, PUBLIC_CHANNEL);
        assert_eq!(second_life.viewer_line(owner_say.clone()), None);
        let passed = opensim.viewer_line(owner_say.clone());
        assert_eq!(passed, Some(owner_say.clone()));
        assert_eq!(heard(&opensim, &owner_say, 120.0), Some(FULLY_AUDIBLE));
        // The debug channel.
        let on_debug = line(ChatType::Shout, DEBUG_CHANNEL);
        assert_eq!(
            second_life
                .viewer_line(on_debug.clone())
                .map(|line| line.chat_type),
            Some(ChatType::Shout)
        );
        assert_eq!(
            opensim.viewer_line(on_debug).map(|line| line.chat_type),
            Some(ChatType::DebugChannel)
        );
        // A negative channel.
        let hidden = line(ChatType::Normal, ChatChannel(-1));
        assert_eq!(second_life.viewer_line(hidden.clone()), None);
        assert_eq!(opensim.viewer_line(hidden.clone()), Some(hidden));
    }

    /// A long text is cut by the name it is said under on Second Life, and by
    /// characters and then bytes on OpenSim — never inside a character.
    #[test]
    fn a_long_text_is_cut_as_each_flavour_cuts_it() {
        let name = "Speaker Resident";
        let ascii = "x".repeat(3000);
        assert_eq!(
            TextLimit::WithTheName.kept(&ascii, name).len(),
            SECOND_LIFE_TEXT_AND_NAME_BYTES - name.len()
        );
        assert_eq!(TextLimit::WithTheName.kept("short", name), "short");
        assert_eq!(
            TextLimit::CharactersThenBytes.kept(&ascii, name).len(),
            1000
        );
        // Six bytes of tag and then two bytes a character, as the census
        // sends them: OpenSim kept 1022 bytes of 1200.
        let wide = format!("lc-12-{}", "\u{e9}".repeat(597));
        assert_eq!(TextLimit::CharactersThenBytes.kept(&wide, name).len(), 1022);
        let wide = "\u{e9}".repeat(2000);
        let kept = TextLimit::WithTheName.kept(&wide, name);
        assert_eq!(kept.len() % 2, 0, "cut between two characters");
        assert_eq!(kept.len(), 1134);
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
