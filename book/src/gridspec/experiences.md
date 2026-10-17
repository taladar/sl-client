# Experiences

An experience is a Second Life grant a script asks an agent for once; its
records, the agent's preferences and the estate's lists live behind a family
of capabilities with no UDP path (*Content Layer*,
[Experiences](../content/experiences.md)). Stock OpenSim ships no experience
module.

Measured on 2026-10-16 by three `sl-repl` wire probes on aditi (Mauve, the
`primary` avatar), then the conformance cases `experience-info`,
`experience-permissions` and `experience-admin-contributor` on aditi, the
local OpenSim and both fake-grid flavours. None of our aditi avatars owns,
administers or contributes to an experience — creating one needs a Premium
account — so aditi's fixture is a public experience of a stranger's,
**Bugsly Testing** (`040979ee-…`, a resident's, with a group), and the
owner's side of every capability is unmeasured (below). **held** marks a row
a conformance case asserts on every run.

## The capabilities

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the thirteen experience capabilities in the seed (**held**: the one each case needs) | **all granted** | **none** | Second Life: all; OpenSim: none |
| an unknown id over `GetExperienceInfo` | `experience_keys` empty, the id in `error_ids` **as a string** | — | the same |
| `IsExperienceAdmin` / `IsExperienceContributor` for an unknown id | `{ status: false }` | — | false |
| `GroupExperiences` for a group with none | `experience_ids: []` | — | `[]` |
| `GroupExperiences` for Bugsly Testing's group | six ids, Bugsly Testing among them | — | the arena's group: the arena |

## A record

Every field of every record aditi served across 120-odd search results:
`public_id`, `name`, `description`, `agent_id`, `group_id`, `maturity`,
`properties`, `quota`, `expiration`, `slurl`, `extended_metadata`.

| behaviour | Second Life | fake grid |
| --- | --- | --- |
| the owner (`agent_id`) | **always** a resident, a group's experience too | the fixture resident, or the agent for its own |
| the group (`group_id`) | the nil id, or a group **beside** the owner (Bugsly Testing: both; **held**) | the arena carries a group beside its owner |
| `slurl` | a `secondlife://Aditi/secondlife/<region>/x/y/z` URL, the empty string, or the word **`last`** — about one record in five | a URL, nothing, or `last` (the tour) |
| `maturity` | 13, 21, 42 — and **0** and **254**, which the reference shows no rating for | 13 or 21 |
| `properties` | 0, 8, 16, 24, 48 (grid + private), 64 (disabled), 216 (privileged, grid, disabled, suspended) | the five corners of the record |
| `expiration` | 600, also 10, 60 and 300 | 0 |
| `quota` | 128, also 1024 | per record |
| `extended_metadata` | an LLSD-XML string (`logo`, `marketplace`), or empty | empty |

`last` and `home` are the two location words of the reference's SLURL type
(`LLSLURL`); its profile shows the word. Our decoder read `slurl` as a URL
and refused `last` — and with it the whole reply, so every search page
holding such a record came back as nothing, and the profile of such an
experience never filled. The record keeps the owner and the group as two
fields now; it used to fold them into one, which dropped the group of every
group experience.

## Search (`FindExperienceByName`)

| behaviour | Second Life | fake grid |
| --- | --- | --- |
| a page | **30** records; reply order is the grid's, not by name | 30, by name |
| more than one page | `total_pages`, and `next_page_url` / `previous_page_url` as **relative paths** (`/experiences/search/name?page=2`) | the two markers as query suffixes; no `total_pages` (the reference reads neither the URLs nor the count, only whether each marker is there) |
| a one-page result (`linden`: 10) | none of the three keys | neither marker |
| no match | `experience_keys: []` alone | the same |
| page 0 | page **1**'s records, `next_page_url` page 2 | the same |
| a page past the end (5 and 9 of 4) | the **last** page's records, `previous_page_url` the requested page less one, no next | the same |
| a private experience (`properties` 32) | **listed** (`Load Testing`, by name) | listed |
| a disabled one (64), a suspended one (128) | listed | listed |
| a search for an experience's id | nothing | nothing |
| an exact name (`Bugsly Testing`; **held**) | that record, alone | the same (`Fake Grid Trial 03`) |

## Preferences (`GetExperiences`, `ExperiencePreferences`)

| behaviour | Second Life | fake grid |
| --- | --- | --- |
| an avatar with none | `experiences` and `blocked` both **`<undef/>`** | `<undef/>` |
| Allow, Block, Forget (**held**: allow, then forget, restores) | the reply carries both full lists, an empty one `<undef/>` | the same |
| Block an allowed experience | it moves from `experiences` to `blocked` | the same |
| Allow an id **no experience has** | **kept**: it is listed as allowed until forgotten | kept |

## Relationships and the estate

| behaviour | Second Life | fake grid |
| --- | --- | --- |
| `AgentExperiences`, `GetAdminExperiences`, `GetCreatorExperiences` for an avatar with none (**held**) | `experience_ids: []` (an array, not `<undef/>`) | the fixture agent owns two, administers three, contributes to one |
| `IsExperienceAdmin` / `IsExperienceContributor` for a stranger's experience (**held**) | false, false | false, false |
| `RegionExperiences` GET, not a manager (**held**: two allowed) | answered: Mauve allows two; `blocked` / `trusted` `<undef/>`, `default` the **nil id**, and a `disabled` key, `<undef/>` | the fixture's three lists and a real default; the nil id when there is none, `disabled` `<undef/>` |
| `RegionExperiences` POST, not a manager | **405 Method Not Allowed**, body `Method Not Allowed` | applied (the fake grid models no estate rights yet: `server-fake-grid-estate-actions`) |
| `ExperienceQuery` on the parcel for Mauve's two and one other | `{ experiences: { …: true, true, false }, parcel_id: 3, parcelid: 3 }` — the region's two admitted, the grid-wide stranger not | a parcel given a list admits that list, one given none admits everything; which of the two Mauve's parcel is could not be read (its own experience lists need land rights), so the undeclared case is unmeasured |
| `UpdateExperience`, not an administrator | **403 Forbidden**, a sentence: `Agent <id> is not an admin for experience <id>` | the same |

The nil `default` is "no default" to the reference (`notNull()` guards the
sticky trusted row); we read it as a default named nil, until this
measurement. Our POST helper discarded a refusal's status and body; both
runtimes now log them.

## OpenSim

The seed grants no experience capability (**held**, all three cases): the
three cases record `experience_capabilities = absent` and stop. The fake
grid's OpenSim flavour withholds the same set
(`OPENSIM_REFUSED_CAPABILITIES`). What our viewer does about a floater whose
capabilities are missing is the roadmap's
`viewer-refused-capability-silent-drops`.

## What could not be measured

- Everything an owner or administrator sees: `IsExperienceAdmin` true, the
  owned / admin / contributor lists non-empty, a successful
  `UpdateExperience` and its `removed` answer for a refused field, acquiring
  an experience — no aditi avatar owns one, and creating one needs Premium.
- `RegionExperiences` as an estate manager, and an estate's real `default` —
  no estate rights on aditi (`gridspec-aditi-test-land`).
- `GroupExperiences` for a group our avatar is in that has experiences.
- The `disabled` key's meaning: aditi only ever sent it `<undef/>`, and the
  reference never reads it.
