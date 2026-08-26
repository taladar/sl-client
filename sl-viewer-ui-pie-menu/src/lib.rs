//! The viewer's radial (**pie**) menu widget.
//!
//! It is its own crate rather than one more module of `sl-viewer-ui-widgets`
//! because it is the one widget in that crate with a *single* consumer. Twelve
//! crates read the widget vocabulary — floaters, text inputs, tabs, tables — and
//! recompile when any of it changes; only the composition root ever spawns a pie.
//! Leaving 3.4k lines of it in the shared crate charged every one of those twelve
//! for an edit none of them can see. It names nothing in `ui-widgets` and
//! `ui-widgets` names nothing here, so the two are siblings over
//! `sl-viewer-ui-core` and compile in parallel.
//!
//! Everything the widget is, and the angular-stability invariant it exists to
//! keep, is documented on [`pie_menu`].

#![expect(
    clippy::module_name_repetitions,
    reason = "the module owns one widget and is named for it, so its types read \
              as `pie_menu::PieMenuDef`. That only became a lint when these items \
              turned `pub` for the crate split; renaming them would churn every \
              call site in the viewer to satisfy a style rule this codebase does \
              not follow"
)]

// The layout harness, under the name the widget's test module already uses. It
// is a sibling crate rather than a module here because the binary tests against
// it too, and it must not depend on the widgets it is used to test.
#[cfg(test)]
pub use sl_viewer_testkit as ui_test;

pub mod pie_menu;
