# Terrain, wind and clouds

A simulator sends a region's ground as `LayerData` messages: a group header
(a stride, a patch size, a layer type) and then patches of 16 × 16 cells,
each a header (`QuantWBits`, a `dc_offset`, a `range`, its position) and
its heights as quantized DCT coefficients. The wind travels the same way,
as two patches — the east and the north component of one 16 × 16 field
over the whole region — and so once did the clouds and the water. The four
textures the ground is painted with, and the heights they blend at, are in
the `RegionHandshake`.

Measured on 2026-10-08 by the `terrain-layerdata` conformance case, which
arrives in a region and listens for 100 s with its circuits probed from the
first datagram: three runs on aditi (in Ahern, a mainland region with three
neighbours, the agent in its south-west corner at 7.7, 10.1) and three on
the local OpenSim (the south-western region of its 2 × 2 block, the agent
at its centre). The case reads each message's time, length and reliability
off the datagram and its headers off `Event::TerrainLayerBatch`. The four
textures were fetched by `terrain-composition`, once on each grid. Rows
marked **held** are checked on every run, live or offline: each fake
flavour against its live grid's answer.

## The ground

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| how much of the region comes (**held**) | all 256 patches, each once, unasked | the same | the same |
| how it is sent (**held**) | reliably; a stride of 264, a patch size of 16, `LayerID.Type` and the group header both `L` | the same | the same |
| when | from 1.36 s after the first datagram to 1.76 s: thirty messages in 0.4 s | from 0.54–0.67 s to 0.95–1.08 s: five messages a tenth of a second apart | with the rest of the arrival |
| where a message is cut (**held**) | before the patch that would take it past about 1,200 bytes: payloads of 975 to 1,201 bytes, the region's last 684 | by the patch that takes it past 890 bytes (its source: byte 900 of the datagram): 899, 901, 904 and 911 bytes, the last 745; up to 1,071 in a neighbour's | `FakeSl` keeps a payload within 1,200 bytes and its end marker, `FakeOpensim` closes one once past 890 |
| so how many patches a message holds | 4 to 13 of this hilly region | 44 to 60 of a mostly flat one | whatever fits: about a hundred of the stock flat ground |
| which patch comes first (**held**) | the one the agent stands in | the same | the same |
| the order after it (**held** as strict or not) | nearest first, roughly: 26 of 255 patches came nearer the agent than the one before, and no two runs gave the same order | nearest first exactly, the distance counted in whole patches from the agent's; equally far patches in no fixed order | `FakeSl` by the distance from the agent to a patch's centre, `FakeOpensim` in whole patches from the agent's |
| a patch with relief | ten bits of prequantization; coefficients of 9 to 13 bits | the same, 10 to 12 bits | the same |
| a patch with none (**held**) | the same as any other. A `range` of one (5 of 256) is a spread under a metre, not a flat patch | a header alone: `QuantWBits` 0 — two bits of prequantization and of word size — a `dc_offset` half a metre under the height, a `range` of one, and an end-of-block. 176 of 256 | every patch transformed on `FakeSl`, a flat one as a header alone on `FakeOpensim` |
| a neighbour's ground (**held**) | all 256 patches of each of the three, down its child circuit, from 1.7–1.9 s to 3.7–4.5 s, in 107 or 108 messages | all 256 of each of the three, from 1.8 s to 3.9 s, in 37 messages | all 256 of each neighbour |
| sent again later | never in 100 s | never | never |

Neither grid holds any of the ground back for being far from the agent: the
far corner of the region and all of each neighbour came in the first five
seconds. OpenSim's source sorts what lies within the draw distance and then
sends the rest in rows; nothing at this draw distance told the two apart.

## The wind

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| what a message holds (**held**) | two patches, both at position (0, 0) | the same | the same |
| how often (**held**) | every 1.000 s | every 13.63 s: 150 of its frames | each flavour's |
| the first one (**held**) | 0.3 ms behind the last message of the ground, and then on the region's clock, 0.6 to 0.8 s later | whenever the region's clock next says: 0.4 s, 3.7 s and 9.0 s after arrival, once ahead of the ground | `FakeSl` behind the ground and then every second; `FakeOpensim` an interval after the session opens |
| reliably (**held**) | no | yes | each flavour's |
| the group header's stride (**held**) | 18 | 264, as for the ground | each flavour's |
| the prequantization (**held**) | six bits | ten | each flavour's |
| its size | 46 to 62 bytes | 606 to 773 bytes | by the same arithmetic |
| the values | −4.6 to 2.6 m/s over three runs, 0.5 to 2.4 m/s apart within one patch | −1.0 to 1.0, close to 2.0 apart within every patch | the fixture's one velocity everywhere |
| down a child circuit (**held** as the feed running) | each neighbour's own, every second, unreliably | each neighbour's own, at the same instants as the root's | each neighbour's, on its flavour's timer |

## Clouds and water

Neither grid sent a cloud (`8`) or a water (`W`) layer in any run, nor an
extended layer (`M`, `9`, `:`, `X`), which only a region larger than 256 m
would; the local OpenSim has none. OpenSim's cloud module is off unless
configured. The case fails a run in which any other layer than the ground
and the wind arrives (**held**). A fake-grid fixture with `clouds` set
still sends a cloud layer: a region neither grid has.

## The textures

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the handshake's four `TerrainDetail` ids | four textures, each fetched and decoded as JPEG2000 | the same: its four stock ones | the four default ones, served |
| the heights they blend at | 20 m and a 60 m range at every corner | 10 m and 60 m | 10 m and 60 m |
| the water height | 20 m | 20 m | the region's |

These are the region's own settings rather than its grid's. An earlier
note had aditi's mainland sending four nil ids; Ahern, which is mainland,
names four, so that was some other region's doing. A region whose ids are
nil, or name PBR materials instead of textures, was not found.

## What our viewer does with it

- `Event::TerrainPatch` hands it each patch's heights whichever way the
  message was cut or the patch written; `Event::TerrainLayerBatch` carries
  the rest of a message — its headers and the order of its patches — which
  the viewer has no use for and this measurement does.
- It makes nothing of the wind yet.
- Nil detail ids are replaced with the default textures. Materials in their
  place are `viewer-pbr-terrain`.

Checked through the viewer automation, whose ground pick answers with the
height the viewer has for a point (`e2e_terrain`): on each fake flavour, in
a region of terraces, the height is the region's on a flat patch, on
either side of a terrace edge inside one patch, and beyond it; and on each
live grid (`SL_E2E_GRID=opensim` and `=aditi`) the viewer has ground the
pointer can rest on beside its avatar.

Not measured: whether Second Life counts "nearest" from the agent or from
its camera, which sat beside it; what either grid does at a small draw
distance; a region larger than 256 m.
