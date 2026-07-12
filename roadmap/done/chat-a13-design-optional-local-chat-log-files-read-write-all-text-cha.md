---
id: chat-a13
title: Design optional local chat-log files (read + write, all text chat)
topic: chat
status: done
origin: CHAT_ROADMAP.md
---

Context: [context/chat.md](../context/chat.md).

**A13. Design optional local chat-log files (read + write, all text
chat).** A **runtime** feature (the sans-IO `Session` does no file I/O — this
lives in `sl-client-tokio` / `sl-client-bevy`, fed by the event stream) that
optionally persists message history to per-conversation log files and reads it
back, for **long-term** scrollback beyond the in-memory A8 cap, **similar to
the Firestorm viewer** and ideally **format-compatible** with it. **Covers all
text-chat types** (user-set scope): **nearby / local chat** (`ChatReceived` —
otherwise out of the session-state scope, but **in** scope for logging), 1:1
IM, group, and conference. Design, grounded in Firestorm `LLLogChat`
(`lllogchat.cpp`): a per-account `chat_logs/` directory; per-conversation
transcript filenames (`chat.txt` for nearby; `firstname.lastname.txt` for 1:1,
with a legacy display-name option; `<group name> (group).txt` for group; a
participant-hash name for ad-hoc / conference — sanitised, optional date
suffix); the line format `[YYYY/MM/DD HH:MM]  Name: message` (timestamp / date
/ seconds toggles; space-prefixed continuation lines); **read-back the tail**
(Firestorm recalls the last ~20 KB / a "history lines" count) to **seed the A8
in-memory `history`** on session open; plus the optional `conversation.log`
metadata index of past conversations. Decide the config surface (enable per
text-chat type, log dir, filename scheme, timestamp format, recall size),
default **off** (opt-in, as Firestorm defaults nearby logging off), and how
the runtime supplies **wall-clock** time (the sans-IO core lacks it — so file
lines get real dates even for our own sends, A8's `timestamp = None`). Note
the boundary: A8 is the in-memory working set, A13 is the long-term file store
that A13 spills to and seeds from.
**Done — see § Local chat-log files reference (from A13) + task B9 in
§ Phase B.** Decided: A13 is a **runtime** feature (sl-client-tokio /
sl-client-bevy / REPL at parity) — the sans-IO `Session` does **no** file I/O.
It taps the **event stream** (+ our own outbound commands) and writes / reads
per-conversation files; default **off**. Logs **all** text-chat types from
their events: nearby `Event::ChatReceived` (`event.rs:374` — out of
session-state scope, logged) → `chat.txt`; 1:1 `InstantMessageReceived`
→ `<account>.txt` (legacy `firstname.lastname.txt` option); group
`GroupSessionMessage` → `<group> (group).txt`; conference
`ConferenceSessionMessage` → `Ad-hoc Conference hash<md5-of-sorted-ids>.txt` —
Firestorm `cleanFileName` sanitisation (``"'\/?*:.<>|[]{}~`` → `_`), optional
date suffix (`-%Y-%m-%d` nearby / `-%Y-%m` IM·group; never adhoc). Line format
Firestorm-style — `[YYYY/MM/DD HH:MM:SS]··Name: message` (two spaces after
`]`; **seconds default ON** — user-set 2026-06-27, vs Firestorm's optional
default-off `FSSecondsinChatTimestamps`, still format-compatible via its
`DATE_FORMAT_SEC = "%Y/%m/%d %H:%M:%S"`), toggles timestamp / date / seconds /
24h, multi-line continuation lines prefixed `\n␠`, colon-in-name → `%3A`,
system speaker `Second Life:`.
**Read-back reconciled with A10 (the key correction):** Firestorm seeds the
in-memory ring from a ~20 KB tail read, but A10's `QueryChatHistoryPage` makes
the file the **deep archive**, so **B9 serves the *older* pages of
`QueryChatHistoryPage` directly from the file** (`seek` to the tail / `mmap`,
parse the window) — **no** sans-IO "seed history" command; the Session ring
keeps only this-session live messages and on a fresh login *all* scrollback
comes from the file via paging. Firestorm's `LOG_RECALL_SIZE = 20480` is the
default seek/page window; a failed line-parse falls back to a plain-text
message (Firestorm behaviour). Optional `conversation.log` metadata index
(`[unix] type · · offline name| participant_id session_id history_file|`) for
conversation discovery, with retention-days purge. **Config** `ChatLogConfig`
(runtime, default off): enable-per-type (nearby default off), log dir,
filename scheme (modern / legacy IM names, date-suffix), timestamp format
(timestamp / date / seconds / 24h), recall window; the per-account dir + names
come from the runtime's `login_account` (`methods.rs:6959`) / the events'
`from_name`. **Wall-clock:** A13 is runtime, so it has `SystemTime::now()` —
file lines get real **local** dates even for our own sends (A8's in-memory
`timestamp = None`); inbound prefers the wire `timestamp` (Unix; the original
time for an offline IM) else receipt-now. **Naming correction:**
A8/B4's history-entry type `ChatMessage` **collides** with the existing
nearby-chat `ChatMessage` (`types/chat.rs:254`), so it is **renamed
`SessionMessage`** — threaded through B4 (the entry), A10/B7
(`ChatHistoryPage { messages: Arc<[SessionMessage]> }`) and B9. **Boundary:**
A8 = in-memory working set (this session, the 256-cap `history`); A13 =
long-term on-disk store (all history) and the **only** file I/O — it spills
A8's messages to disk and serves the archive back through A10's paging. Local
file I/O is **grid-agnostic**, so A13 is testable on **any** grid (unlike A5 /
A12 voice). **This closes A11's open "local chat-log file cases" question and
is the LAST Phase A item — with it Phase A (A1–A13) is complete and signs
off; Phase B implementation may begin (ask the user first).**

Phase A scopes the planning only; the implementation tasks each Phase A item
produces are appended to **Phase B** below as that item is worked, tagged with
the producing item. Phase B is a *draft* until Phase A is signed off; tick a box
only when the step builds, is clippy-clean (restriction lints), and `cargo test`
passes. Keep `sl-client-tokio`, `sl-client-bevy`, and the REPL at feature
parity; never push client-only types into shared `sl-types`.

## Local chat-log files reference (from A13)

The optional, default-off **runtime** chat-log file feature: write every
text-chat line to a per-conversation transcript and read it back for long-term
scrollback. It lives entirely in the runtimes (`sl-client-tokio` /
`sl-client-bevy` / the REPL) — the sans-IO `Session` does **no** file I/O — and
is fed by the **event stream** plus our own outbound commands. Grounded in
Firestorm `LLLogChat` (`lllogchat.cpp`) and `LLConversationLog`
(`llconversationlog.cpp`), and **format-compatible** with them so the files
interleave with a Firestorm install.

**What is logged, and to which file.** All four text-chat types, each from its
event (and our matching outbound command for our own lines):

| Type | Event | File name |
|------|-------|-----------|
| nearby / local | `Event::ChatReceived` (`event.rs:374`) | `chat.txt` |
| 1:1 IM | `Event::InstantMessageReceived` (dialog `Message`) | `<account>.txt` (legacy `firstname.lastname.txt` option) |
| group | `Event::GroupSessionMessage` | `<group name> (group).txt` |
| conference | `Event::ConferenceSessionMessage` | `Ad-hoc Conference hash<md5-of-sorted-participant-ids>.txt` |

- Nearby chat is **out of the session-state scope** (no `ChatSession` is opened
  for it — A1) but **in** scope for logging; A13 is the *only* place nearby chat
  is persisted.
- Names are sanitised with Firestorm `cleanFileName` (every char in
  ``"'\/?*:.<>|[]{}~`` → `_`). Files live in a per-account `chat_logs/`
  directory. Optional **date suffix** (`LogFileNamewithDate`): `-%Y-%m-%d` for
  nearby, `-%Y-%m` (monthly) for IM / group; **never** for ad-hoc.
- The per-account directory + the 1:1 / group names come from the runtime's
  `login_account` (`methods.rs:6959`) and the events' `from_name` /
  `GroupSessionMessage.group_id`; the conference hash is the MD5 of the sorted
  participant ids (A6 roster).

**The line format** (Firestorm `LLChatLogFormatter`, `lllogchat.cpp:1041`):

    [YYYY/MM/DD HH:MM:SS]  Name: message

- **Seconds are ON by default** (user-set, 2026-06-27) — Firestorm's
  `DATE_FORMAT_SEC = "%Y/%m/%d %H:%M:%S"`, which it gates behind the optional
  default-*off* `FSSecondsinChatTimestamps`; A13 flips that default to **on** so
  log lines carry `HH:MM:SS`. Byte-compatible: Firestorm's parser reads the
  seconds variant (`TIMESTAMP_AND_STUFF_SEC`, `:92`).
- **Two spaces** separate the `]` from the name (Firestorm `IM_SEPARATOR`
  context). Multi-line messages: each embedded newline is written as `\n␠` (a
  newline + one leading space), so continuation lines are space-prefixed and the
  parser re-joins them. A literal colon in a name is URI-encoded `%3A`. A system
  message with no sender writes the name `Second Life:`.
- Toggles (config below): timestamp on/off (`LogTimestamp`), the date component
  (`LogTimestampDate` → `[HH:MM:SS]` time-only for today), seconds
  (default on, may be turned off), 24-hour vs 12-hour AM/PM.

**Read-back — reconciled with A10's paging (the load-bearing correction).**
Firestorm reads the last `LOG_RECALL_SIZE = 20480` bytes on open and *seeds the
in-memory buffer*. A13 was originally written the same way ("seed the A8
in-memory `history`"), but **A10's `QueryChatHistoryPage` supersedes that**: the
file **is** the deep archive, so —

- the sans-IO `Session` ring keeps only **this-session live** messages (the A8
  256-cap tail); it is **not** seeded from the file, and there is **no** new
  "load history" command;
- **B9 serves the *older* pages of `QueryChatHistoryPage` from file** — when
  A10's `prev` cursor points past the in-memory tail, the runtime reads the file
  (`seek` to the window / `mmap`), parses the lines back into `SessionMessage`s,
  and returns the page. `LOG_RECALL_SIZE` (20 KB) is the default seek/page
  window;
- on a **fresh login** the ring is empty, so *all* scrollback for an opened
  conversation comes from the file via paging — which is exactly the Firestorm
  "recall on open" behaviour, expressed through A10's pull API.

A stored line is parsed into `(timestamp, name, message)`; a line that fails
the format regex is kept as a **plain-text** `SessionMessage` (Firestorm
fallback), and space-prefixed continuation lines fold into the prior message.

**Optional `conversation.log` index** (`llconversationlog.cpp`). A per-account
metadata index of past conversations, default off, format-compatible:

    [<unix>] <type> <reserved> <offline> <name>| <pid> <sid> <file>|

(`type` 0=P2P / 1=group / 2=adhoc; `reserved` always 0; `offline` = has-unread).
Used for conversation discovery without scanning transcripts; entries older than
a retention (Firestorm `FSConversationLogLifetime`, default 30 days)
are purged on load.

**Config — `ChatLogConfig` (runtime, default OFF).** Opt-in, mirroring
Firestorm's per-account toggles:

- **enable per text-chat type** — nearby / IM / group / conference independently
  (nearby **default off**, as Firestorm; the `KeepConversationLogTranscripts`
  tri-state is the precedent);
- **log dir** (default the per-account `chat_logs/`);
- **filename scheme** — modern `<account>` vs legacy `firstname.lastname`
  (`UseLegacyIMLogNames`), the date-suffix toggle;
- **timestamp format** — timestamp on/off, date on/off, **seconds on/off
  (default ON)**, 24h/12h;
- **recall window** (default `LOG_RECALL_SIZE` 20480 bytes) — the page size B9
  reads from the file;
- the optional `conversation.log` index on/off + its retention days.

**Wall-clock — the runtime supplies it.** The sans-IO core has no clock (A8's
`SessionMessage.timestamp` is `None` for our own sends, the wire `timestamp` for
inbound). A13 is runtime, so it stamps lines with `SystemTime::now()` in **local
time** — file lines get real dates even for our own sends. For an inbound
message it prefers wire `InstantMessage.timestamp` (Unix; the *original* time
for an offline IM that may be replayed long after it was sent) and falls back to
receipt-now.

**Naming correction (cross-cutting, owned here).** A8/B4's planned history-entry
type `ChatMessage` **collides** with the pre-existing nearby-chat `ChatMessage`
(`types/chat.rs:254`, a different struct: `from_name` / `source` / `chat_type` /
…). A13 renames the A8 entry **`SessionMessage`**; it threads through B4 (the
field type), A10 / B7 (`ChatHistoryPage { messages: Arc<[SessionMessage]> }`,
the snapshot builders) and B9 (the parse target). Cross-references added at the
A8 and A10 reference sections.

**Boundary & testability.** A8 is the in-memory working set (this session, the
256-cap ring); A13 is the long-term on-disk store (all history) and the **only**
file I/O in the system — it spills A8's messages to disk on arrival and serves
the archive back through A10's paging. Because it is purely local file I/O, A13
is **grid-agnostic and testable on any grid** (write a message, assert the file
line; read it back, assert the `SessionMessage`) — unlike the SL-only voice
paths (A5 / A12). **This closes A11's "local chat-log file cases" question.**
