---
id: viewer-help-onboarding-misc
title: Onboarding/help odds-and-ends
topic: viewer
status: ideas
origin: main-menu survey (2026-07-23)
refs: [viewer-report-abuse, viewer-login-screen, viewer-notification-history]
---

Context: [context/viewer.md](../context/viewer.md).

A grab bag of small Help/Content-menu features with no coverage, each
needing individual scoping before promotion:

- **Viewer UI hints** (`ToggleUIHints`) + **Guidebook** floater
  (`Help.ToggleHowTo`): onboarding hint bubbles and the how-to guide.
- **Whitelist adviser** (`fs_whitelist_floater`): FS's
  antivirus-exclusion adviser — likely minimal relevance on Linux;
  evaluate before adopting.
- **Report Problem / Bug** flow (`Advanced.ReportBug`) — distinct from
  abuse reporting ([[viewer-report-abuse]]); on FS this opens the JIRA
  flow, for us likely a GitHub-issue link with prefilled sysinfo.
- **Sysinfo button in IM** (`SysinfoButtonInIM`): paste system info into
  a support conversation.
- **MOTD overlay toggle** (`Advanced.ToggleHUDInfo motd`): show the
  grid message-of-the-day on screen after login
  ([[viewer-login-screen]] shows it at login only).

Reference (Firestorm, read-only): `menu_viewer.xml` Help/Content
sections, `menu_login.xml`.

## Parity-audit addendum (2026-08-19)

The parity audit folds in the external SL web-link menu-entry family
(~15 entries, all "open URL in embedded/external browser" one-liners
once a web-launch helper exists — deliberately excluding the
Firestorm-project self-reference links): Avatar ▸ **Account** plus
the **[Membership]** entry; Content ▸ **SL Marketplace, L$ Market
Data, Script
Library, SL Community**; World ▸ **Events** (the secondlife.com events
web page); Help ▸ **Second Life Help, Tutorial, Knowledge Base, Wiki,
Community Forums, Support portal, [SECOND_LIFE] Blogs**; and the
grid-configurable **[CURRENT_GRID] Help / About [CURRENT_GRID]** pair
(OpenSim grid-info URLs).
