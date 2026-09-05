//! The viewer's widgets: the things a panel is actually made of.
//!
//! This is the layer above the vocabulary in `sl-viewer-ui-core` and below every
//! feature that uses one. A widget here knows how to lay itself out, take focus,
//! read and write a setting, and report what the user did — and knows nothing
//! about avatars, parcels, inventory or the grid.
//!
//! - [`floater`], [`floater_persist`] — the window abstraction and the geometry
//!   it remembers between sessions.
//! - [`menu`] — the menu bar's dropdowns and the line-based context menu, and
//!   [`menu_accel`] — the accelerator each entry draws, dispatched to it.
//! - [`ui_text_input`], [`ui_search`], [`ui_combo`], [`ui_radio`],
//!   [`ui_color_picker`] — the input controls.
//! - [`ui_tab`], [`ui_table`] — tab strips and the sortable, virtualized table.
//! - [`settings_binding`] — the two-way binding that makes a control read and
//!   write a settings-store value directly.
//!
//! # Two widgets that used to be here, and why they are not
//!
//! Everything above is named by five to thirteen crates, which is what makes it
//! a shared vocabulary worth one crate. Two modules were not, and every consumer
//! above was paying to recompile them for edits none of them could see:
//!
//! - the **radial menu** is now `sl-viewer-ui-pie-menu`, a *sibling* crate over
//!   `sl-viewer-ui-core` — 3.4k lines that only the composition root ever spawns,
//!   and that named nothing here;
//! - the **`:shortcode:` completion popup** is now `sl_viewer_chat::emoji_complete`,
//!   beside the one text field that attaches it.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one widget and is named for it, so its types read \
              as `ui_tab::UiTab` and `floater::FloaterSpec`. That only became a \
              lint when these items turned `pub` for the crate split; renaming \
              them would churn every call site in the viewer to satisfy a style \
              rule this codebase does not follow"
)]

// The layout harness, under the name the widgets' test modules already use. It
// is a sibling crate rather than a module here because the binary tests against
// it too, and it must not depend on the widgets it is used to test.
#[cfg(test)]
pub use sl_viewer_testkit as ui_test;

pub mod floater;
pub mod floater_persist;
pub mod menu;
pub mod menu_accel;
pub mod settings_binding;
pub mod ui_color_picker;
pub mod ui_combo;
pub mod ui_radio;
pub mod ui_search;
pub mod ui_tab;
pub mod ui_table;
pub mod ui_text_input;
