---
id: viewer-conversations-session-restore
title: Restore IM sessions at login + IM↔nearby routing options
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-social-im-conversations, viewer-conversation-log,
       viewer-offline-im-drain, viewer-chat-transcript-style-options]
---

Context: [context/viewer.md](../context/viewer.md).

Conversation-session continuity options for our conversations UI
(done [[viewer-social-im-conversations]]): reopen the IM sessions that
were open at last logout (`FSRestoreOpenIMs`, with the session list
persisted per-account in `FSLastOpenIMs`), and open the conversations
window automatically at login when offline messages arrived
(`FSOpenIMContainerOnOfflineMessage` — pairs with the done
[[viewer-offline-im-drain]]).

The second half is IM↔nearby routing: show incoming IMs in the nearby
chat transcript (`FSShowIMInChatHistory`) and log them there
(`FSLogIMInChatHistory`). The FS-features auditor flagged the same
family from the other end — `fsconsoleutils.cpp` routes chat *and* IMs
into the legacy transparent text console when `FSUseNearbyChatConsole`
is set; our nearby-chat overlay is the implemented equivalent of the
console itself, but the "route IMs into the nearby display" preference
has no home in our tree. Related presentation knobs — IM-tab ordering
(`FSAutoOrderIMTabs*`) and tab-name format (`FSIMTabNameFormat`) —
only apply where a tabbed layout exists; broader transcript styling
lives in [[viewer-chat-transcript-style-options]].

Reference (Firestorm, read-only):
`indra/newview/fsfloaterimcontainer.cpp`,
`indra/newview/fsfloaterim.cpp`, `indra/newview/fsconsoleutils.cpp`,
`indra/newview/app_settings/settings_per_account.xml`
(FSLastOpenIMs).
