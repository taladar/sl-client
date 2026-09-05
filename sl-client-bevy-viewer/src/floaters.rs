//! The **floater registry**: every window the viewer opens, named once, so a
//! sweep and the gallery can reach them.
//!
//! The floater half of [`crate::ui_elements`], and it lives here for the same
//! reason that one does: it names three dozen feature modules, and a crate
//! holding it would have to depend on all of them. The composition root already
//! does.
//!
//! **A new floater belongs in [`FLOATERS`].** Registering it costs one entry
//! and buys the window the whole layout matrix — every script, both directions,
//! three font sizes, three scale factors, three UI scales — plus a card in the
//! gallery a person can drag, resize, minimize and close by hand. Not
//! registering it means no check in this repository has ever seen the window,
//! and none ever will until somebody remembers to write one by hand.
//!
//! # Why every entry calls the viewer's own spec function
//!
//! An entry does not restate its window's geometry; it points at the
//! [`FloaterSpec`](crate::floater::FloaterSpec) constructor the module's own
//! `Startup` system calls. Two callers, one source — see
//! [`FloaterElement`](crate::floater::FloaterElement) for the argument, and
//! `every_floater_spec_in_the_workspace_is_registered` below for the guard that
//! notices a constructor nobody registered.
//!
//! # Specimen or stub
//!
//! A floater's chrome is always constructible; its *content* is often a live
//! view of a session — an inventory tree, a profile, a parcel. Where the module
//! has already written the static specimen of that content for [`ELEMENTS`],
//! the entry reuses it and the sweep measures the window around the real
//! layout. The rest carry a [`FloaterContent::Stub`] line: the chrome is still
//! held to account — a title that runs long in Arabic, a glyph clipped at 22 px,
//! a default rect that does not fit — and the stub is the obvious, greppable
//! place for the specimen when somebody writes one.
//!
//! [`ELEMENTS`]: crate::ui_elements::ELEMENTS

use crate::floater::{FloaterContent, FloaterElement};

/// **The registry.** Every floater the viewer opens, swept by
/// [`crate::ui_test`] and rendered by [`crate::gallery`].
///
/// In id order, so a duplicate or a gap is visible by reading. See the [module
/// documentation](self).
pub(crate) const FLOATERS: &[FloaterElement] = &[
    FloaterElement {
        id: "about",
        summary: "About: viewer version, the connected grid and region, system information as one \
                  copyable support block, credits and the third-party licenses.",
        spec: crate::about_floater::about_floater_spec,
        content: FloaterContent::Stub(
            "The Info / Credits / Licenses tabs, filled from the build info and the live session.",
        ),
    },
    FloaterElement {
        id: "about-land",
        summary: "About Land: the parcel's General / Covenant / Objects / Options / Media / Access \
                  tabs — the parcel the agent is standing in, or one picked on the map.",
        spec: crate::about_land::about_land_floater_spec,
        content: FloaterContent::Stub(
            "The parcel's tabs, filled from the ParcelProperties reply for the parcel under the \
             agent.",
        ),
    },
    FloaterElement {
        id: "about-landmark",
        summary: "About Landmark: a landmark item's name, its resolved region and position, and \
                  the Teleport / Show on Map actions.",
        spec: crate::about_landmark::about_landmark_floater_spec,
        content: FloaterContent::Stub(
            "The landmark's name, its resolved region and position, and the teleport actions.",
        ),
    },
    FloaterElement {
        id: "about-region",
        summary: "Region / Estate: the region's Region / Debug / Terrain / Estate / Covenant tabs \
                  — the estate-owner surface behind the region the agent is in.",
        spec: crate::about_region::about_region_floater_spec,
        content: FloaterContent::Stub(
            "The region and estate tabs, filled from the RegionInfo and EstateOwner replies.",
        ),
    },
    FloaterElement {
        id: "add-to-contact-set",
        summary: "Add to Contact Set: the prompt, the set combo and the Add / New Set… / Cancel \
                  row — opened on a chosen resident.",
        spec: crate::contact_sets_panel::add_to_set_floater_spec,
        content: FloaterContent::Stub(
            "The prompt naming the resident, the set combo, and the Add / New Set… / Cancel row.",
        ),
    },
    FloaterElement {
        id: "asset-blacklist",
        summary: "Asset Blacklist: the filter row over the sortable table of blacklisted assets, \
                  with its count line and the remove actions.",
        spec: crate::asset_blacklist::blacklist_floater_spec,
        content: FloaterContent::Stub(
            "The filter row, the sortable blacklist table with its count line, and the remove \
             actions.",
        ),
    },
    FloaterElement {
        id: "avatar-picker",
        summary: "Choose Resident: the name search field over its result list, and the OK / Cancel \
                  reply row every consumer is written against.",
        spec: crate::avatar_picker::avatar_picker_floater_spec,
        content: FloaterContent::Stub(
            "The name search field, the result list filled from the directory reply, and the \
             OK / Cancel row.",
        ),
    },
    FloaterElement {
        id: "avatar-profile",
        summary: "Profile: a resident's Second Life / Web / Interests / Picks / Classifieds / \
                  Notes tabs. Subject-bound, so its geometry is not persisted.",
        spec: crate::avatar_profile::avatar_profile_floater_spec,
        content: FloaterContent::Stub(
            "The six profile tabs, rebuilt per open from the subject's profile replies.",
        ),
    },
    FloaterElement {
        id: "avatar-render-settings",
        summary: "Avatar Render Settings: the filter row over the per-avatar render-override \
                  table, with its count line and the trailing actions.",
        spec: crate::avatar_render_floater::render_settings_floater_spec,
        content: FloaterContent::Stub(
            "The filter row, the per-avatar render-override table with its count line, and the \
             trailing actions.",
        ),
    },
    FloaterElement {
        id: "block-by-name",
        summary: "Block Object by Name: the name field and the Block / Cancel row — the by-name \
                  half of the mute list, which has no picker to open.",
        spec: crate::blocked::block_by_name_floater_spec,
        content: FloaterContent::Stub("The object-name field and the Block / Cancel row."),
    },
    FloaterElement {
        id: "build-tools",
        summary: "Build Tools: the tool-mode radio group, the snap toggles and the numeric \
                  transform rows — the object-edit surface, swept here inside its own window.",
        spec: crate::edit_tool::build_tools_floater_spec,
        content: FloaterContent::Specimen(crate::edit_tool::spawn_build_tools_specimen),
    },
    FloaterElement {
        id: "color-picker",
        summary: "Color Picker: the shared swatch-driven picker — a saturation/value field, the \
                  channel sliders and the reply row. Subject-bound to whatever swatch opened it.",
        spec: crate::ui_color_picker::color_picker_floater_spec,
        content: FloaterContent::Stub(
            "The saturation / value field, the hue and alpha tracks, the channel fields and the \
             OK / Cancel reply row.",
        ),
    },
    FloaterElement {
        id: "contact-set-config",
        summary: "Contact Set Settings: the set's name field with its Rename button, the colour \
                  swatch, and Close.",
        spec: crate::contact_sets_panel::contact_set_config_floater_spec,
        content: FloaterContent::Stub(
            "The set's name field with its Rename button, the colour swatch, and Close.",
        ),
    },
    FloaterElement {
        id: "conversations",
        summary: "Conversations: the conversation strip beside the transcript pane, split by a \
                  draggable divider. Docks into its own host beside the nearby-chat bar.",
        spec: crate::conversations::conversations_floater_spec,
        content: FloaterContent::Stub(
            "The conversation strip, the divider and the transcript pane, seeded with the Nearby \
             view.",
        ),
    },
    FloaterElement {
        id: "debug_settings",
        summary: "Debug settings: the raw settings editor — a search box over the changed-marker \
                  list beside the per-layer detail column.",
        spec: crate::debug_settings::debug_settings_floater_spec,
        content: FloaterContent::Specimen(crate::debug_settings::spawn_debug_settings_specimen),
    },
    FloaterElement {
        id: "emoji-picker",
        summary: "Emoji: the grouped glyph grid, the skin-tone swatch row and the preview line, \
                  opened by `Ctrl+E` for the focused field.",
        spec: crate::emoji_picker::emoji_picker_floater_spec,
        content: FloaterContent::Specimen(crate::emoji_picker::spawn_emoji_picker_specimen),
    },
    FloaterElement {
        id: "experiences",
        summary: "Experiences: the Allowed / Blocked headed lists, each row an experience name \
                  with a Forget button.",
        spec: crate::experiences_floater::experiences_floater_spec,
        content: FloaterContent::Specimen(crate::experiences_floater::spawn_experiences_specimen),
    },
    FloaterElement {
        id: "group-profile",
        summary: "Group: a group's General / Roles / Members / Notices / Land tabs. \
                  Subject-bound, so its geometry is not persisted.",
        spec: crate::group_profile::group_profile_floater_spec,
        content: FloaterContent::Stub(
            "The group's tabs, rebuilt per open from the group profile and role replies.",
        ),
    },
    FloaterElement {
        id: "inventory",
        summary: "Inventory: the tab / expand / collapse toolbar, the search field and the \
                  virtualized folder tree — the viewer's largest window.",
        spec: crate::inventory::inventory_floater_spec,
        content: FloaterContent::Stub(
            "The tab / expand / collapse toolbar, the search field and the virtualized folder \
             tree, filled from the inventory skeleton.",
        ),
    },
    FloaterElement {
        id: "inventory-filters",
        summary: "Inventory Filters: the type checkboxes, the date-range and permission filters, \
                  and the reset row that drives the inventory window's view.",
        spec: crate::inventory_filters::inventory_filters_floater_spec,
        content: FloaterContent::Stub(
            "The type checkboxes, the date-range and permission filters, and the reset row.",
        ),
    },
    FloaterElement {
        id: "inventory-gallery",
        summary: "Inventory Gallery: the thumbnail grid view of a folder, the alternative to the \
                  tree.",
        spec: crate::inventory_gallery::inventory_gallery_floater_spec,
        content: FloaterContent::Stub(
            "The thumbnail grid of the selected folder, filled from the inventory model and the \
             texture cache.",
        ),
    },
    FloaterElement {
        id: "item-properties",
        summary: "Item Properties: an inventory item's name, description and sale fields with its \
                  permission checkboxes. Subject-bound, so its geometry is not persisted.",
        spec: crate::inventory_properties::item_properties_floater_spec,
        content: FloaterContent::Stub(
            "The item's name, description and sale fields with its permission checkboxes.",
        ),
    },
    FloaterElement {
        id: "material-editor",
        summary: "Edit Material: the GLTF material asset editor — the base-colour, metallic / \
                  roughness, normal and emissive channels with their texture swatches.",
        spec: crate::edit_material_asset::material_editor_floater_spec,
        content: FloaterContent::Stub(
            "The base-colour, metallic / roughness, normal and emissive channels with their \
             texture swatches, loaded from the material asset.",
        ),
    },
    FloaterElement {
        id: "minimap",
        summary: "Mini-map: the composited local map surface with its parcel lines, avatar dots \
                  and compass labels.",
        spec: crate::minimap::minimap_floater_spec,
        content: FloaterContent::Specimen(crate::minimap::spawn_minimap_specimen),
    },
    FloaterElement {
        id: "notecard-editor",
        summary: "Notecard: the view toggle over the editable body and the Save button — the \
                  notecard asset editor.",
        spec: crate::edit_notecard::notecard_editor_floater_spec,
        content: FloaterContent::Specimen(crate::edit_notecard::spawn_notecard_editor_specimen),
    },
    FloaterElement {
        id: "object-contents",
        summary: "Object Contents: the selected prim's task-inventory list with its drop target \
                  and the open / remove actions.",
        spec: crate::edit_contents::object_contents_floater_spec,
        content: FloaterContent::Stub(
            "The selected prim's task-inventory list with its drop target and the open / remove \
             actions.",
        ),
    },
    FloaterElement {
        id: "preferences",
        summary: "Preferences: the search box over a leading tab strip, the labelled setting rows \
                  and the OK / Cancel footer.",
        spec: crate::preferences::preferences_floater_spec,
        content: FloaterContent::Specimen(crate::preferences::spawn_preferences_specimen),
    },
    FloaterElement {
        id: "preview-animation",
        summary: "Animation preview: an animation item's play / stop controls and its metadata. \
                  Subject-bound, so its geometry is not persisted.",
        spec: crate::inventory_properties::animation_preview_floater_spec,
        content: FloaterContent::Stub(
            "The animation's play / stop controls and its priority / duration metadata.",
        ),
    },
    FloaterElement {
        id: "preview-texture",
        summary: "Texture preview: an inventory texture decoded to an image node at its own \
                  aspect. Subject-bound, so its geometry is not persisted.",
        spec: crate::inventory_properties::texture_preview_floater_spec,
        content: FloaterContent::Stub(
            "The decoded texture as an image node at its own aspect ratio, with its dimensions.",
        ),
    },
    FloaterElement {
        id: "quick-preferences",
        summary: "Quick Preferences: the environment preset and time-of-day combos over the \
                  curated setting sliders.",
        spec: crate::quick_preferences::quick_prefs_floater_spec,
        content: FloaterContent::Specimen(crate::quick_preferences::spawn_quick_prefs_specimen),
    },
    FloaterElement {
        id: "radar",
        summary: "Radar: the counts line, the nearby-avatar table with its status glyphs and \
                  band-coloured ranges, and the action buttons.",
        spec: crate::radar::radar_floater_spec,
        content: FloaterContent::Specimen(crate::radar::spawn_radar_specimen),
    },
    FloaterElement {
        id: "script-editor",
        summary: "Script: the LSL editor's body, the Running toggle, Save & Compile, and the \
                  compile-diagnostic rows.",
        spec: crate::edit_script::script_editor_floater_spec,
        content: FloaterContent::Specimen(crate::edit_script::spawn_script_editor_specimen),
    },
    FloaterElement {
        id: "search",
        summary: "Search: the query field over the category tabs and the result list — the \
                  directory surface.",
        spec: crate::search::search_floater_spec,
        content: FloaterContent::Stub(
            "The query field, the category tabs and the result list, filled from the directory \
             replies.",
        ),
    },
    FloaterElement {
        id: "snapshot",
        summary: "Snapshot: the preview frame, the include toggles, Refresh, the format picker and \
                  the destination tabs.",
        spec: crate::snapshot_floater::snapshot_floater_spec,
        content: FloaterContent::Stub(
            "The preview frame, the include toggles, the Refresh button, the format picker and \
             the destination tabs.",
        ),
    },
    FloaterElement {
        id: "texture-picker",
        summary: "Pick: Texture — the inventory swatch grid, the quick-choice row and the \
                  OK / Cancel reply protocol every texture swatch is written against.",
        spec: crate::ui_texture_picker::texture_picker_floater_spec,
        content: FloaterContent::Stub(
            "The inventory swatch grid, the quick-choice row and the OK / Cancel reply row.",
        ),
    },
    FloaterElement {
        id: "wearable-editor",
        summary: "Edit Wearable: a worn item's visual-param sliders and its texture swatches, with \
                  the Save / Save As row.",
        spec: crate::edit_wearable::wearable_editor_floater_spec,
        content: FloaterContent::Stub(
            "The worn item's visual-param sliders and texture swatches, with the Save / Save As \
             row.",
        ),
    },
    FloaterElement {
        id: "web-browser",
        summary: "Web Browser: the navigation toolbar, the embedded browser view and the status \
                  row — where a script's `llLoadURL` and a profile's web tab land.",
        spec: crate::web_floater::web_floater_spec,
        content: FloaterContent::Stub(
            "The navigation toolbar, the embedded browser view and the status row.",
        ),
    },
    FloaterElement {
        id: "worldmap",
        summary: "World Map: the composited grid-tile surface with its region fills and markers, \
                  beside the search side panel.",
        spec: crate::world_map::world_map_floater_spec,
        content: FloaterContent::Specimen(crate::world_map::spawn_world_map_specimen),
    },
];

#[cfg(test)]
mod tests {
    use super::FLOATERS;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;

    /// Ids are what a failing check names, what the gallery lists and what
    /// `floater_persist` keys a remembered rectangle by — so a duplicate would
    /// make one window's failure indistinguishable from another's, and would
    /// make two windows share one saved geometry.
    #[test]
    fn floater_ids_are_unique() {
        let mut ids: Vec<&str> = FLOATERS.iter().map(|floater| floater.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "two registered floaters share an id");
    }

    /// The registry has to be non-empty, or every sweep over it passes by
    /// iterating over nothing.
    #[test]
    fn the_registry_is_not_empty() {
        assert!(!FLOATERS.is_empty(), "no floaters are registered");
    }

    /// An entry's `id` must be the id its own spec constructor returns.
    ///
    /// The entry carries the id twice — once so the list reads as a list, and
    /// once inside the spec the viewer actually spawns — and the whole value of
    /// the registry rests on those being the same string. If they diverge, the
    /// gallery labels a card with one name while spawning another window, and a
    /// failing sweep names an id nothing in the viewer answers to.
    #[test]
    fn a_registered_floater_agrees_with_its_own_spec() {
        for floater in FLOATERS {
            let spec = (floater.spec)();
            assert_eq!(
                floater.id, spec.id,
                "the `{}` entry spawns a floater whose id is `{}`",
                floater.id, spec.id
            );
        }
    }

    /// A registered floater's title must be non-empty, and its minimum must fit
    /// inside its default: a `min_size` larger than `default_size` opens the
    /// window already below its own floor, which `apply_floater_content`
    /// silently corrects by growing it — so the declared default is a fiction
    /// nobody would notice.
    #[test]
    fn a_registered_floater_declares_a_coherent_rectangle() {
        for floater in FLOATERS {
            let spec = (floater.spec)();
            assert!(
                !spec.title.trim().is_empty(),
                "the `{}` floater has no title",
                floater.id
            );
            if let (Some(default_size), Some(min_size)) = (spec.default_size, spec.min_size) {
                assert!(
                    min_size.x <= default_size.x && min_size.y <= default_size.y,
                    "the `{}` floater opens at {default_size:?}, below its own minimum \
                     {min_size:?}",
                    floater.id
                );
            }
        }
    }

    /// **The rule, enforced.** Every `*_floater_spec` constructor in the
    /// workspace must be named by an entry in [`FLOATERS`].
    ///
    /// Writing "every floater registers" in a doc comment makes it a
    /// convention; this makes it a check. A new window is a new spec
    /// constructor — that is the shape every one of them has — so scanning for
    /// the constructors and demanding each be referenced here catches the one
    /// failure mode a registry actually has: somebody adds a window, never adds
    /// the line, and every sweep goes on being green about a smaller viewer
    /// than it claims.
    ///
    /// Source-scanning rather than reflective because a `const` list cannot see
    /// what was never added to it, and the alternative — a runtime census of an
    /// app with every plugin installed — would need the session the registry
    /// exists to do without. The precedent is `ui_font`'s scan of `src` for
    /// unregistered faces.
    #[test]
    fn every_floater_spec_in_the_workspace_is_registered() -> Result<(), std::io::Error> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or_else(|| {
                std::io::Error::other("the viewer crate has no parent directory".to_owned())
            })?
            .to_owned();
        let registry = fs_err::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/floaters.rs"),
        )?;

        let mut defined: BTreeSet<String> = BTreeSet::new();
        for crate_dir in fs_err::read_dir(&root)? {
            let src = crate_dir?.path().join("src");
            if !src.is_dir() {
                continue;
            }
            collect_spec_constructors(&src, &mut defined)?;
        }
        assert!(
            !defined.is_empty(),
            "found no `*_floater_spec` constructors at all — the scan is looking in the wrong \
             place and this test is passing vacuously"
        );

        let unregistered: Vec<&String> = defined
            .iter()
            .filter(|name| !registry.contains(name.as_str()))
            .collect();
        assert!(
            unregistered.is_empty(),
            "these floaters are not in `FLOATERS`, so no sweep and no gallery can reach them — \
             add an entry for each: {unregistered:#?}"
        );
        Ok(())
    }

    /// Every `fn <name>_floater_spec(` defined under `dir`, recursively.
    ///
    /// Deliberately naive: it matches the declaration line, which is what the
    /// convention is about. The private helper `preview_floater_spec` (which the
    /// two item previews share) is skipped by the `pub` prefix — it is not a
    /// window of its own, and its two public callers are what the registry
    /// names.
    fn collect_spec_constructors(
        dir: &std::path::Path,
        found: &mut BTreeSet<String>,
    ) -> Result<(), std::io::Error> {
        for entry in fs_err::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                collect_spec_constructors(&path, found)?;
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            for line in fs_err::read_to_string(&path)?.lines() {
                let Some(rest) = line
                    .strip_prefix("pub fn ")
                    .or_else(|| line.strip_prefix("pub(crate) fn "))
                else {
                    continue;
                };
                let Some((name, _)) = rest.split_once('(') else {
                    continue;
                };
                if name.ends_with("_floater_spec") {
                    found.insert(name.to_owned());
                }
            }
        }
        Ok(())
    }
}
