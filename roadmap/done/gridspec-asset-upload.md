---
id: gridspec-asset-upload
title: Asset uploads and their costs, refusals and announcements on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, test-asset-save-mutation-survey,
  viewer-mesh-upload-sequence, viewer-new-wearable-refused-on-second-life,
  viewer-new-material-refused-on-second-life, gridspec-money]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-13)

Measured and written up in `book/src/gridspec/assets.md` § Uploads.

- **Discover.** A new case, `upload-http`, beside `asset-upload` (which
  drives the client's command and hides the HTTP): it posts both requests of
  an upload itself, listens after each upload for what the grid sends
  unasked, and asks for the balance. `NewFileAgentInventory`: eight stated
  costs, each completed where an uploader came back; eleven requests that are
  not what a viewer writes; fourteen classes; textures, sounds and
  animations that are and are not what they say; a model's fee request and
  five that are no model; the uploader asked twice and with a `GET`; the nil
  folder and one nobody has; and a second avatar that has given its L$ away.
  `MeshUploadFlag`, `InventoryThumbnailUpload` (seventeen requests) and
  `SendPostcard`'s first request. The client's own commands. Five runs on
  aditi (about L$ 100 each) and seven on OpenSim, held to 221 measured
  answers.
- **Findings.** Second Life does not look at `expected_upload_cost` in the
  first request: **it holds it to the bytes** (*The server expects a
  different upload fee*), since only the bytes say whether a texture is a
  large one; a request that states none is charged the price. It states the
  balance in the first answer and refuses there what the balance does not
  cover. It takes a texture or snapshot, a sound, an animation, **a body
  part and clothing (L$ 10 each)** and a mesh; refuses a notecard, script,
  gesture, settings and material by name and the rest with a `500`;
  validates the bytes (a stereo or 22 kHz sound and a 61 s animation pass);
  an uploader serves once and is spent by a `GET`. OpenSim takes every
  class, free and unread — **its `EconomyData` price is not what it
  charges** — files a class it has no name for **as a texture**, and after
  an `object` upload answers `complete` for nothing stored and *Uploader
  busy* until the agent logs in again. A model's first answer carries the
  price and the cost data on both. Thumbnails: any first request gets an
  uploader; up to 256 pixels wide is taken, whatever the shape; free.
- **Fake grid.** `sl_proto::NewFilePolicy` (`ImitatedGrid::new_file_policy`):
  each flavour's classes, prices, refusals in the grid's own words, the cost
  held to the bytes, the bytes validated, the one-shot uploader, OpenSim's
  filing and its stuck uploader. The Second-Life flavour states and charges
  a balance (`upload_funds`, L$ 10 000 a session). Both cases run offline on
  both flavours. **Not imitated**, each for a stated reason: what an upload
  was charged and somebody who cannot pay (no ledger — [[gridspec-money]]),
  a mesh's cost data and bytes ([[server-fake-grid-mesh-upload-cost]], now
  unblocked), a thumbnail read back (the inventory has no place for one),
  the balance pushed after a charge, and the words for an unreadable
  animation.
- **Capabilities.** `MeshUploadFlag` (`Command::RequestMeshUploadFlag` →
  `Event::MeshUploadFlag`), `InventoryThumbnailUpload`
  (`Command::UploadInventoryThumbnail`) and `SendPostcard`
  (`Command::SendPostcardViaCaps`), in both runtimes and the REPL, served by
  the fake grid per flavour.
- **Viewer.** `sl_proto::expected_upload_cost` is the price a viewer states
  (the benefits package's, else `EconomyData`'s); the conformance cases use
  it, **the viewer has nothing to price yet** — it uploads no texture, sound
  or animation. What it does upload over `NewFileAgentInventory`, new
  wearables and new materials, **Second Life refuses**: filed as
  [[viewer-new-wearable-refused-on-second-life]] and
  [[viewer-new-material-refused-on-second-life]] rather than fixed here. A
  refusal stated as a bare `message` (the thumbnail uploader's) is now read.
- **Not measured.** A mesh **upload** on Second Life: every test account's
  `mesh_upload_status` is `invalid`, and the grid refuses the upload after
  quoting it. A postcard's second request: it sends a real e-mail, and the
  case sends one only when the fixtures name a `postcard_to`. Premium
  prices; a money module that charges on OpenSim; the legacy UDP upload.

## Known already

SL rejects a notecard over `NewFileAgentInventory`, charges from benefits,
checks `expected_upload_cost`; announcement rows exist. Each aditi upload costs
L$.

## Discover

`asset-upload` with wrong / unaffordable costs, sound and animation validation,
mesh step-1 costing (after [[viewer-mesh-upload-sequence]]); record once per
class.

## Document

`book/src/gridspec/assets.md` § Uploads.

## Fake grid

Small in this task: cost checks, notecard refusal. Mesh costing:
[[server-fake-grid-mesh-upload-cost]].

## Viewer

Price from benefits on SL, `EconomyData` on OpenSim.

## Capabilities done in this task

The protocol halves of three upload-related capabilities: `MeshUploadFlag`
(both grids; [[viewer-mesh-upload-sequence]]), `InventoryThumbnailUpload`
(Second Life; [[viewer-inventory-thumbnails]]) and `SendPostcard` (Second Life;
[[viewer-snapshot-postcard]]) — the command, the two-step upload where it
applies, and the fake grid serving each per flavour.
