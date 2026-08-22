//! The gallery's element registry: every panel and widget the UI gallery and
//! the interaction-contract tests can spawn, named once.
//!
//! This table lives in the binary crate rather than beside the widget types it
//! spawns, for the same reason `REGISTRARS` does: it names three dozen feature
//! modules, and a crate holding it would have to depend on all of them. The
//! composition root already does.
//!
//! **A new panel or widget belongs in [`ELEMENTS`].** That is the whole point
//! of the registry — the gallery renders what is listed here and nothing
//! hand-picked, so an element added here is covered by the gallery sweep and
//! the contract tests without touching either.

use crate::ui_element::UiElement;

/// **The registry.** Every UI element, checked by [`crate::ui_test`] and
/// rendered by [`crate::gallery`].
///
/// Add a panel or widget here and it inherits the whole suite. See the [module
/// documentation](self).
pub(crate) const ELEMENTS: &[UiElement] = &[
    UiElement {
        id: "label",
        summary: "A text label in a decorated container — the pattern a text run must be \
                  wrapped in.",
        spawn: crate::ui_element::spawn_label,
    },
    UiElement {
        id: "button",
        summary: "A focusable `bevy_ui_widgets` button that emits a `UiAction` rather than \
                  doing anything.",
        spawn: crate::ui_element::spawn_button,
    },
    UiElement {
        id: "button-row",
        summary: "Three buttons flowing in text order, wrapping when they outgrow the panel.",
        spawn: crate::ui_element::spawn_button_row,
    },
    UiElement {
        id: "field-grid",
        summary: "The build window's shape: rows of X/Y/Z fields whose columns must align \
                  however the row labels are translated.",
        spawn: crate::ui_element::spawn_field_grid,
    },
    UiElement {
        id: "text-editor",
        summary: "A multi-line `EditableText` with a caret, reachable by `Tab`.",
        spawn: crate::ui_element::spawn_text_editor,
    },
    UiElement {
        id: "text-input-line",
        summary: "The reusable single-line text field (`crate::ui_text_input`): a bordered, \
                  glyph-width-sized `EditableText` that scrolls its content horizontally.",
        spawn: crate::ui_text_input::spawn_line_specimen,
    },
    UiElement {
        id: "text-input-multiline",
        summary: "The reusable multi-line text field: newlines allowed, prose soft-wraps at a \
                  bound and scrolls vertically.",
        spawn: crate::ui_text_input::spawn_multiline_specimen,
    },
    UiElement {
        id: "text-input-float",
        summary: "The signed-decimal numeric field: accepts an optional `-`, digits and one `.`; \
                  a bad character is rejected as typed and a bad arrangement reverted.",
        spawn: crate::ui_text_input::spawn_float_specimen,
    },
    UiElement {
        id: "text-input-integer",
        summary: "The signed-integer numeric field: an optional `-` then digits.",
        spawn: crate::ui_text_input::spawn_integer_specimen,
    },
    UiElement {
        id: "text-input-unsigned",
        summary: "The non-negative-integer numeric field: digits only, the sign key rejected.",
        spawn: crate::ui_text_input::spawn_unsigned_specimen,
    },
    UiElement {
        id: "build-tools",
        summary: "The Build Tools panel shape (viewer-object-edit-floater-shell): the tool-mode \
                  radio group, a snap toggle row, and a numeric transform row of float fields.",
        spawn: crate::edit_tool::spawn_build_tools_specimen,
    },
    UiElement {
        id: "build-create",
        summary: "The Create tool panel (viewer-prim-creation): the base-type radio (the seven \
                  prim volume types plus Tree and Grass), a species combo, and the build hint.",
        spawn: crate::edit_create::spawn_create_panel_specimen,
    },
    UiElement {
        id: "notecard-editor",
        summary: "The notecard editor's content (viewer-notecard-editor): a view toggle, an \
                  editable multi-line body, and a Save button. The live floater \
                  (`crate::edit_notecard`) fetches and saves the asset; here it is static so its \
                  layout is swept.",
        spawn: crate::edit_notecard::spawn_notecard_editor_specimen,
    },
    UiElement {
        id: "notecard-reader",
        summary: "The notecard rich read-only reader (viewer-notecard-editor / \
                  crate::notecard_render): prose with a linkified URL and an inline clickable \
                  embedded item (a landmark). Shown static so the interleaved run / item-box \
                  layout is swept.",
        spawn: crate::edit_notecard::spawn_notecard_reader_specimen,
    },
    UiElement {
        id: "script-editor",
        summary: "The LSL script editor's content (viewer-lsl-editor-save-compile): an editable \
                  multi-line body, a Running toggle, a Save & Compile button, and a sample \
                  compile-diagnostic row. The live floater (`crate::edit_script`) fetches, \
                  uploads and compiles; here it is static so its layout is swept.",
        spawn: crate::edit_script::spawn_script_editor_specimen,
    },
    UiElement {
        id: "search-field",
        summary: "The reusable search-field widget (`crate::ui_search`): a single-line field in a \
                  bordered box with a leading search glyph, a placeholder shown while empty, and a \
                  trailing × clear button shown only while it holds a term. The menu-bar and \
                  inventory search boxes are two live consumers.",
        spawn: crate::ui_search::spawn_search_specimen,
    },
    UiElement {
        id: "panel",
        summary: "A composite: a titled panel with prose and a button row, bounded but not \
                  sized.",
        spawn: crate::ui_element::spawn_panel,
    },
    UiElement {
        id: "radial-menu-target",
        summary: "Right-click to open a live pie menu under the pointer. The pie is opened, used \
                  and dismissed one at a time — never a persistent card — so this is its \
                  registered form; its layout is checked directly in `crate::pie_menu`'s tests.",
        spawn: crate::pie_menu::spawn_radial_menu_target,
    },
    UiElement {
        id: "inventory-row",
        summary: "An inventory tree row: indent, expand arrow, type icon and label — an expanded \
                  folder over an indented item. The live window (`crate::inventory`) recycles \
                  this row through the virtualized list; here it is static so its layout is swept.",
        spawn: crate::inventory::spawn_inventory_row_sample,
    },
    UiElement {
        id: "floater",
        summary: "A floating window's chrome: a title bar with dock / minimize / close buttons, a \
                  content slot, and a resize grip. The live manager (`crate::floater`) makes it \
                  draggable and dockable; here it is static so its layout is swept.",
        spawn: crate::floater::spawn_floater_specimen,
    },
    UiElement {
        id: "tabs-top",
        summary: "A tabbed container with the tab strip on the top edge — three tabs fronting three \
                  panels, one shown. The reusable widget (`crate::ui_tab`); one element per \
                  placement so every orientation is swept.",
        spawn: crate::ui_tab::spawn_tabs_block_start,
    },
    UiElement {
        id: "tabs-bottom",
        summary: "The tab widget with its strip on the bottom edge — a block-axis placement, which \
                  stays bottom under RTL (only the inline axis mirrors).",
        spawn: crate::ui_tab::spawn_tabs_block_end,
    },
    UiElement {
        id: "tabs-leading",
        summary: "The tab widget with a vertical strip on the leading edge (left under LTR); it \
                  mirrors to the right under RTL with no separate code.",
        spawn: crate::ui_tab::spawn_tabs_inline_start,
    },
    UiElement {
        id: "preferences",
        summary: "The preferences shell: a search box, a leading (RTL-mirroring) tab strip, \
                  labelled setting rows and an OK / Cancel footer. Static — the live shell \
                  (`crate::preferences`) adds the store binding, filter and snapshot/revert.",
        spawn: crate::preferences::spawn_preferences_specimen,
    },
    UiElement {
        id: "debug-settings",
        summary: "The raw debug-settings editor: a search box over a changed-marker settings \
                  list beside the detail column — per-layer value read-outs, a scope combo, a \
                  numeric editor and the copy / reset buttons. Static — the live floater \
                  (`crate::debug_settings`) adds the store, the table and the commit paths.",
        spawn: crate::debug_settings::spawn_debug_settings_specimen,
    },
    UiElement {
        id: "quick-preferences",
        summary: "The Quick Preferences panel: the environment preset / time-of-day combos over \
                  a divider and the curated setting slider rows (draw distance, particle cap). \
                  Static — the live panel (`crate::quick_preferences`) adds the store binding and \
                  the environment wiring.",
        spawn: crate::quick_preferences::spawn_quick_prefs_specimen,
    },
    UiElement {
        id: "tabs-trailing",
        summary: "The tab widget with a vertical strip on the trailing edge (right under LTR) — a \
                  placement the reference viewer cannot express, usable for LTR too, not only as \
                  an RTL mirror.",
        spawn: crate::ui_tab::spawn_tabs_inline_end,
    },
    UiElement {
        id: "radio-group-row",
        summary: "A radio-button group flowing along the inline axis (`crate::ui_radio`): \
                  mutually-exclusive options with a filled-dot indicator, one selected, the group \
                  the single focus stop. The Build Tools mode switch is a live consumer; here the \
                  horizontal layout is swept, wrapping and mirroring under RTL.",
        spawn: crate::ui_radio::spawn_radio_row,
    },
    UiElement {
        id: "radio-group-column",
        summary: "The radio-button group stacked down the block axis — the reference viewer's usual \
                  radio shape. Same widget as `radio-group-row`, the vertical layout swept.",
        spawn: crate::ui_radio::spawn_radio_column,
    },
    UiElement {
        id: "combo-box",
        summary: "A combo / dropdown (`crate::ui_combo`): a bordered value button that opens a \
                  popover list of options and emits the chosen one. The Texture tab's bumpiness / \
                  shininess / mapping controls are live consumers; swept closed here.",
        spawn: crate::ui_combo::spawn_combo_element,
    },
    UiElement {
        id: "menu-bar",
        summary: "A closed menu bar (`crate::menu`): a strip of pull-down buttons whose drop-downs \
                  open on click — command / check / disabled entries, separators, accelerators and \
                  a submenu. Swept closed; its opened drop-down layout is checked in the module's \
                  own tests, and it is drivable live by the gallery's right-click menu toggle.",
        spawn: crate::menu::spawn_menu_bar_specimen,
    },
    UiElement {
        id: "bottom-toolbar",
        summary: "The persistent bottom toolbar (`crate::bottom_toolbar`): a row of floater-toggle \
                  buttons in an enabled, an active (lit) and a disabled placeholder state. The live \
                  bar (bottom-anchored, wrapping upward) toggles the main floaters; here it is \
                  static so all three button states' layouts are swept.",
        spawn: crate::bottom_toolbar::spawn_bottom_toolbar_specimen,
    },
    UiElement {
        id: "notification-toast",
        summary: "A toast card from the notification host (viewer-ui-notification-host): an \
                  accent-bordered panel with a wrapping message, an OK / Cancel button row and \
                  the \"don't show me this again\" checkbox. The live host \
                  (`crate::notification_host`) stacks, times out and dismisses these in a corner \
                  channel; here it is static so its layout is swept.",
        spawn: crate::notification_host::spawn_notification_specimen,
    },
    UiElement {
        id: "group-notice-toast",
        summary: "A group-notice card (viewer-group-notice-display): the group image, a \"Group \
                  Notice\" header, the \"Sent by …\" title, the subject / SLT date / body, an \
                  attached item row, and the OK / Group Notices / Group Chat actions. The live \
                  host (`crate::group_notice`) pops one per received notice; here it is static so \
                  its layout is swept.",
        spawn: crate::group_notice::spawn_group_notice_specimen,
    },
    UiElement {
        id: "script-dialog-toast",
        summary: "A script-dialog card (viewer-dialog-lldialog): an object / owner title, the \
                  dialog message, and a three-column button grid filled bottom-up (button 0 \
                  bottom-left), plus the Block / Ignore actions. The live host \
                  (`crate::script_dialog`) pops one per received llDialog; here it is static so \
                  the grid layout is swept.",
        spawn: crate::script_dialog::spawn_script_dialog_specimen,
    },
    UiElement {
        id: "script-dialog-textbox-toast",
        summary: "An llTextBox script-dialog card (viewer-dialog-lldialog): the object / owner \
                  title, the prompt, a text-entry field and a Submit / Block / Ignore row. The \
                  live host (`crate::script_dialog`) pops one per received llTextBox; here it is \
                  static so the text-prompt layout is swept.",
        spawn: crate::script_dialog::spawn_script_textbox_specimen,
    },
    UiElement {
        id: "linkified-text",
        summary: "A run of text with its http(s) URL, labelled [url text] link and location \
                  SLURL turned into coloured, hoverable, clickable spans \
                  (viewer-url-linkification). The shared decoration layer every text-bearing \
                  panel consumes; here a static sample sweeps the segment wrapping / link tint.",
        spawn: crate::linkified_text::spawn_linkified_text_specimen,
    },
    UiElement {
        id: "load-url-toast",
        summary: "A script web-page request card (viewer-dialog-script-load-url): the \"Open a \
                  web page?\" heading, the object / owner title, the script message, the target \
                  URL, and the Load / Block / Ignore actions. The live host (`crate::load_url`) \
                  pops one per received llLoadURL (LoadURL message); here it is static so the \
                  layout is swept.",
        spawn: crate::load_url::spawn_load_url_specimen,
    },
    UiElement {
        id: "script-permission-toast",
        summary: "A script permission-request card (viewer-permission-request-dialog): the \
                  object / owner intro, a bulleted line per requested permission, \"Is this \
                  OK?\", and the Yes / No / Block actions. The live host \
                  (`crate::script_permission`) pops one per received llRequestPermissions \
                  (ScriptQuestion message); here it is static so the layout is swept.",
        spawn: crate::script_permission::spawn_script_permission_specimen,
    },
    UiElement {
        id: "script-permission-caution-toast",
        summary: "The money-access caution card (viewer-permission-request-dialog): the \
                  ScriptQuestionCaution warning shown when a script asks to debit L$, the \
                  \"also requesting\" list, and the Allow access / Deny actions. The live host \
                  (`crate::script_permission`) pops one per debit request; here it is static \
                  so the layout is swept.",
        spawn: crate::script_permission::spawn_script_permission_caution_specimen,
    },
    UiElement {
        id: "experience-permission-toast",
        summary: "The experience-acceptance card (viewer-experience-permission-dialog): the \
                  ScriptQuestionExperience object / owner / scope intro, the experience name, the \
                  remembered-until-revoked note, the bulleted permission lines, \"Is this OK?\", \
                  and the Yes / No / Block Experience / Block Object actions. The live host \
                  (`crate::experience_permission`) pops one per received experience \
                  ScriptQuestion; here it is static so the layout is swept.",
        spawn: crate::experience_permission::spawn_experience_specimen,
    },
    UiElement {
        id: "experiences-floater",
        summary: "The Experiences manage surface (viewer-experience-permission-dialog): the \
                  Allowed / Blocked headed lists, each row an experience name with a Forget \
                  button. The live floater (`crate::experiences_floater`) fills the lists from \
                  the GetExperiences reply; here it is static so the layout is swept.",
        spawn: crate::experiences_floater::spawn_experiences_specimen,
    },
    UiElement {
        id: "inventory-offer-toast",
        summary: "An inventory-offer card (viewer-dialog-offers-invites): the gift heading, the \
                  \"{giver} has given you an item\" lead, the item name, and the Accept / Decline \
                  / Block actions. The live host (`crate::offers_invites`) pops one per received \
                  inventory-offer IM; here it is static so the layout is swept.",
        spawn: crate::offers_invites::spawn_inventory_offer_specimen,
    },
    UiElement {
        id: "teleport-offer-toast",
        summary: "A teleport-offer / lure card (viewer-dialog-offers-invites): the location \
                  heading, the \"{offerer} has offered to teleport you\" lead, the offer message, \
                  and the Teleport / Decline actions. The live host (`crate::offers_invites`) \
                  pops one per received lure IM; here it is static so the layout is swept.",
        spawn: crate::offers_invites::spawn_teleport_offer_specimen,
    },
    UiElement {
        id: "friendship-offer-toast",
        summary: "A friendship-offer card (viewer-dialog-offers-invites): the handshake heading, \
                  the \"{agent} is offering to be your friend\" lead, any custom message, and the \
                  Accept / Decline actions. The live host (`crate::offers_invites`) pops one per \
                  received friendship-offer IM; here it is static so the layout is swept.",
        spawn: crate::offers_invites::spawn_friendship_offer_specimen,
    },
    UiElement {
        id: "group-invite-toast",
        summary: "A group-membership invitation card (viewer-dialog-offers-invites): the people \
                  heading, the \"{inviter} has invited you to join a group\" lead, the invite \
                  message and any membership fee, and the Join / Decline actions. The live host \
                  (`crate::offers_invites`) pops one per received group-invitation IM; here it is \
                  static so the layout is swept.",
        spawn: crate::offers_invites::spawn_group_invite_specimen,
    },
    UiElement {
        id: "minimap",
        summary: "The minimap surface (`crate::minimap`): terrain-ish backdrop, a parcel line, \
                  avatar dots and the compass labels. The live floater composites a CPU image \
                  from the world mirror; here it is static so its layout is swept.",
        spawn: crate::minimap::spawn_minimap_specimen,
    },
    UiElement {
        id: "radar",
        summary: "The avatar radar's content (`crate::radar`): the counts line, the nearby-avatar \
                  table with its status glyphs and band-coloured ranges, and the action buttons. \
                  The live floater binds a virtualized table off the radar model; here it is \
                  static so its layout is swept.",
        spawn: crate::radar::spawn_radar_specimen,
    },
    UiElement {
        id: "worldmap",
        summary: "The world-map floater's layout (`crate::world_map`): a tile-ish map surface \
                  with region fills and markers beside the search side panel with result rows. \
                  The live floater composites grid tiles and live markers into a CPU image; \
                  here it is static so its layout is swept.",
        spawn: crate::world_map::spawn_world_map_specimen,
    },
    UiElement {
        id: "parcel-audio-bar",
        summary: "The parcel streaming-audio cluster (`crate::parcel_audio`): the ♫ marker, a \
                  width-capped now-playing title, play and mute glyph buttons and the volume \
                  slider. The live cluster (trailing side of the bottom area) follows the \
                  agent's parcel stream; here it is static so its layout is swept.",
        spawn: crate::parcel_audio::spawn_parcel_audio_specimen,
    },
    UiElement {
        id: "emoji-picker",
        summary: "The emoji-picker floater's novel layout (`crate::emoji_picker`): a couple of grid \
                  rows of glyphs, the skin-tone swatch row and the preview line. The live floater \
                  (`Ctrl+E`) filters, groups and inserts a chosen glyph into the focused field; \
                  here it is static so its layout is swept.",
        spawn: crate::emoji_picker::spawn_emoji_picker_specimen,
    },
    UiElement {
        id: "chat-input",
        summary: "The reusable chat-input widget (`crate::chat_input`): a single-line field in a \
                  bordered box with a trailing emoji button and an inline `:`-completer. The live \
                  widget opens the picker for its field and sends on Enter; here it is static so \
                  the bar layout is swept.",
        spawn: crate::chat_input::spawn_chat_input_specimen,
    },
    UiElement {
        id: "local-chat-input",
        summary: "The reusable local-chat-input widget (`crate::local_chat_input`): the chat input \
                  plus a whisper/say/shout select box. The live widget parses `/N` channels and \
                  `/command`s and maps Shift/Ctrl+Enter to whisper/shout; here it is static so the \
                  bar layout is swept.",
        spawn: crate::local_chat_input::spawn_local_chat_input_specimen,
    },
    UiElement {
        id: "browser-view",
        summary: "The embedded-browser view (`crate::browser_widget`): a surface-backed image \
                  node with click-to-focus input routing. In the gallery the web-media engine is \
                  live and renders an offline data-URL page; in a headless test it stays the \
                  dark placeholder.",
        spawn: crate::browser_widget::spawn_browser_specimen,
    },
];

#[cfg(test)]
mod tests {
    use super::ELEMENTS;
    use crate::ui_element::SCRIPTS;
    use pretty_assertions::assert_eq;

    /// Ids are what a failing check names and what the gallery lists, so a
    /// duplicate would make one element's failure indistinguishable from
    /// another's.
    #[test]
    fn element_ids_are_unique() {
        let mut ids: Vec<&str> = ELEMENTS.iter().map(|element| element.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "two registered elements share an id");
    }

    /// The registry has to be non-empty, or every matrix test below passes by
    /// iterating over nothing — the failure mode where a green suite means the
    /// suite ran out of work rather than found none.
    #[test]
    fn the_registry_is_not_empty() {
        assert!(!ELEMENTS.is_empty(), "no UI elements are registered");
        assert!(!SCRIPTS.is_empty(), "no scripts are registered");
    }
}
