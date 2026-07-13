---
id: viewer-social-panels
title: People / friends / groups / profiles / IM UI
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The social surface, several related panels: the **people** panel (friends /
nearby / recent / blocked), the **groups** list plus group profile / roles /
notices, avatar **profiles** (picks / classifieds), and the **IM /
conversation** UI including chat **input** and group chat.

Local-chat receive and IM already have protocol support (`protocol-2` IM,
`chat.rs` overlay); this stub adds the interactive panels and, notably, chat
**input** (typing to local/IM/group), which the MVP viewer deferred.

Reference (Firestorm, read-only): `llpanelpeople`, `llavatarlist`,
`llfloaterprofile`, `llpanelgroup*`, `llgroupmgr`, `llimview`,
`llfloaterimsession`, `llconversationview`, `fsfloaternearbychat`.

Builds on: `protocol-2` IM and the `chat.rs` overlay. Supersedes the MVP "no
chat input" non-goal.

Deps: [[viewer-ui-framework]].
