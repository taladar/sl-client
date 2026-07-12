---
id: chat-a12
title: Design the per-session voice-channel state (signalling only)
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A12. Design the per-session voice-channel state (signalling only).** A
chat session (group / conference / 1:1) can carry a **voice** channel beside
its text channel (user-set scope). Design the SL-**signalling** state the
`ChatSession` tracks for voice: whether the session *has* voice, the
`voice_channel_info` (channel uri / credentials handed over on the invite or
the `"accept invitation"` reply / `ParcelVoiceInfoRequest`), whether we have
**joined** the voice channel at the signalling level (driven by an A5 voice
accept), and the voice **membership** (who is in the voice channel, read from
the SL session roster / agent-list updates — not audio). Reuse the existing
voice-signalling feature (`Event::VoiceAccountProvisioned`,
`Event::ParcelVoiceInfo`, `Command::RequestVoiceAccount` /
`RequestParcelVoiceInfo` / `SendVoiceSignaling`). Add join/leave-voice
commands at the signalling level and the accessors. **Explicitly OUT of scope
(user-set):** the Vivox / WebRTC audio transport itself and the
"who-is-currently-speaking" / talk-activity indicators it drives — those live
in the external voice client, not sl-client. State the boundary: sl-client
models voice **session state**, not voice **audio**.
**Done — see § Per-session voice-channel reference (from A12) + task B8 in
§ Phase B.** voice is a per-`ChatSession` **facet** (the A2/A5-reserved
"voice-channel state (A12)" slot), **additive** — a `voice: VoiceChannelState`
field, **not** a separate session. It is **distinct** from the two voice
surfaces that already exist and **stay standalone**: the **agent-global voice
*account*** (`VoiceAccountInfo`, provisioned once per login via
`Command::RequestVoiceAccount` → `Event::VoiceAccountProvisioned`,
`methods.rs:495`) — the credentials to the voice *server* — and the
**spatial / parcel channel** (`ParcelVoiceInfo` via
`Command::RequestParcelVoiceInfo`, `methods.rs:500`) — nearby/local voice,
which is **not** a chat session (local chat out of scope). A12 adds **only**
the per-session *room* state for group / conference / 1:1. Shapes:
`VoiceChannelState { has_voice: bool, channel: Option<VoiceChannelInfo>,
joined: bool, members: BTreeSet<AgentKey> }` and
`VoiceChannelInfo { channel_uri: Option<Url>, channel_credentials:
Option<String>, voice_server_type: Option<String>, session_handle:
Option<String> }` (mirrors the SL session `voice_channel_info` / the existing
`ParcelVoiceInfo`'s nested `voice_credentials`, `sl-wire voice.rs:494`).
**Sources:** `has_voice` / `channel` from A5 voice **invite body** plus the
`ChatSessionRequest "accept invitation"` reply's `voice_channel_info` (decoder
gaps — B5 classifies `InviteChannel`, B8 decodes the channel); `joined` set
**optimistically** by A5 voice-accept / a new `JoinSessionVoice`, cleared by
`LeaveSessionVoice` / a voice decline (signalling only — no audio ack);
`members` folded from `ChatterBoxSessionAgentListUpdates` agent-list
**voice-connected** flag — **NOT** the speaking flag (out of scope). **New
commands** (full parity): `JoinSessionVoice { session }` /
`LeaveSessionVoice { session }` — sans-IO records `voice.joined`; **runtime**
orchestrates the existing voice plumbing (ensure the account via
`RequestVoiceAccount`, then session voice request via `ChatSessionRequest`;
leave = `"decline invitation"` / `"decline p2p voice"` per A5, or the WebRTC
logout `RequestVoiceAccount{logout}` / `SendVoiceSignaling`). **Accessors**
`session_has_voice` / `session_voice_channel` / `session_voice_joined` /
`session_voice_members`; the A10 `ChatSessionInfo` gains the reserved voice
fields (`has_voice` / `voice_joined` / `voice_members`). Voice covers **all
three kinds** (group / conference multi-agent **and** the 1:1 P2P voice call —
A5's `"decline p2p voice"`), which **closes A11's open "voice-channel cases"**
question. **OUT (user-set):** the Vivox / WebRTC audio transport and the
"who-is-speaking" / talk-activity indicators (the SL-signalling-only scope);
the boundary is voice **session state**, not voice **audio**. **OpenSim
limitation:** `ChatSessionRequest` is stubbed and voice runs via
FreeSwitch / Vivox, so per-session voice is **SL-only testable** (aditi) — the
same constraint as A5's voice path.

## Per-session voice-channel reference (from A12)

The per-`ChatSession` **voice** facet, at the SL **signalling** level only. A
group / conference / 1:1 session can carry a voice channel beside its text
channel (2026-06-27 scope expansion); A12 tracks *that the session has voice,
the channel coordinates, whether we have joined, and who is in it* — **never**
the audio stream nor who is currently speaking. It **reuses** the existing
voice-signalling feature wholesale and adds only per-session state on top.

**What already exists vs. what A12 adds.** Three voice surfaces, kept separate:

| Surface | Scope | Existing? | Owner |
|---------|-------|-----------|-------|
| Voice **account** (`VoiceAccountInfo`, `RequestVoiceAccount` → `VoiceAccountProvisioned`, `methods.rs:495`) | agent-global credentials to the voice *server* (Vivox SIP / WebRTC JSEP) | yes (`sl-wire voice.rs:329`) | unchanged |
| **Parcel / spatial** channel (`ParcelVoiceInfo`, `RequestParcelVoiceInfo` → `ParcelVoiceInfo`, `methods.rs:500`) | nearby / local voice (**not** a chat session) | yes (`sl-wire voice.rs:494`) | unchanged |
| **Per-session** channel (`VoiceChannelState` on `ChatSession`) | the group / conference / 1:1 *room's* voice | **no — A12 adds it** | A12 / B8 |

The account is provisioned once per login; the per-session join *uses* that
account to connect to a session's channel. A12 does **not** re-provision
— it records the per-session room state and triggers the existing plumbing.

**The state (additive on `ChatSession`, the A2/A5-reserved slot):**

    struct VoiceChannelState {
        has_voice: bool,                   // session offers a voice channel
        channel: Option<VoiceChannelInfo>, // coordinates (uri / creds)
        joined: bool,                      // we joined at the SIGNALLING level
        members: BTreeSet<AgentKey>,       // who is in voice, not speaking
    }

    struct VoiceChannelInfo {
        channel_uri: Option<url::Url>,         // sip:… / the session voice room
        channel_credentials: Option<String>,  // per-channel credentials
        voice_server_type: Option<String>,    // "vivox" | "webrtc"
        session_handle: Option<String>,        // the SL voice session handle
    }

- `VoiceChannelInfo` mirrors the SL session `voice_channel_info` block and the
  existing `ParcelVoiceInfo`'s nested `voice_credentials` (`channel_uri` /
  `channel_credentials`) — a small **client-local** struct in sl-proto, not a
  reuse of `ParcelVoiceInfo` (whose `parcel_local_id` / `region_name` are
  parcel-only). `Default`-able (all `Option` / `false` / empty set), so
  `ChatSession::new` initialises an empty, no-voice facet.
- `members` is the **voice-connected** subset of the text roster (A6) — strictly
  a membership set, **never** the talk-activity / speaking state.

**Where each field is fed:**

| Field | Source | Decoder |
|-------|--------|---------|
| `has_voice` / `channel` | the A5 voice **invite body** (`ChatterBoxInvitation` `voice` body) and the `ChatSessionRequest "accept invitation"` reply `voice_channel_info` | **gaps:** B5 classifies `InviteChannel`; **B8 decodes the channel** (the invitation decoder ignores `voice` today — `conversions.rs:2521`) |
| `joined` | **optimistic**: set by the A5 voice-accept or a new `JoinSessionVoice`; cleared by `LeaveSessionVoice` / a voice decline | sans-IO state only (no audio ack) |
| `members` | the modern `ChatterBoxSessionAgentListUpdates` agent-list **voice** flag (the voice-connected subset) | **gap:** B8 decodes the agent-list voice flag — **not** the `is_now_speaking` flag (out of scope) |

**New commands (signalling-level; full six-site parity):**

    JoinSessionVoice  { session: ChatSessionKind }
    LeaveSessionVoice { session: ChatSessionKind }

- The **sans-IO `Session`** only records the per-session `voice.joined`
  transition (optimistic, like A4's text `Joined`) and exposes the accessors.
- The **runtime** orchestrates the existing voice plumbing: on join, ensure a
  voice account (`RequestVoiceAccount`, once) then signal into the session's
  channel via `ChatSessionRequest` (the same cap A5 uses, with the *voice*
  methods); on leave, `"decline invitation"` (multi-agent) / `"decline p2p
  voice"` (1:1 P2P) per A5, or the WebRTC teardown
  (`RequestVoiceAccount{logout}` / `SendVoiceSignaling{completed}`). No new HTTP
  helper — `post_voice_cap` / `post_chat_session_request` (B5) cover it.

**Accessors** (read model; fold into the A10 `ChatSessionInfo` view):

    fn session_has_voice(&self, session: ChatSessionKind) -> bool
    fn session_voice_channel(&self, session: ChatSessionKind)
        -> Option<&VoiceChannelInfo>
    fn session_voice_joined(&self, session: ChatSessionKind) -> bool
    fn session_voice_members(&self, session: ChatSessionKind)
        -> impl Iterator<Item = AgentKey> + '_

`ChatSessionInfo` (A10) gains the reserved voice fields `has_voice: bool`,
`voice_joined: bool`, `voice_members: Vec<AgentKey>` (and the channel info if a
driver wants it), so a UI can show a voice indicator + roster without a separate
query. This is the A10-noted "A12 appends voice fields to `ChatSessionInfo`".

**All three kinds carry voice.** Group / conference are multi-agent voice; a 1:1
is a **P2P voice call** (A5's `"decline p2p voice"`), so a `Direct` session's
`VoiceChannelState` is valid, its `members` implicitly `{ self, peer }`. This
**closes the A11 open question** "the voice-channel cases of A12".

**Boundary (user-set, restated).** sl-client models voice **session state** —
has-voice, channel coordinates, joined-at-signalling, membership — and **nothing
audio**: Vivox / WebRTC media transport and the "who-is-currently-speaking" /
talk-activity indicators live in **external voice client**. The crate's voice
feature is signalling only (the standing project rule).

**Persistence.** `VoiceChannelState` lives on `ChatSession`, so it follows
the A9 rule: it **persists** across teleport / crossing and clears only
on logout (with the rest of the session). A7's reset also drops an
offlined friend from `voice.members` (the same fan-out as `participants` /
`typing`), idempotent with the agent-list updates.

**OpenSim limitation.** `ChatSessionRequest` is stubbed in both OpenSim voice
modules and voice runs through its own FreeSwitch / Vivox path, so the
per-session voice flow is **SL-only testable** (live-aditi) — the identical
constraint A5 noted for the modern accept/decline. The local OpenSim grid
exercises only the text channel.
