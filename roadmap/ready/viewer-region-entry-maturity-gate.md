---
id: viewer-region-entry-maturity-gate
title: Eight ported notifications for a refused entry, and nothing raises one
topic: viewer
status: ready
origin: noticed auditing maturity enforcement while doing test-fake-grid-imitates-economy (2026-09-08)
points: 3
refs: [protocol-account-benefits-package, viewer-search-maturity-filter]
---

Context: [context/viewer.md](../context/viewer.md).

A grid refuses an entry the account's maturity does not allow, and says so with
an `AgentAlertMessage` naming `RegionEntryAccessBlocked`. `sl-proto` decodes
that alert — `lifecycle.rs` pins it twice — and
`sl-viewer-notifications` carries the whole family of strings the reference
viewer answers it with:

- `RegionEntryAccessBlocked` and `_Notify`
- `_Change` and `_PreferencesOutOfSync` — the two that *offer to fix it*
- `_NotifyAdultsOnly` and `_AdultsOnlyContent`
- `LandClaimAccessBlocked`, `RegionMaturityChange`

Nothing raises any of them. They were ported as catalogue data by the
`viewer-notification-catalogue-*` family, which ports strings rather than the
code that raises them, and no code path maps a refusal onto one — so the viewer
decodes the grid's refusal and
drops it into the generic alert path — or nowhere the user connects to what they
just tried to do.

The two that matter most are the ones that offer a remedy. The reference
viewer's flow is not "you may not go there" but "your preference says Moderate
and that region is Adult — raise it?", and `_PreferencesOutOfSync` is the case
where the *server* thinks the preference is one thing and the viewer another,
which the General panel's retry conversation can already detect but has nothing
to say about.

Wanted:

- A path from the decoded refusal to the right member of the family, chosen the
  way the reference viewer chooses it (which rating was refused, whether the
  account *could* raise its preference, whether the mismatch is a sync problem
  rather than an entitlement one).
- The preference-change offer wired to the existing maturity conversation in
  `sl-viewer-preferences`, so accepting it re-sends and retries rather than
  writing the setting locally and diverging again.
- The fake grid taught to refuse, which is what makes this offline-testable:
  a region with a maturity above the account's, and an account whose ceiling is
  below it. Both flavours can do this — the ratings are region policy, not a
  grid divergence — so it needs no live-grid run to verify.

Depends on [[protocol-account-benefits-package]] for the account's real ceiling:
today the fake grid hard-codes `agent_access_max = "A"`, so every account is
maximally entitled and a refusal cannot be provoked at all.

Acceptance: an account whose ceiling forbids a region is refused with the
family member the reference viewer would raise, accepting the offered change
re-sends the preference through the existing conversation, and a fake-grid case
provokes the refusal without a live grid.
