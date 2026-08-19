---
id: viewer-url-context-menus
title: Right-click context menus on linkified text and names
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-url-linkification, viewer-inspector-popups,
  viewer-minimap-menu-avatar-actions, viewer-contact-sets,
  viewer-conversation-log, viewer-chat-mention-autocomplete]
---

Context: [context/viewer.md](../context/viewer.md).

The reference attaches a per-kind context menu to every linkified span
(twelve menu_url_*.xml files plus menu_slurl.xml, menu_avatar_icon.xml,
menu_object_icon.xml, and the menu_fs_namelist_avatar*.xml name-list
menus): agent links get View Profile / IM / Add Friend / teleport verbs
/ Copy Name / Copy Url / Copy Mention URI; group links get Show Info /
(De)Activate / Join / Leave / Copy Group / Copy SLurl; objectim links
get Object Profile / Block / Show on Map / Teleport to / Copy; place,
parcel, map and teleport SLURLs get Show Info / Show on Map / Teleport /
Copy SLurl; experience links Copy SLurl; inventory links Show Item /
Copy Name / Copy SLurl; plain http links Open / Open in Internal or
External Browser / Copy URL; email Compose / Copy; slapp Run This
Command / Copy.

Our left-click dispatch of linkified text is done (`url_linkify.rs`,
`linkified_text.rs`, `slurl_dispatch.rs`, `ui_name_link.rs`;
[[viewer-url-linkification]] and the inspector popups with their
Profile / IM / Add Friend / Offer TP / Block / Show-on-map buttons,
[[viewer-inspector-popups]]), but right-clicking a link or name does
nothing — none of the four modules builds a context menu.

Scope: one declarative menu per `url_linkify.rs` link kind over the
existing line-menu widget, routing actions to the shared
avatar/group/place action layer ([[viewer-minimap-menu-avatar-actions]]
owns the shared avatar verbs); the many copy-to-clipboard verbs (name /
URL / UUID / SLurl / mention URI) land here via `clipboard.rs`, and the
mention URI ties into [[viewer-chat-mention-autocomplete]]. Backends not
yet implemented sit greyed in reference positions, per house pattern.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/menu_url_agent.xml` through
`menu_url_teleport.xml`, `menu_slurl.xml`, `menu_avatar_icon.xml`,
`menu_object_icon.xml`, `menu_fs_namelist_avatar.xml`;
`indra/llui/llurlaction.cpp`, `indra/newview/llchathistory.cpp`
(context-menu hookup).
