---
id: test-e2e-web-browser-typed-address-lost-under-load
title: e2e_preferences' web browser test sees the home page, not the typed address
topic: test
status: bugs
origin: commit hook's full suite while landing server-lsl-mono-collision-model-under-lowered-limit (2026-11-11)
---

Context: [context/automation.md](../context/automation.md).

## Observation

`e2e_preferences::the_web_browser_and_the_search_web_tab_load_a_page`
failed once in the commit hook's full run (4977 tests) and passed when run on
its own straight after. The web browser window opened by the menu
(`floater:web-browser#new-0`) never showed the address typed into it
(`http://127.0.0.1:<port>/browser`, then Enter). For the whole 60 s wait its
document read the default home page instead:
`https://secondlife.com/?openid_identifier=…anonymous`, titled "Official Site |
Second Life …".

## Likely cause

A race between the window's home page navigation and the typed one: under load
the home page's load either lands after the typed navigation and replaces it,
or Enter reaches the field before the floater takes the address. Passing alone
and failing under load points at ordering, not at a wait that is too short,
since the home page did load.

It failed the same way again in the hook's full run while landing
test-lsl-content-suite (2026-11-13), test 1246 of 8441, the document reading
`https://secondlife.com/` for the whole wait.

Also worth noting: the test reaches the public internet (secondlife.com)
through the home page. An offline run would fail this way every time.

## Wanted

Find which of the two orderings it is (a trace of the browser's navigations
with their sources), make a typed address always win over a home page load
still in flight, and keep the test off the network, for example by staging a
local home page.
