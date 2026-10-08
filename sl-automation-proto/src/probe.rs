//! State probes: what a viewer reports about the state a test asserts on that
//! is not one widget — the conversations, the notifications, the status bar,
//! the own agent, the environment drawn, the selection, the inventory, the logs
//! of what happened and whether the scene has settled.
//!
//! Every readout is read from a model the viewer already keeps, never scraped
//! from the widgets that draw it.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Which conversation a transcript belongs to.
///
/// In JSON the kind is a tag beside the id: `{"kind":"nearby"}`,
/// `{"kind":"direct","id":"…"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ConversationRef {
    /// Local chat: everyone and everything in earshot.
    Nearby,
    /// A one-to-one instant message session, by the other resident's agent id.
    Direct(Uuid),
    /// A group's chat session, by the group id.
    Group(Uuid),
    /// An ad-hoc conference, by its session id.
    Conference(Uuid),
}

/// Who spoke a transcript line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerKind {
    /// A resident's avatar, the own one included.
    Agent,
    /// An in-world object.
    Object,
    /// The region or the grid.
    System,
}

/// How a line of local chat was said.
///
/// The simulator does not say which channel a heard line was on: what reaches
/// a viewer is public chat (channel 0) or one of the kinds only a script uses
/// ([`Owner`](Self::Owner), [`Direct`](Self::Direct), [`Region`](Self::Region),
/// [`Debug`](Self::Debug)). The channel of a line the own agent *said* is in
/// the outbound command it was sent with ([`LogStream::Command`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatKind {
    /// A whisper (10 m).
    Whisper,
    /// Ordinary speech (20 m).
    Normal,
    /// A shout (100 m).
    Shout,
    /// Region-wide (`llRegionSay`).
    Region,
    /// From an object to its owner only (`llOwnerSay`).
    Owner,
    /// From an object to this agent only (`llRegionSayTo`).
    Direct,
    /// The script debug channel.
    Debug,
    /// A chat type byte the viewer does not name.
    Other(u8),
}

/// One line of a transcript, as the conversation shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptLine {
    /// Whether the own agent said it.
    pub own: bool,
    /// The speaker's name as the transcript shows it (a resident's legacy
    /// name, an object's name).
    pub speaker: String,
    /// The speaker's id: an agent id, an object id; absent for the system.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_id: Option<Uuid>,
    /// What kind of thing spoke.
    pub speaker_kind: SpeakerKind,
    /// How it was said, for local chat; absent for an instant message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_kind: Option<ChatKind>,
    /// What was said.
    pub text: String,
}

/// One conversation: its transcript and whether it waits on the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationReadout {
    /// Which conversation this is.
    pub conversation: ConversationRef,
    /// Lines that arrived while it was not the one shown.
    pub unread: u32,
    /// Whether it is an invitation the user has not accepted yet.
    pub pending_invite: bool,
    /// The lines of this session, oldest first. Lines recalled from an earlier
    /// session's log are not among them.
    pub lines: Vec<TranscriptLine>,
}

/// A button a notification offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferedButton {
    /// The name its response carries — locale-independent, what a test
    /// answers with.
    pub name: String,
    /// The label as the user reads it, in the viewer's current locale.
    pub label: String,
    /// Whether it is the choice Enter and expiry take.
    pub default: bool,
}

/// One notification the viewer raised: a toast, an alert, a modal dialog.
///
/// Its buttons are also nodes of the semantic UI while it is shown, so a test
/// answers it with a locator like any other button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationReadout {
    /// The viewer's id for it, unique within the session.
    pub id: u64,
    /// The catalogue template it was raised from — locale-independent.
    pub template: String,
    /// Its text as the user reads it.
    pub text: String,
    /// The buttons it offers, in the order it shows them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<OfferedButton>,
    /// Whether it is on screen (or queued to be) now.
    pub live: bool,
    /// The button it was answered with, once it was; absent while live and for
    /// one that expired or was dismissed without a choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
}

/// A wall-clock time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ClockTime {
    /// The hour, `0..24`.
    pub hour: u8,
    /// The minute, `0..60`.
    pub minute: u8,
}

/// What the status bar reports, read from the models it is drawn from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusReadout {
    /// The current region's name, once the region handshake completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// The name of the parcel the agent stands on, once known and when it has
    /// one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parcel: Option<String>,
    /// The L$ balance, once the grid said it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<i64>,
    /// Second Life Time (US Pacific) now.
    pub time: ClockTime,
}

/// A region the agent is in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionReadout {
    /// The region's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The region's handle: its grid position, south-west corner in metres,
    /// `x` in the high 32 bits.
    pub handle: u64,
    /// The region's id, once the handshake stated it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
}

/// Where a teleport is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TeleportState {
    /// No teleport since login, or the last one's progress is no longer shown.
    Idle,
    /// Asked for; the grid has not answered yet.
    Requested,
    /// The simulator is reporting progress.
    InProgress,
    /// The destination answered; the handover to it is underway.
    Arriving,
    /// It arrived.
    Succeeded,
    /// It failed; the agent is still where it was.
    Failed {
        /// The reason the grid gave (or the viewer's, for its own watchdog).
        reason: String,
        /// Further detail the grid attached, if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

/// The own agent's teleport, as the viewer's progress display tracks it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeleportReadout {
    /// Where it is.
    #[serde(flatten)]
    pub state: TeleportState,
    /// Where it goes, as the surface that started it named it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    /// The simulator's last progress message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Whether the viewer judged it slow.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub stalled: bool,
}

/// How the camera is driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraView {
    /// Behind and above the avatar, orbiting it.
    ThirdPerson,
    /// Through the avatar's eyes.
    Mouselook,
    /// Free flight, detached from the avatar.
    Flycam,
}

/// The own agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentReadout {
    /// The agent id, once logged in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<Uuid>,
    /// The region the agent is in, once the handshake completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<RegionReadout>,
    /// Where the agent stands, in region-local metres (Second Life axes), once
    /// known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 3]>,
    /// The object the agent sits on, by full id; absent when standing or
    /// sitting on the ground.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seated_on: Option<Uuid>,
    /// The teleport, where the viewer tracks one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teleport: Option<TeleportReadout>,
    /// How the camera is driven, where the viewer has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraView>,
    /// The heading the own avatar faces, in radians about the Second Life up
    /// axis counter-clockwise from east (north is π/2), wrapped to `-π..=π` —
    /// the viewer's own heading, once it is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<f32>,
    /// Where the camera's eye is, in region-local metres (Second Life axes)
    /// of the agent's region, once the viewer draws one — what a test reads
    /// to see the camera framed somewhere, or brought back behind the avatar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_eye: Option<[f32; 3]>,
    /// The baked textures the viewer uploaded for its own avatar and named in
    /// its appearance, sorted — on a grid that leaves baking to the viewer,
    /// once it has published; empty where the grid bakes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub published_bakes: Vec<Uuid>,
}

/// What the viewer's world map knows: the regions the grid named, the items
/// it reported, and how its tiles fared.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldMapReadout {
    /// Every region the grid has named, sorted by cell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<MapRegionReadout>,
    /// Every item layer the grid has reported, sorted by region and kind. A
    /// layer the map asked for and got nothing in is not here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<MapItemLayerReadout>,
    /// The tile server the map fetches from, once it resolved one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_server: Option<String>,
    /// How many tiles are decoded and drawable.
    pub tiles_ready: usize,
    /// How many tiles are still being fetched.
    pub tiles_pending: usize,
    /// How many tiles the server does not have, or failed to send.
    pub tiles_absent: usize,
}

/// A region the world map knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapRegionReadout {
    /// Its name.
    pub name: String,
    /// Its grid cell, `[x, y]`.
    pub grid: [u32; 2],
}

/// One kind of item in one region of the world map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapItemLayerReadout {
    /// The grid cell of the region the items lie in, `[x, y]`.
    pub grid: [u32; 2],
    /// The item type's wire code: 6 for agent locations, 1 for telehubs, 7
    /// for land for sale.
    pub kind: u32,
    /// The items.
    pub items: Vec<MapItemReadout>,
}

/// One item of the world map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapItemReadout {
    /// Where it lies in its region, `[east, north]` in metres.
    pub at: [f64; 2],
    /// Its `Extra`: the agents counted at an agent location, a parcel's area.
    pub extra: i32,
    /// Its `Extra2`: a parcel's price, 1 for an infohub.
    pub extra2: i32,
    /// Whether the map draws it: an agent location that counts nobody is the
    /// grid's way of saying a region is empty, and is not drawn.
    pub drawn: bool,
}

/// The environment the viewer draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentReadout {
    /// The sky being drawn, once the viewer has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sky: Option<SkyReadout>,
    /// Whether the viewer's own local sky — a preset pinned from World ▸
    /// Environment, or a script's `@setenv_*` — is drawn instead of the shared
    /// one.
    pub local_sky: bool,
    /// How far the cross-fade a manual change started has got, `0.0..=1.0`,
    /// while one runs; absent when the sky is not fading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<f32>,
    /// The water being drawn, once the viewer has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub water: Option<WaterReadout>,
    /// The windows previewing what they edit over everything else — each
    /// named by its floater id, the one drawn on top last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previewing: Vec<String>,
}

/// The water being drawn, in the settings' own units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaterReadout {
    /// The water frame's name.
    pub name: String,
    /// The underwater fog density.
    pub fog_density: f32,
}

/// The sky being drawn, in the settings' own units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkyReadout {
    /// The sky frame's name.
    pub name: String,
    /// The ambient light colour, RGB.
    pub ambient: [f32; 3],
    /// The haze density.
    pub haze_density: f32,
    /// Where the sun stands: its azimuth (`0.0..TAU`, counter-clockwise from
    /// east) and its elevation above the horizon, in radians.
    pub sun: [f32; 2],
}

/// One selected object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SelectedObject {
    /// Its full id.
    pub full_id: Uuid,
    /// Its region-local id.
    pub local_id: u32,
    /// Whether it is the primary selection — the one the edit tools show.
    pub primary: bool,
}

/// Where an inventory path starts.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum InventoryRoot {
    /// The agent's own inventory.
    #[default]
    Agent,
    /// The shared library.
    Library,
}

/// One entry of an inventory folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryEntry {
    /// The folder's or item's id.
    pub id: Uuid,
    /// Its name.
    pub name: String,
    /// What it is: a folder's preferred type, an item's asset type, in the
    /// viewer's spelling (`notecard`, `object`, `current_outfit`, …).
    pub kind: String,
}

/// One inventory folder and what the viewer knows of its contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryFolderReadout {
    /// The folder's id.
    pub id: Uuid,
    /// Its name.
    pub name: String,
    /// Whether its contents have been fetched; until they are, `items` is
    /// empty whatever the folder holds.
    pub loaded: bool,
    /// Its sub-folders, by name.
    pub folders: Vec<InventoryEntry>,
    /// Its items, by name.
    pub items: Vec<InventoryEntry>,
}

/// Which of the viewer's logs an entry is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    /// An event the session reported: something the grid said or did.
    Event,
    /// A command the viewer sent to the session: something it asked the grid.
    Command,
    /// A user interface action: a button, a menu entry, a shortcut.
    UiAction,
    /// A feedback sound the viewer raised — a radar alert, the typing chirp —
    /// whether or not it was audible.
    Sound,
}

impl LogStream {
    /// The stream's serialized spelling, used for display too.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Command => "command",
            Self::UiAction => "ui_action",
            Self::Sound => "sound",
        }
    }
}

impl fmt::Display for LogStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One entry of the viewer's event log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// Its sequence number: one counter over every stream, so the order of two
    /// entries is the order they were recorded in.
    pub seq: u64,
    /// Which log it is from.
    pub stream: LogStream,
    /// What it is: an event's or a command's variant name (`ChatReceived`,
    /// `Chat`), a UI action's `element.action`, a sound's name
    /// (`radar_alert`).
    pub kind: String,
    /// The whole entry as the viewer prints it, cut short past a limit.
    pub detail: String,
}

/// A read of a log from a cursor.
///
/// A log is bounded; a reader that falls further behind than it holds is told
/// how many entries it missed rather than silently skipping them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogPage<T> {
    /// The entries from the cursor on, oldest first.
    pub entries: Vec<T>,
    /// The cursor for the next read: one past the last entry read.
    pub next: u64,
    /// How many entries after the cursor were dropped before this read.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped: u64,
}

impl<T> Default for LogPage<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next: 0,
            dropped: 0,
        }
    }
}

/// Whether a count is zero, to leave it out of the JSON.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
const fn is_zero(count: &u64) -> bool {
    *count == 0
}

/// How grave a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    /// Something went wrong that the viewer worked around.
    Warn,
    /// Something went wrong.
    Error,
}

/// One warning or error the viewer logged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticLine {
    /// Its sequence number among the diagnostics.
    pub seq: u64,
    /// How grave it is.
    pub level: LogLevel,
    /// The module that logged it.
    pub target: String,
    /// The message and its fields, as the log prints them.
    pub message: String,
}

/// The warnings and errors the viewer logged.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DiagnosticsReadout {
    /// How many warnings, ever.
    pub warnings: u64,
    /// How many errors, ever.
    pub errors: u64,
    /// The lines from the cursor on.
    pub lines: LogPage<DiagnosticLine>,
}

/// Whether the viewer has settled: everything the scene asked for has arrived
/// and every render pipeline is ready.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct QuiescenceReadout {
    /// Whether a region handshake completed — quiet means nothing before it.
    pub region_up: bool,
    /// Assets in flight or queued and decoded work not yet built, across every
    /// store; absent in a viewer with no asset stores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outstanding: Option<u64>,
    /// The outstanding work by bucket — `<store>.<stage>` for an asset store
    /// (`textures.downloading`), the queue's name for a build queue — the
    /// empty ones left out: what a wait that never goes quiet reports.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outstanding_by: BTreeMap<String, u64>,
    /// Render pipelines queued or compiling; absent in a viewer that does not
    /// render.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting_pipelines: Option<u32>,
}

impl QuiescenceReadout {
    /// Whether the region is up and nothing is outstanding or compiling.
    #[must_use]
    pub fn is_quiet(&self) -> bool {
        self.region_up
            && self.outstanding.unwrap_or(0) == 0
            && self.waiting_pipelines.unwrap_or(0) == 0
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde::Serialize;
    use serde::de::DeserializeOwned;

    use super::{
        AgentReadout, CameraView, ChatKind, ClockTime, ConversationReadout, ConversationRef,
        DiagnosticLine, DiagnosticsReadout, EnvironmentReadout, InventoryEntry,
        InventoryFolderReadout, LogEntry, LogLevel, LogPage, LogStream, MapItemLayerReadout,
        MapItemReadout, MapRegionReadout, NotificationReadout, OfferedButton, QuiescenceReadout,
        RegionReadout, SelectedObject, SkyReadout, SpeakerKind, StatusReadout, TeleportReadout,
        TeleportState, TranscriptLine, WaterReadout, WorldMapReadout,
    };

    /// Serializes `value`, reads it back and checks nothing was lost.
    fn round_trip<T>(value: &T) -> Result<String, serde_json::Error>
    where
        T: Serialize + DeserializeOwned + PartialEq + core::fmt::Debug,
    {
        let json = serde_json::to_string(value)?;
        let back: T = serde_json::from_str(&json)?;
        assert_eq!(&back, value, "{json}");
        Ok(json)
    }

    #[test]
    fn conversations_round_trip() -> Result<(), serde_json::Error> {
        let nearby = ConversationReadout {
            conversation: ConversationRef::Nearby,
            unread: 2,
            pending_invite: false,
            lines: vec![
                TranscriptLine {
                    own: false,
                    speaker: "Door".to_owned(),
                    speaker_id: Some(uuid::Uuid::from_u128(7)),
                    speaker_kind: SpeakerKind::Object,
                    chat_kind: Some(ChatKind::Other(42)),
                    text: "Locked".to_owned(),
                },
                TranscriptLine {
                    own: false,
                    speaker: "Second Life".to_owned(),
                    speaker_id: None,
                    speaker_kind: SpeakerKind::System,
                    chat_kind: Some(ChatKind::Region),
                    text: "Restart".to_owned(),
                },
            ],
        };
        round_trip(&nearby)?;
        for conversation in [
            ConversationRef::Direct(uuid::Uuid::from_u128(1)),
            ConversationRef::Group(uuid::Uuid::from_u128(2)),
            ConversationRef::Conference(uuid::Uuid::from_u128(3)),
        ] {
            round_trip(&ConversationReadout {
                conversation,
                unread: 0,
                pending_invite: true,
                lines: vec![TranscriptLine {
                    own: true,
                    speaker: "You".to_owned(),
                    speaker_id: None,
                    speaker_kind: SpeakerKind::Agent,
                    chat_kind: None,
                    text: "hi".to_owned(),
                }],
            })?;
        }
        assert_eq!(
            serde_json::to_string(&ConversationRef::Nearby)?,
            r#"{"kind":"nearby"}"#
        );
        Ok(())
    }

    #[test]
    fn notifications_and_status_round_trip() -> Result<(), serde_json::Error> {
        round_trip(&NotificationReadout {
            id: 3,
            template: "TeleportOffered".to_owned(),
            text: "Come here".to_owned(),
            buttons: vec![OfferedButton {
                name: "Teleport".to_owned(),
                label: "Teleport".to_owned(),
                default: true,
            }],
            live: false,
            response: Some("Teleport".to_owned()),
        })?;
        round_trip(&StatusReadout {
            region: Some("East".to_owned()),
            parcel: None,
            balance: Some(-5),
            time: ClockTime {
                hour: 23,
                minute: 59,
            },
        })?;
        Ok(())
    }

    #[test]
    fn agent_round_trips_in_every_teleport_state() -> Result<(), serde_json::Error> {
        for state in [
            TeleportState::Idle,
            TeleportState::Requested,
            TeleportState::InProgress,
            TeleportState::Arriving,
            TeleportState::Succeeded,
            TeleportState::Failed {
                reason: "no".to_owned(),
                detail: Some("blocked".to_owned()),
            },
        ] {
            round_trip(&AgentReadout {
                agent_id: Some(uuid::Uuid::from_u128(1)),
                region: Some(RegionReadout {
                    name: Some("Default Region".to_owned()),
                    handle: (256_000_u64 << 32) | 256_000,
                    id: None,
                }),
                position: Some([128.0, 128.0, 25.5]),
                seated_on: None,
                teleport: Some(TeleportReadout {
                    state,
                    destination: None,
                    message: Some("Requesting".to_owned()),
                    stalled: true,
                }),
                camera: Some(CameraView::Mouselook),
                heading: Some(1.5),
                camera_eye: Some([120.0, 128.0, 27.0]),
                published_bakes: vec![uuid::Uuid::from_u128(0xBA4E)],
            })?;
        }
        let json = serde_json::to_string(&TeleportReadout {
            state: TeleportState::Idle,
            destination: None,
            message: None,
            stalled: false,
        })?;
        assert_eq!(json, r#"{"state":"idle"}"#);
        for view in [
            CameraView::ThirdPerson,
            CameraView::Mouselook,
            CameraView::Flycam,
        ] {
            round_trip(&view)?;
        }
        Ok(())
    }

    #[test]
    fn environment_round_trips() -> Result<(), serde_json::Error> {
        let json = round_trip(&EnvironmentReadout {
            sky: Some(SkyReadout {
                name: "Midday".to_owned(),
                ambient: [1.0, 0.0, 0.0],
                haze_density: 0.5,
                sun: [0.0, 1.5],
            }),
            local_sky: true,
            transition: Some(0.25),
            water: Some(WaterReadout {
                name: "Default".to_owned(),
                fog_density: 4.0,
            }),
            previewing: vec!["settings-editor-sky".to_owned()],
        })?;
        assert_eq!(
            json,
            r#"{"sky":{"name":"Midday","ambient":[1.0,0.0,0.0],"haze_density":0.5,"sun":[0.0,1.5]},"local_sky":true,"transition":0.25,"water":{"name":"Default","fog_density":4.0},"previewing":["settings-editor-sky"]}"#
        );
        let none = round_trip(&EnvironmentReadout {
            sky: None,
            local_sky: false,
            transition: None,
            water: None,
            previewing: Vec::new(),
        })?;
        assert_eq!(none, r#"{"local_sky":false}"#);
        Ok(())
    }

    #[test]
    fn world_map_round_trips() -> Result<(), serde_json::Error> {
        let json = round_trip(&WorldMapReadout {
            regions: vec![MapRegionReadout {
                name: "Home".to_owned(),
                grid: [1000, 1000],
            }],
            items: vec![MapItemLayerReadout {
                grid: [1000, 1000],
                kind: 6,
                items: vec![MapItemReadout {
                    at: [1.0, 1.0],
                    extra: 0,
                    extra2: 0,
                    drawn: false,
                }],
            }],
            tile_server: Some("http://127.0.0.1:9000/".to_owned()),
            tiles_ready: 4,
            tiles_pending: 1,
            tiles_absent: 2,
        })?;
        assert_eq!(
            json,
            r#"{"regions":[{"name":"Home","grid":[1000,1000]}],"items":[{"grid":[1000,1000],"kind":6,"items":[{"at":[1.0,1.0],"extra":0,"extra2":0,"drawn":false}]}],"tile_server":"http://127.0.0.1:9000/","tiles_ready":4,"tiles_pending":1,"tiles_absent":2}"#
        );
        let nothing = round_trip(&WorldMapReadout::default())?;
        assert_eq!(
            nothing,
            r#"{"tiles_ready":0,"tiles_pending":0,"tiles_absent":0}"#
        );
        Ok(())
    }

    #[test]
    fn selection_and_inventory_round_trip() -> Result<(), serde_json::Error> {
        round_trip(&vec![SelectedObject {
            full_id: uuid::Uuid::from_u128(9),
            local_id: 4,
            primary: true,
        }])?;
        round_trip(&InventoryFolderReadout {
            id: uuid::Uuid::from_u128(1),
            name: "Objects".to_owned(),
            loaded: true,
            folders: Vec::new(),
            items: vec![InventoryEntry {
                id: uuid::Uuid::from_u128(2),
                name: "Box".to_owned(),
                kind: "object".to_owned(),
            }],
        })?;
        Ok(())
    }

    #[test]
    fn logs_round_trip_and_omit_a_zero_drop() -> Result<(), serde_json::Error> {
        let page = LogPage {
            entries: vec![LogEntry {
                seq: 4,
                stream: LogStream::UiAction,
                kind: "toolbar.inventory".to_owned(),
                detail: "UiAction".to_owned(),
            }],
            next: 5,
            dropped: 0,
        };
        let json = round_trip(&page)?;
        assert!(!json.contains("dropped"), "{json}");
        round_trip(&DiagnosticsReadout {
            warnings: 3,
            errors: 1,
            lines: LogPage {
                entries: vec![DiagnosticLine {
                    seq: 0,
                    level: LogLevel::Error,
                    target: "sl_client_bevy".to_owned(),
                    message: "boom code=3".to_owned(),
                }],
                next: 1,
                dropped: 2,
            },
        })?;
        for stream in [
            LogStream::Event,
            LogStream::Command,
            LogStream::UiAction,
            LogStream::Sound,
        ] {
            let json = round_trip(&stream)?;
            assert_eq!(json, format!("\"{stream}\""), "display matches serde");
        }
        Ok(())
    }

    #[test]
    fn quiet_needs_the_region_and_nothing_outstanding() {
        let mut readout = QuiescenceReadout::default();
        assert!(!readout.is_quiet(), "no region is not quiet");
        readout.region_up = true;
        assert!(readout.is_quiet(), "a viewer with nothing to wait for");
        readout.outstanding = Some(1);
        assert!(!readout.is_quiet());
        readout.outstanding = Some(0);
        readout.waiting_pipelines = Some(2);
        assert!(!readout.is_quiet());
        readout.waiting_pipelines = Some(0);
        assert!(readout.is_quiet());
    }

    /// The breakdown travels with the total, and an empty one is left out of
    /// the JSON.
    #[test]
    fn the_outstanding_breakdown_round_trips() -> Result<(), serde_json::Error> {
        let quiet = QuiescenceReadout {
            region_up: true,
            ..QuiescenceReadout::default()
        };
        assert_eq!(serde_json::to_string(&quiet)?, r#"{"region_up":true}"#);
        let busy = QuiescenceReadout {
            region_up: true,
            outstanding: Some(3),
            outstanding_by: [("textures.downloading".to_owned(), 3)].into(),
            waiting_pipelines: Some(0),
        };
        let json = serde_json::to_string(&busy)?;
        assert!(
            json.contains(r#""outstanding_by":{"textures.downloading":3}"#),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<QuiescenceReadout>(&json)?, busy);
        Ok(())
    }
}
