# Grid behaviour

Second Life and OpenSim speak the same protocol and do not behave the same.
This part records, feature by feature, **what each live grid actually
does** — measured on Second Life's beta grid (aditi) and on a local OpenSim
— and what the fake grid does when it imitates each of them.

It is the committed record behind three things:

- `sl-fake-grid`'s `ImitatedGrid` flavours (`sl-fake-grid/src/imitates.rs`):
  every divergence in a chapter here is a row there, and the fake grid's
  conformance runs are held to the measurement;
- the viewer, which has to handle both grids' answers;
- the protocol chapters of the *Content Layer*, which describe the messages
  and link here for how each grid uses them.

## How a chapter reads

Each behaviour gets a table with one column per grid:

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| *what is observed* | *the answer, and how / when it was measured* | *the same* | *what each flavour does* |

A cell that could not be measured says so and why (no estate rights on
aditi, no module on stock OpenSim, …).

Chapters are added by the roadmap's `gridspec-*` tasks; see
`roadmap/context/gridspec.md` for the method.
