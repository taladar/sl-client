# sl-content-test

Tests somebody else's LSL content offline. A case is a TOML file naming the
objects to rez and what each holds (scripts, notecards, animations, sounds,
textures, items to give), the avatars to bring and what each does (walk,
touch, sit, pay, answer a dialog or a permission question, wait for a line),
and what should then have happened (what was said, by whom, on which channel;
what an avatar was shown, offered and charged; which run-time errors are
expected). The runner plays it on an in-process fake grid — no grid, no
window, no network — and reports each expectation that did not hold, every
script that did not compile and every run-time error with the line it
happened on.

```sh
cargo run --release -p sl-content-test -- sl-content-test/cases
```

The exit status is 0 when every case passed, 1 when one failed, and 2 when a
case could not be read or played. The format, the report and what the fake
grid does not imitate are described in the book chapter
`book/src/tools/content-test.md`.

A case that passes here can still fail on Second Life: the fake grid is not
Second Life. Physics, vehicles and timing under load are approximate, and
whatever the fake grid does not implement it says so rather than imitating.
