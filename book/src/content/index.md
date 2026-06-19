# Content Layer

This part covers the features a user actually experiences — logging in, moving
around, owning things, talking, seeing the world — and how each is carried over
the plumbing from the [Communication Layer](../comms/index.md).

A recurring shape runs through every chapter, so it is worth stating once. In
this workspace, an application interacts with the world through two enums:

- it submits a **`Command`** to ask for something (send chat, fetch a folder,
  teleport, buy a parcel), and
- it receives an **`Event`** when something happens (chat arrives, a folder's
  contents come back, a teleport finishes, an object appears).

So each chapter below can be read as: *which commands drive this feature, which
events report it, and whether the traffic goes over
[LLUDP](../comms/lludp-transport.md) or [CAPS](../comms/caps.md).* `Command` is
defined in `sl-proto/src/command.rs` and `Event` in
`sl-proto/src/types/event.rs`; both are re-exported from `sl-proto`.

The chapters:

- **[Login](login.md)** — getting onto the grid.
- **[Teleport](teleport.md)** — moving between places, near and far.
- **[Inventory](inventory.md)** — the avatar's folders and items.
- **[Chat & Instant Messaging](chat.md)** — local chat, IMs, group/conference
  sessions.
- **[3D World Information](world.md)** — objects, terrain, parcels, and the
  avatars around you.
- **[Sound, Music & Media](sound-media.md)** — sound effects, streaming music,
  parcel/object media, and voice.
- **[Groups](groups.md)**, **[Economy & Money](economy.md)**,
  **[Profiles, Picks & Classifieds](profiles.md)**,
  **[Appearance](appearance.md)**, **[Friends & Presence](friends.md)**,
  **[Experiences](experiences.md)**, and **[Materials](materials.md)** — the
  remaining feature domains, one chapter each.
