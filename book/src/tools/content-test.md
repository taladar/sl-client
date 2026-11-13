# Testing LSL content

`sl-content-test` tests scripted content somebody else wrote — a vendor, a
HUD, an animation system, a whole linkset of them — offline. A **case** is a
data file: the objects to rez and what each holds, the avatars to bring and
what each does, and what should then have happened. The runner plays it on an
in-process [fake grid](fake-grid.md) and reports, for every expectation that
did not hold, what was expected and what happened instead; every script that
does not compile, with the grid's errors; and every run-time error, with the
script, the line and the event it stopped in.

```sh
cargo run --release -p sl-content-test -- sl-content-test/cases
```

No grid, no window, no network. The exit status is 0 when every case passed,
1 when one failed, and 2 when a case could not be read or played at all — a
missing file, a case file that does not parse, a run that hung — so "the
content is broken" and "the test did not happen" never look the same. Each
run of a case is one `PASS` or `FAIL` line, with what went wrong indented
under it, and a last line counting both; `--verbose` adds everything the run
observed. `RUST_LOG` raises the grid's own log on stderr.

The scripts under test are the thing being tested; the grid is the fixture.
That is the difference from the [probe corpus](lsl-probes.md), whose scripts
are ours and pin down the runtime's semantics against the live grids.

## A case

A case is a `case.toml` in a directory of its own, holding the files it names
(or a `<name>.case.toml` beside them). Given a directory, the runner finds
every case file below it. Paths in a case are relative to the case file.

```toml
name = "Gadget vendor: menu, notecard price, payment, delivery"
grids = ["second-life"]

[[object]]
name = "Gadget Vendor"
position = [128.0, 131.0, 25.25]
contents = [
  { script = "vendor.lsl" },
  { notecard = "config.txt" },
  { object = "Red gadget" },
  { object = "Blue gadget" },
]

[[object.link]]
name = "Display"
offset = [0.0, 0.0, 0.5]
contents = [{ script = "display.lsl" }]

[[avatar]]
name = "Buyer Resident"
steps = [
  { await_chat = "Ready" },
  { touch = "Gadget Vendor" },
  { answer_dialog = "Blue" },
  { await_chat = "Showing" },
  { pay = "Gadget Vendor" },
  { await_chat = "Enjoy" },
]

[[expect]]
by = "Gadget Vendor"
says = "Thank you, Buyer Resident! Enjoy your Blue gadget."

[[expect]]
transcript = "Buyer Resident"
lines = ["paid Gadget Vendor L$50", "Gadget Vendor offered Blue gadget"]
```

That is an excerpt of `sl-content-test/cases/vendor/`, which the crate's
tests play as written and again with its vendor script broken.

A misspelt field is an error naming it, never silently ignored.

### The run

| field | default | |
| --- | --- | --- |
| `name` | the directory's name (or the file's stem) | what the report calls the case |
| `grids` | `["second-life"]` | each of `"second-life"` and `"opensim"` is a run of its own |
| `seed` | `1` | every id, `llFrand` draw and timer of the run comes from it |
| `step_ms` | `100` | one tick of the region's clock |
| `max_ticks` | `600` | a run still going after this many ticks is failed for not finishing |
| `settle_ticks` | `20` | ticks run after every avatar has finished, for what its last step set going |

The region is the fake grid's stock region: 256 m square, flat ground at
25 m, an avatar arriving at its middle (128, 128). Its heartbeat is stepped by
the runner, so a tick is a fixed amount of region time however fast the
machine is.

### Objects

Each `[[object]]` is rezzed when the region starts:

| field | |
| --- | --- |
| `name` | the root prim's name, which steps and expectations name it by |
| `position` | `[x, y, z]`, region metres |
| `size` | `[x, y, z]` metres, `[0.5, 0.5, 0.5]` by default |
| `owner` | who owns it: one of the case's avatars by name, or anybody else, who gets an account that never logs in; `Content Owner` by default |
| `click` | its click action before a script sets one: `touch`, `sit`, `buy`, `pay`, `open`, `play`, `open-media`, `zoom` |
| `contents` | what the root prim holds |
| `[[object.link]]` | a linked prim: `name`, `offset` (root-relative metres), `size`, `contents`; link numbers follow the order written |

An item of `contents` names exactly one of:

| key | the item |
| --- | --- |
| `script = "file.lsl"` | an LSL script, running from the start unless `running = false` |
| `notecard = "file.txt"` | a notecard of that text |
| `object = "Name"` | an object item a script can give (`llGiveInventory`) |
| `animation`, `sound`, `texture` = `"file"` | an asset-backed item, its bytes read from the file |

`name` overrides the item's name (the file's stem by default). An asset item
takes `id` to fix its key, for content that names it by uuid. A table of
`[[asset]]` (`id`, `file`) serves an asset no prim holds, which a script names
by key alone.

### Avatars

Each `[[avatar]]` is a [scripted avatar](fake-grid.md#scripted-avatars): a
real client the grid logs in itself, as `name` (`First Last`), at `start`
(region metres, the arrival point by default), `dressed` or not. It runs its
`steps` in order, each one action:

| step | |
| --- | --- |
| `walk = [x, y, z]` | walk there |
| `touch = "Name"` | touch the object of that name |
| `sit = "Name"` | sit on it, and wait for the grid's answer |
| `stand = true` | stand up |
| `say = "text"` | say it, on `channel` (0 by default) |
| `pay = "Name"` | pay it `amount`, or what its pay window offers first, and wait for the grid's answer |
| `answer_dialog = "Label"` | answer the next dialog with that button (`answer_dialog_index` by position, `answer_text` for a text box) |
| `grant = 0x404` | answer the next permission question granting those `PERMISSION_*` bits of what it asked; `refuse = true` grants nothing |
| `await_chat = "text"` | wait until it hears a line containing that |

A step is taken `after_ticks` ticks after the one before: 1 for an action and
0 for an answer or an awaited line, unless the step says otherwise. Waiting is
counted in ticks, never in time, so a run does the same thing on any machine.

What no step answers, `answers` does:

```toml
[avatar.answers]
permissions = "grant"
dialogs = "first-button"
payments = "default-price"
```

Each is `"ignore"` (`"never"` for payments: a touch stays a touch) when not
given, which is itself worth testing — a question nobody answers stays open.

### Expectations

Each `[[expect]]` is one of four kinds, told apart by its fields.

**A line said** — `says` (the exact text) or `says_containing` (a part), with
any of `by` (the speaker's name), `channel` (region chat only) and `heard_by`
(an avatar: look among what that avatar was told, which also holds the lines
said to it alone — `llOwnerSay`, `llRegionSayTo`, `llInstantMessage` — rather
than in the region's chat). `never = true` turns it round: the line must not
have been said.

**A transcript** — `transcript` (an avatar) and `lines`, which the avatar's
transcript must hold in that order, others between them allowed. A transcript
line reads as what happened:

```text
arrived
touched Gadget Vendor
Gadget Vendor showed a dialog "Choose a colour" ["Red", "Blue"]
answered Gadget Vendor with "Blue"
heard Display: Showing the Blue gadget for Buyer Resident
paid Gadget Vendor L$50
heard Gadget Vendor: Thank you, Buyer Resident! Enjoy your Blue gadget.
Gadget Vendor offered Blue gadget
```

Run a case `--verbose` once to see the lines to copy.

**A run-time error** — `fault` (text the error contains, `Math Error`) and
optionally `script`. Any run-time error no expectation names fails the case,
so a script that stops quietly cannot pass.

**A place** — `object` (a prim, root or linked, by name) and `at` (region
metres): where it stands when the run ends, within `within` metres (a
centimetre by default). What a script moved, and what it should have left
alone.

## The report

A broken vendor — its `money` handler dividing by a price it zeroed — reads
(each line wrapped here to fit, at a `⏎`):

```text
FAIL Gadget vendor: menu, notecard price, payment, delivery [second-life] ⏎
  200 ticks
    script "vendor" in "Gadget Vendor" stopped at line 102, column 27 ⏎
      in money with Math Error
    Buyer Resident had not finished its steps after 200 ticks; ⏎
      it was on step 6 of 6 (await_chat = "Enjoy")
      its transcript ends:
      …
      paid Gadget Vendor L$50
    expected "Gadget Vendor" to say "Thank you, Buyer Resident! ⏎
      Enjoy your Blue gadget." to Buyer Resident; it said:
      Gadget Vendor says: Ready: 2 colours at L$50
      …
    expected Buyer Resident's transcript to hold, in order, line 4 of 4: ⏎
      "Gadget Vendor offered Blue gadget"; its transcript:
      …
```

The line a script stopped at is the one thing in the report no resident of a
live grid is told: the grid's own error lines leave it out, so the runtime
hands the whole fault to the grid (`Host::faulted`) and the grid to the
harness (`FakeGrid::script_faults`). The region's chat comes from a tap on the
region's listens (`FakeGrid::overhear`) that hears every line on every
channel, so object-to-object chatter on a hidden channel is as checkable as a
`llSay` on channel 0.

A call to a library function the fake grid only stubs is a `note:` too
(`FakeGrid::stubbed_calls`): it answered its default and did nothing else.

A key a script writes literally that the case does not supply — no `[[asset]]`
or item `id` for it, and not a built-in animation — is a `note:` under the
verdict, not a failure: the fake grid answers a fetch of it as not found,
where the live grid may well hold it. A script that does not compile, as the
imitated grid compiles it, fails the case with the grid's errors.

## Fidelity

This grid is not Second Life, and a case passing here can fail there. The
[gridspec](../gridspec/index.md) records what the fake grid was measured to do
like each live grid and the [probe corpus](lsl-probes.md) the
library's behaviour against them; within that, these surfaces are known to be
approximate:

- **physics and vehicles**: physics is kinematic
  ([physics](../simulator/physics.md)) — a physical object falls, lands,
  bounces where OpenSim's would, collides, passes through a volume-detecting
  prim and is drawn by `llMoveToTarget`, but it does not tumble, slide, stack
  or ride, and forces, impulses and vehicles are not implemented;
- **timing under load**: a script's speed is charged as each engine was
  measured to run on an idle region, and the region never lags, drops events
  under load, or runs a script slower because its neighbours are busy;
- **what the grid does not implement**: a library function the grid has no
  implementation of is a run-time error naming itself, which fails the case;
  one it only stubs answers its default and does nothing else, and each such
  call is a `note:` under the verdict, so a default is never mistaken for the
  function's real answer;
- **an object item** in a prim's contents can be given but not rezzed: no
  object asset stands behind it;
- **a gift** is offered and not filed: the avatar's transcript says what it
  was offered, and nothing lands in its inventory;
- **OpenSim's payments**: OpenSim tells a payer nothing of a payment to an
  object, so on an `"opensim"` run a `pay` step waits for an answer that never
  comes.

The library's coverage and the grid's divergences from each live grid are
tracked in `roadmap/context/lsl.md` and the gridspec; the
[differential runner](lsl-probes.md) is what bounds the gap.
