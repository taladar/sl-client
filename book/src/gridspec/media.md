# Media

What each grid keeps of the media a prim shows, and what it does with a
request to change it. The messages are described in
[Sound, Music & Media](../content/sound-media.md).

## Media on a prim

A prim face may show a web page. Its settings — a home address, the address
shown now, a whitelist, who may interact and who may use the controls — are
one record per face, kept by the region and read and written over two
capabilities: `ObjectMedia`, a POST whose `verb` is `GET` or `UPDATE`, and
`ObjectMediaNavigate`, a POST that moves one face to another address. Neither
answers with the media. What says that a prim's media changed is the prim
sent again with a new **media version** in its `MediaURL` field,
`x-mv:<serial>/<agent>`, and a viewer that sees a version it does not hold
asks with a `GET`.

Measured on 2026-10-12 by the conformance case `object-media`. One avatar
rezzes a cube on land it may build on; a second, no friend with edit rights
and with no group in common, stands by. The case POSTs each request to the
capability itself, so that it reads the status and the body, then listens on
both avatars for the cube sent again and reads the media back with the
owner's `GET`. It ran four times on aditi (Mauve, the public sandbox parcel)
and four on the local OpenSim. Every row below is checked on every run of
both live grids.

### Reading

| question | Second Life | OpenSim |
| --- | --- | --- |
| the two capabilities | granted | granted |
| a `GET` of a prim with no media | `200`, a map of `object_id`, `object_media_data` and `object_media_version`; one `undef` for each of the prim's faces; **the version an empty string** | the same map and the same six `undef`; the version `x-mv:0000000000/` and the nil id |
| that prim in the object stream | no `MediaURL`, no face with the media flag | the same |
| a `GET` once a face has media | the face's record a map, the others `undef` | the same |
| the keys of a record | `alt_image_enable`, `auto_loop`, `auto_play`, `auto_scale`, `auto_zoom`, `controls`, `current_url`, `first_click_interact`, `height_pixels`, `home_url`, `perms_control`, `perms_interact`, `whitelist_enable`, `width_pixels` — and **`whitelist` only when it has an entry** | the same, with `whitelist` always, an empty array when it has none |
| a record read back against the one set | every field as sent | the same |
| the other avatar's `GET` | what the owner's states | the same |
| a `GET` of a prim the region does not have | `404`, no body | `400`, no body |
| a `GET` naming no prim | `200`, `error` = `{code: 8005, message: "No object_id"}` | `400` |
| a verb that is neither | `200`, `error` 8006 `Invalid verb` | `400` |
| an empty body | `200`, `error` 8006 `No verb` | `400` |
| an HTTP `GET` of `ObjectMedia` | `503` | `400` |
| an HTTP `GET` of `ObjectMediaNavigate` | `405` | `400` |

Second Life answers a request it understood and will not serve with **`200`
and an `error` map** of `code` and `message`; only a prim, a face or a record
that is not there is a `404`. OpenSim answers every refusal `400` with no
body. A viewer that reads the status alone takes every Second Life refusal
for a success.

### The version

| question | Second Life | OpenSim |
| --- | --- | --- |
| the form | `x-mv:`, a serial of ten digits, `/`, an agent id | the same |
| the first set | serial 1, naming the avatar that set it | the same |
| every accepted `UPDATE` and navigation after it | the serial up by 1, naming the avatar that sent it | the same |
| an `UPDATE` that changes nothing | up by 1 all the same | the same |
| a navigation to where the face already is | up by 1 all the same | the same |
| a request that is refused | the version stays | the version stays — but for a stranger's `UPDATE`, below |
| the prim's `MediaURL` after each | the version the `GET` states | the same |
| a clear (an `UPDATE` of six `undef`) | up by 1; the `GET` lists six `undef` and still states the version | the same |
| an `ObjectImage` stating another version in its `MediaURL` | ignored: nobody is told, the version stays the region's | the same |

The serial counts requests, not changes, so two versions do not mean two
different sets of records. Nor does one version mean one set: a face's media
flag changes what a `GET` states without moving it (below).

### Who is told

| what is sent | Second Life | OpenSim |
| --- | --- | --- |
| after an accepted `UPDATE` or navigation, to the avatar that sent it | the prim in a full `ObjectUpdate`, its `MediaURL` the new version | the same |
| to the other avatar | the same | the same |
| after a refused one | nothing, to either | nothing — but for a stranger's `UPDATE`, below |
| the face's media flag in the texture entry of that update | set on the faces that now have media, **cleared on the ones that lost it** | set on a face given media when the prim had none; **never cleared, and not set on a face given media later** |

Neither grid sends the media itself, and neither says what changed: a viewer
learns of a navigation by another avatar from the version alone.

### Setting

Every `UPDATE` here is the owner's, naming all six faces unless the row says
otherwise.

| request | Second Life | OpenSim |
| --- | --- | --- |
| media on face 0 of a prim that has none | `200`, an `undef` body; the record lands | `200`, **no body**; the record lands |
| media on face 3 and none on face 0, of a prim with media on face 0 | face 0 loses its media and face 3 gains it; the flag follows | **face 0 loses its media and face 3 shows none**: the record is kept, but face 3 never gets the media flag, and a `GET` states no media for a face without it. Face 0 keeps its flag |
| seven records for six faces | `200`, `error` 8001 `Bad input: incorrect number of texture indices`; nothing changes | `400`; nothing changes |
| one record | the same error; nothing changes | `400` — **and face 0 takes the record**, at the old version and with nobody told |
| no record | the same error | `400`; nothing changes |
| a home and a current address the record's own whitelist does not pass | lands as sent | lands as sent |
| the records by the client's own `Command::SetObjectMedia` | land | land |

OpenSim's one-record case is its handler writing face by face and failing on
the second: what it wrote stays, and the lines that would have moved the
version and scheduled the update are never reached. Of everything measured it
is the one change to a prim's media no viewer can learn of.

### Navigation

Of face 0, by the owner, with the record's permissions at their default
(anybody) unless the row says otherwise.

| request | Second Life | OpenSim |
| --- | --- | --- |
| to another page | `200`, an `undef` body; `current_url` moves and `home_url` stays | the same |
| to nothing (an empty `current_url`) | `404`; nothing changes | `200`; **`current_url` becomes empty** |
| to text that is no URL (`not a url`) | `200`; `current_url` becomes **`https://not a url`** | `200`; `current_url` becomes `not a url` |
| to a `data:` URL | `200`; kept as sent | the same |
| of a face with no media, of a face the prim has not got, of a prim with no media, of a prim the region does not have | `404` | `400` |
| by the other avatar | `200`; lands, and the version names **that avatar** | the same |
| by the client's own `Command::NavigateObjectMedia` | lands | lands |

Both grids keep an address that is no URL, and anybody who may interact with
the face may send one. `MediaEntry::from_llsd` therefore reads such an address
as none, with a warning, where it used to refuse the record — and with it the
answer, so that one navigation by a passer-by cost a viewer the media of
every face of the prim.

### Permission

| request | Second Life | OpenSim |
| --- | --- | --- |
| navigation by the other avatar, `perms_interact` the owner only | `200`, `error` 8002 `agent <id>: navigate not allowed on object <id>`; nothing changes | `400`; nothing changes |
| navigation by the owner under the same | lands | lands |
| navigation by the owner, `perms_interact` nobody | **lands** | **`400`** |
| an `UPDATE` by the owner, `perms_control` nobody | lands | lands |
| an `UPDATE` by the other avatar, whatever `perms_control` says | `200`, `error` 8002 `Agent <id> does not have edit permission on object <id>`; nothing changes, nobody is told | **`200`, no body; no record changes — and the version goes up by 1 naming that avatar, with a full update to both** |

An `UPDATE` is a matter of edit permission on both grids and `perms_control`
plays no part in it; navigation is a matter of `perms_interact`, which Second
Life does not hold the owner to. OpenSim checks each face of a stranger's
`UPDATE`, changes none, and then versions and announces the request as if it
had.

### The whitelist

With `whitelist_enable` on and a whitelist of `example.com`.

| request | Second Life | OpenSim |
| --- | --- | --- |
| navigation to `https://example.org/elsewhere`, by either avatar | `200`, `error` 8003 `navigate (<url>) did not match whitelist domains`; nothing changes | `400`; nothing changes |
| navigation to `https://example.com/listed` | lands | lands |
| navigation with the whitelist on and **empty** | **lands**: an empty list passes everything | **`400`**: an empty list passes nothing |

### The face's media flag

A face's texture entry carries a media flag (bit 0 of its media byte), which
an `ObjectImage` sets like any other texture change.

| request | Second Life | OpenSim |
| --- | --- | --- |
| an `ObjectImage` clearing the flag of the one face with media | **the face loses its media**: a `GET` states none. The version stays; both avatars get a terse update with the texture entry | the same — and with no face left the records are dropped for good |
| an `ObjectImage` setting it again | the face has media again: **an empty record**, no addresses, the defaults. The version stays; a terse update | the face has the flag and **no media**. The version stays; a terse update |

So the flag is half of the state and the version does not cover it. The
reference viewer asks again on any change of a media flag
(`MEDIA_FLAGS_CHANGED` in `LLVOVolume::processUpdateMessage`).

### The fake grid

Neither flavour does any of this yet: the fake grid keeps `ObjectMedia` state
for each session by itself, mints a version that names the prim, never puts
it in the object stream, tells nobody and refuses nobody. `object-media`
therefore runs on the live grids only, and the measured behaviour above —
with the 187 answers the case holds a run to — is what
`server-fake-grid-object-media-region` is to build, a row in
`sl-fake-grid/src/imitates.rs` for each cell where the two columns differ.

### The viewer

- **A new version is fetched, once** (`ingest_media_events` in
  `sl-viewer-world-view/src/media_prim.rs`): on both grids that covers every
  accepted set, navigation and clear by anybody, and OpenSim's version for a
  stranger's `UPDATE` that changed nothing costs one fetch of the same
  records.
- **A face that regains its media flag is fetched again**, at whatever
  version: both grids answer differently from before the flag went, and
  differently from each other.
- **A face without the flag shows no media** whatever record is held for it,
  which is what both grids' `GET` says.
- **An address that is no URL** leaves the face with media and nothing to
  show; the other faces are unaffected.
- The viewer gates interaction on `perms_interact` as the reference does: the
  owner bit for the owner, the anybody bit for everyone. That is OpenSim's
  rule exactly, and stricter than Second Life's for an owner whose own record
  names nobody. It passes everything on an empty whitelist, as Second Life
  does; on OpenSim such a face's navigation is refused by the region and the
  other avatars do not follow it.

Not measured: media on a child prim of a linkset and on an attachment, the
group bit of the two permissions, a prim taken and rezzed again or copied,
what a script sets and reads (`llSetPrimMediaParams`, with the LSL prim
functions), a prim with more or fewer than six faces, and what a region
hands a viewer that arrives after the last change.
