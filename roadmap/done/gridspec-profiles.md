---
id: gridspec-profiles
title: Profiles, picks, classifieds, notes and display names on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-display-name-set,
  viewer-mute-entry-name-is-not-the-username]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim ignores the picks / classifieds / notes `GenericMessage` queries,
volunteers replies after edits, cannot read notes back, breaks classified
delete; display names: SL-only pushes and set cap. None of the profile cases
ran on aditi.

## Discover

The profile cases on aditi; display-name reads (set only with the user's consent
— rate-limited for days); profile web URL caps.

## Document

`book/src/gridspec/profiles.md`.

## Fake grid

Display names small in this task; profiles large —
[[protocol-sim-profile-messages]] then [[server-fake-grid-profiles]].

## Viewer

Picks from volunteered replies on OpenSim; blank notes; display-name UI hidden
without the cap.

## Capabilities done in this task

[[protocol-cap-agent-profile]]: profiles over `AgentProfile`. Also the
protocol halves of `UploadAgentProfileImage`
([[viewer-profile-image-editing]]), `SetDisplayName`
([[viewer-display-name-set]]) and `SearchStatRequest`
([[viewer-classified-click-stats]]): the command, the reply, the fake grid's
Second Life flavour serving it; the UI stays in those tasks.

## Done (2026-10-15)

**Discover**: two censuses, two runs per live grid each, then a verification
pass with the tables in place. `profiles` (`sl-conformance/src/cases/
profiles.rs`, 2av, live grids): reads of another's and one's own profile
over UDP and over `AgentProfile`; edits over UDP and over the capability
(one key, two keys, `hide_age`, a 1000-byte text); interests; notes over
both roads; picks and classifieds (a L$0 and a L$50 listing, click counts,
edits, deletes); the profile image over `UploadAgentProfileImage`; and the
same through the client's own commands. Every "UDP" leg sends its message
raw, so it measures the UDP road where the client takes the capability.
`display-names` (rewritten as a 2av census, all four grids): the raw
`GetDisplayNames` answer for A, B and an unknown id, what the client reads,
and whether `SetDisplayName` is granted; names described by Second Life's
rules (the SLID, the default display name) so the test accounts' names do
not leak into the measurement.

The big finding: **neither grid answered the picks, notes and classifieds
generic queries because our `GenericMessage` left the method and parameters
un-terminated**; the reference NUL-terminates them (`addString`). Fixed in
`send_generic_message`; both grids now answer every one. The "OpenSim
ignores these queries" note above was this.

**Document**: `book/src/gridspec/profiles.md`, linked from
`content/profiles.md`, `comms/caps-reference.md` and
`gridspec/capabilities.md`.

**Fake grid**: display names here — `DisplayNameStyle` per flavour (Second
Life: a Resident's first name as the default display name, no expiry,
unknown ids under `bad_ids`; OpenSim: the legacy name as the user name,
unknown ids left out), held by `display-names` on both flavours. Second
Life's flavour now serves `SetDisplayName` (from the reference, see below:
`409` stale, `400` empty or over 31 characters, else `200` and a
`DisplayNameUpdate` to everybody in the region), `SearchStatRequest`
(counts at 0, as measured) and `UploadAgentProfileImage` (the image stored).
Profiles themselves are [[protocol-sim-profile-messages]] (now ready) then
[[server-fake-grid-profiles]], which also serves `AgentProfile`.

**Viewer**: the profile floater's picks and notes now fill on both grids
(the NUL fix), and on Second Life come over `AgentProfile` — a second read
of a profile already being read is not sent, so the floater's four requests
cost one GET. There is no display-name UI yet; hiding it without the
capability is a note on [[viewer-display-name-set]]. No Interests tab
exists, so the capability's lack of interests shows nowhere.

**Capabilities**: [[protocol-cap-agent-profile]] done here; the protocol
halves of `UploadAgentProfileImage` (`Command::UploadProfileImage`,
`Event::ProfileImageUploaded`), `SetDisplayName` (`Command::SetDisplayName`)
and `SearchStatRequest` (`Command::RequestClassifiedClickStats`,
`Event::ClassifiedClickStats`, also from Second Life's
`classifiedclickthrough`) done in both runtimes and the REPL; their UI stays
in [[viewer-profile-image-editing]], [[viewer-display-name-set]] and
[[viewer-classified-click-stats]].

### Differences from the plan

- `SetDisplayName` was **not measured**, by the user's decision (a change
  locks the name for a week): only that Second Life grants it. The fake
  grid's answers follow the reference.
- `AgentProfile` is not served by the fake grid's Second Life flavour: it
  needs the profile store [[server-fake-grid-profiles]] builds.
- Not measured: a profile of an avatar with a partner, images or a
  `caption`; a group shown in the profile over the capability (Second Life
  answered `groups` empty for both test avatars, though UDP lists theirs);
  a classified's renewal and expiry; a pick's top-pick and sort order.
