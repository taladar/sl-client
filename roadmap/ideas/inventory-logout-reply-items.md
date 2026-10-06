---
id: inventory-logout-reply-items
title: Read the inventory items a LogoutReply names
topic: inventory
status: ideas
origin: gridspec-logout (2026-10-06)
refs: [gridspec-logout, gridspec-inventory-fetch]
---

Context: [context/inventory.md](../context/inventory.md).

Second Life's `LogoutReply` names inventory items in its `InventoryData`
block: four ids on each of two settled aditi logouts (two different
accounts), and the single nil id — "none" — on a logout made the moment the
region was up. OpenSim always sends the nil id
(`book/src/gridspec/session.md` § Logout).

The reference viewer (`process_logout_reply`) looks each id up as an
inventory item and marks its parent folder changed, so the inventory cache it
writes on the way out already accounts for the folder versions the logout
bumped. The session drops the block: `Event::LoggedOut` carries nothing, and
the cache saved at logout holds the folders' versions from before.

Unknown, and the first thing to find out: which items they are (the worn
attachments, whose state the simulator saves at logout, are the likely
answer — compare the ids with the avatar's attachments), and whether the next
login's skeleton really reports a higher version for their folders, which
would make our cache refetch them on every login.

Do, if so: carry the ids out of the session at logout and bump the cached
versions of their folders before the cache is saved.
