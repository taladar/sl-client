---
id: chat-a5
title: Design invitation handling + accept/decline
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A5. Design invitation handling + accept/decline.** A pending-invitations
registry fed by `Event::ConferenceInvited` (and group invites), plus new
accept/decline commands. Decide the path: adopt the modern
`ChatSessionRequest` capability (its accept-invitation method; not implemented
today) versus the UDP implicit-join. Output: the invitation lifecycle and the
new command(s).
**Done — see § Invitation-handling reference (from A5) + B5 in § Phase B.**
**Scope (user-set): a chat session can carry both a TEXT and a VOICE channel
(a group/conference has both), so this roadmap handles *both* — invitations
come in text and voice flavours and A5 covers each.** Decisions: pending
invitations are the A4 **`Invited` entries** enriched to
`Invited(PendingInvite { inviter, session_name, channel: InviteChannel })`
where `InviteChannel { Text | Voice | Both }` (from the `ChatterBoxInvitation`
body — `instant_message` vs `voice`). Commands `AcceptChatInvite` /
`DeclineChatInvite { session_id, from_group }`. The modern path is the shared
**`ChatSessionRequest`** cap; **text and voice use *different* methods on it**
(the distinction that matters): join/leave **text** = `"accept invitation"` /
`"decline invitation"` (the `"accept invitation"` reply is the **participant
roster** → feeds A6); a **voice** accept additionally **starts the voice
channel** (the existing voice feature), a voice decline is `"decline
invitation"` for multi-agent or `"decline p2p voice"` for 1:1 — A5 uses the
text methods for the text channel and the voice methods for the voice channel,
never conflating them. **UDP fallback** (OpenSim stubs `ChatSessionRequest` in
its voice modules): text accept = optimistic `Joined` (sim already added us),
text decline = `SessionLeave`; OpenSim voice is its own FreeSwitch/Vivox path.
Sans-IO `Session` always does the registry transition (accept →
`Joined`; decline → remove). The per-session **voice-channel state**
(has-voice, `voice_channel_info`, joined-voice at *signalling* level, voice
membership from the SL roster) is the new **A12** (appended below); A5 only
feeds the invite→join-signalling trigger. **Out of scope (user-set):** the
Vivox/WebRTC audio transport itself and the "who is speaking" indicators it
drives — those live in the external voice client, not sl-client (whose voice
feature is SL *signalling* only). Note the **decoder gap**:
`chatterbox_invitation_from_llsd` does not yet read the `voice` body, so B5
must classify the invite's `InviteChannel`. (1:1 *text* has no invite; a 1:1
*voice* call is a P2P voice invite, in scope at the signalling level.)

## Invitation-handling reference (from A5)

How a chat-session invitation is tracked and accepted/declined. **Policy
(user-set): adopt the modern Second Life CAPS workflow wherever it exists, and
keep the UDP path only while even OpenSim still uses it.** For session invites
that means **both**: the modern `ChatSessionRequest` cap is the Second Life
path, and the UDP `ImprovedInstantMessage` path is the OpenSim path (OpenSim
**stubs** `ChatSessionRequest` — see below). The simulator stays authoritative;
the pending-invitations registry is a read model.

**Pending invitations = the A4 `Invited` entries** (no separate registry). A5
enriches A4's lifecycle enum so the `Invited` state carries the invite payload,
making the registry self-describing:

    enum ChatSessionLifecycle { Invited(PendingInvite), Joined }   // refines A4/B5

    struct PendingInvite {
        inviter: AgentKey,         // ConferenceInvited.from_agent_id
        session_name: String,      // ConferenceInvited.session_name
        channel: InviteChannel,    // which channel(s) we were invited to
    }

    enum InviteChannel { Text, Voice, Both }

- **`channel`** records whether the invitation is to the **text** channel, the
  **voice** channel, or both. The `ChatterBoxInvitation` body distinguishes them
  (Firestorm `llimview.cpp:5195`): an `instant_message` body is a *text* session
  invite (viewer auto-joins it), a `voice` body is a *voice-call* invite (the
  viewer prompts the user), an `immediate` body is an immediate IM. A group /
  conference can have **both** a text and a voice channel under one session id,
  so the two are tracked together, not as separate sessions.
- Fed by the existing `ChatterBoxInvitation` handler
  (`handle_caps_event`, `methods.rs:663` → `Event::ConferenceInvited`) and the
  UDP `SessionGroupStart` / `SessionConferenceStart` IM path: on an invitation,
  get-or-create the registry entry keyed by `from_group ? Group { group_id } :
  Conference { id }` and set `lifecycle = Invited(PendingInvite{…})`. The event
  is still emitted unchanged (the driver shows the invite and decides).
- The `Invited` payload is dropped when the entry promotes to `Joined` (accept,
  or any inbound traffic — the A4 promotion rule). So pending invitations are
  exactly `chat_sessions` entries whose `lifecycle` is `Invited(..)`, shown by
  the A10 `chat_sessions()` accessor — no second map.
- Only **group / conference** session invites exist (1:1 has none — you just
  message). `GroupInvitation` (dialog 3, a *join-the-group* offer) is a
  different feature and **out of scope** here.

**The two commands** (`command.rs`):

    AcceptChatInvite  { session_id: ImSessionId, from_group: bool }
    DeclineChatInvite { session_id: ImSessionId, from_group: bool }

`session_id` + `from_group` mirror the `ConferenceInvited` fields the driver is
answering (typed `ImSessionId` — a group session id is still an IM session id;
the `Group` key reinterprets it via `GroupKey::from(session_id.uuid())`).
The flat `session_id.uuid()` is exactly the `"session-id"` the CAPS body needs.

**Text vs voice methods on the shared cap (the distinction that matters).** The
one `ChatSessionRequest` cap carries *both* text-session and voice methods; A5
uses the **text** methods for the text channel and the **voice** methods for the
voice channel, never mixing them (Firestorm `llimview.cpp`):

| Action | Channel | `method` | Notes |
|--------|---------|----------|-------|
| join | text | `"accept invitation"` | reply body **is the participant roster** → seeds A6 (`:666`, `:721`) |
| leave/refuse | text | `"decline invitation"` | multi-agent decline (`:3437`) |
| join | voice | `"accept invitation"` **+ start voice channel** | same method, then the voice signalling join (A12 / the existing voice feature) (`:730`) |
| refuse | voice (multi-agent) | `"decline invitation"` | (`:3437`) |
| refuse | voice (1:1 / P2P) | `"decline p2p voice"` | P2P-only (`:3422`) |

The **`"accept invitation"` reply carries the session's current agent roster** —
A5 hands it to A6 as the initial participant list (the modern equivalent of the
UDP `SessionAdd` stream). A *voice* accept uses the **same** `"accept
invitation"` and then triggers the voice-channel join *signalling* (A12); the
actual audio is the external client (out of scope). A viewer **auto-accepts
text** invites and **prompts** for voice — sl-client surfaces both as the
`Invited` entry and leaves the accept/decline decision to the driver.

**Path selection lives in the runtime** (it owns the capability map and all CAPS
HTTP — the sans-IO `Session` cannot POST; mirrors `RequestOfflineMessages`):

- **`ChatSessionRequest` cap present (Second Life)** → POST
  `application/llsd+xml` `{ "method": <per table>, "session-id": <uuid> }` to
  the cap url, following the existing `post_voice_cap` / `post_caps_oneway`
  pattern (`sl-client-tokio` `http.rs`, `voice.rs`). A new constant
  `CAP_CHAT_SESSION_REQUEST = "ChatSessionRequest"`.
- **cap absent (OpenSim)** → **UDP fallback** (text channel): *accept* needs
  **no** wire — the sim added us when it routed the invite, so accepting is just
  the optimistic local `Invited`→`Joined`; *decline* sends a `SessionLeave`
  (`ImprovedInstantMessage`, the existing `leave_*`). OpenSim **voice** runs
  through its own FreeSwitch/Vivox modules, not `ChatSessionRequest`, so a voice
  invite is not exercised on the local grid.

**The sans-IO `Session` effect (always, regardless of path).** The registry
transition is pure state and lives in `Session`:

- `Session::accept_chat_invite(session_id, from_group, now)` → promote the entry
  to `Joined` (get-or-create as `Joined` if somehow absent).
- `Session::decline_chat_invite(session_id, from_group, now)` → **remove** the
  entry.

The runtime calls the `Session` method (registry) **and** does the transport:
the CAPS POST when the cap is present, otherwise the UDP `SessionLeave` for a
decline (accept has no UDP wire). So the registry stays correct on every grid;
only the *wire* differs by path. No new `Event` — accept/decline is a local
action the driver took; the session's joined-ness is later confirmed by inbound
traffic (A4's optimistic model).

**OpenSim test limitation (grounded).** `ChatSessionRequest` is **not**
implemented in OpenSim — both the FreeSwitch and Vivox voice modules have the
`caps.RegisterHandler("ChatSessionRequest", …)` line **commented out**, and the
stub handler just returns `<llsd>true</llsd>`
(`FreeSwitchVoiceModule.cs:296`, `VivoxVoiceModule.cs:434`). opensim-core has no
implementation at all. So the **modern accept/decline is Second-Life-only
testable** (live-aditi); the **UDP-fallback** accept/decline is what the local
OpenSim grid exercises. The implementation must therefore keep both paths real,
not treat UDP as a dead fallback.
