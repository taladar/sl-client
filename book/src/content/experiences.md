# Experiences

**Experiences** are a Second Life feature for scripted content that, once a user
grants permission *to the experience* rather than to each object, can act on the
user's behalf across many objects and regions — attach things, teleport them,
control the camera, and so on, without prompting every time.

> **Second Life only.** OpenSim does not ship an experiences module, so on the
> test grid these requests have nothing to answer them. This is one of the
> clearest SL-vs-OpenSim feature gaps. (See the
> [Introduction](../introduction.md#what-we-are-talking-to).)

## The model

- An **experience** has metadata: a name, description, owner/group, a
  marketplace slug, a thumbnail, and **property flags** (grid-wide, private,
  privileged, suspended, disabled, …).
- A user holds a **permission** per experience — allowed or blocked — which
  scripts in that experience rely on.
- Land can be associated with a set of **region experiences** that are allowed
  to run there.

## What a client does with them

Almost everything is over [CAPS](../comms/caps.md), as LLSD queries against a
family of experience capabilities:

- **Look up** — info for specific experience keys (`RequestExperienceInfo`) and
  search by name (`FindExperiences`), returning `Event::ExperienceInfo` /
  `ExperienceSearchResults`.
- **Permissions** — the user's allowed/blocked list
  (`RequestExperiencePermissions` → `Event::ExperiencePermissions`) and changing
  one (`SetExperiencePermission`).
- **Owned / admin / contributor** — which experiences the avatar owns, admins,
  or contributes to (`RequestOwnedExperiences`, `RequestAdminExperiences`,
  `RequestCreatorExperiences`, `RequestExperienceAdmin`,
  `RequestExperienceContributor`), and group experiences
  (`RequestGroupExperiences`).
- **Edit** — update an experience's metadata (`UpdateExperience` →
  `Event::ExperienceUpdated`).
- **Region** — list and set the experiences allowed on the current land
  (`RequestRegionExperiences` / `SetRegionExperiences` →
  `Event::RegionExperiences`).

---

> **In this codebase**
>
> - Wire types and LLSD query/response builders are in
>   `sl-wire/src/experience.rs` (and the `sl-wire/src/experience/` submodules):
>   `ExperienceInfo`, `ExperiencePermission`, `ExperienceProperties`,
>   `ExperienceUpdate`, and the `PROPERTY_*` flag constants.
> - The experience capabilities are the `CAP_*_EXPERIENCE*` constants in
>   `sl-proto/src/session.rs`; the CAPS driver is
>   `sl-client-tokio/src/experiences.rs`.
> - Commands are the `*Experience*` variants in `sl-proto/src/command.rs`;
>   events the matching ones in `sl-proto/src/types/event.rs`. Worked example:
>   `sl-client-tokio/examples/experiences.rs`.
