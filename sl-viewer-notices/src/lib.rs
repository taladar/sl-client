//! The viewer's notice surfaces: what interrupts, and what it is written in.
//!
//! A script asks to take controls, an experience asks to be trusted, the
//! simulator sends a dialog with buttons. Each arrives unbidden, has to be
//! shown without stealing the world, and has to survive a relog if it was not
//! answered -- that is [`notification_host`] and [`notification_persist`].
//!
//! The text they are written in lives here too. A notice is mostly a sentence
//! with things in it you can click: a resident, a group, a URL, a SLURL. That
//! is [`linkified_text`] and [`ui_name_link`], which every other surface that
//! shows a clickable name or link also uses.

#![expect(
    clippy::module_name_repetitions,
    reason = "each module owns one concept and is named for it, so its types read \
              as `linkified_text::LinkTextStyle` and `script_dialog::ScriptDialog`. \
              That only became a lint when these items turned `pub` for the crate \
              split; renaming them would churn every call site in the viewer to \
              satisfy a style rule this codebase does not follow"
)]

// Lower crates re-aliased under their original module names, so these
// modules keep addressing them as `crate::ui` and `crate::settings`.
pub(crate) use sl_viewer_kit::parcel_names;
pub(crate) use sl_viewer_notifications as notifications;
pub(crate) use sl_viewer_platform::system_browser;
pub(crate) use sl_viewer_platform::url_linkify;
pub(crate) use sl_viewer_settings as settings;
pub(crate) use sl_viewer_ui_core::i18n;
pub(crate) use sl_viewer_ui_core::ui;
pub(crate) use sl_viewer_ui_core::ui_element;
pub(crate) use sl_viewer_ui_core::ui_font;
pub(crate) use sl_viewer_ui_core::virtual_list;
pub(crate) use sl_viewer_ui_widgets::floater;
pub(crate) use sl_viewer_ui_widgets::settings_binding;
pub(crate) use sl_viewer_ui_widgets::ui_combo;
pub(crate) use sl_viewer_ui_widgets::ui_search;
pub(crate) use sl_viewer_ui_widgets::ui_tab;
pub(crate) use sl_viewer_ui_widgets::ui_table;
pub(crate) use sl_viewer_ui_widgets::ui_text_input;
pub(crate) use sl_viewer_world_api as world_api;

pub mod experience_log;
pub mod experience_permission;
pub mod experience_profile;
pub mod experiences_floater;
pub mod linkified_text;
pub mod notification_host;
pub mod notification_persist;
pub mod script_dialog;
pub mod script_permission;
pub mod ui_name_link;

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::KeyboardInput;
    use bevy::input::mouse::AccumulatedMouseScroll;
    use bevy::picking::hover::HoverMap;
    use bevy::prelude::*;
    use sl_viewer_notifications::ShowNotification;
    use sl_viewer_ui_core::i18n::install_untranslated;
    use sl_viewer_ui_core::ui::{UiDirection, UiRoot};
    use sl_viewer_ui_core::virtual_list::VirtualListPlugin;
    use sl_viewer_ui_widgets::floater::FloaterPlugin;
    use sl_viewer_ui_widgets::settings_binding::SettingsBindingPlugin;
    use sl_viewer_ui_widgets::ui_combo::ComboWidgetPlugin;
    use sl_viewer_ui_widgets::ui_search::SearchFieldPlugin;
    use sl_viewer_ui_widgets::ui_tab::TabWidgetPlugin;
    use sl_viewer_ui_widgets::ui_table::TableWidgetPlugin;
    use sl_viewer_ui_widgets::ui_text_input::TextInputPlugin;

    /// **Every experience window in this crate can actually be scheduled.**
    ///
    /// Not a layout check — the viewer's floater sweep already measures the
    /// chrome, and it builds that chrome from the [`FloaterSpec`]s *without*
    /// adding these plugins. This is the check that the systems run at all, and
    /// it catches the two failures that are panics on a system's **first run**
    /// (so: the whole viewer dying on frame one, before anything draws):
    ///
    /// - a `B0001` query conflict — `Query<&mut Text>` beside
    ///   `Query<(&mut Text, &mut TextColor)>` in one system, which the
    ///   experiences floater is one careless parameter away from, since it
    ///   writes both table cells and plain labels;
    /// - a `MessageReader` / `MessageWriter` for a message nothing registered,
    ///   which fails param validation the same way.
    ///
    /// The seams stood up below are the ones the **host** owns: the session
    /// channels, the two name caches the rows resolve owners through, the
    /// agent's position the profile's location button reads, and the locale.
    /// Everything these windows speak over themselves — `OpenExperienceProfile`
    /// above all — must be registered by their own plugins, and that is the
    /// other half of what this pins.
    ///
    /// [`FloaterSpec`]: sl_viewer_ui_widgets::floater::FloaterSpec
    #[test]
    fn every_experience_window_schedules_without_conflicting() {
        let mut app = App::new();
        app.init_resource::<UiScale>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<bevy::input_focus::InputFocus>()
            .init_resource::<Time>()
            // The seams the host owns, and the price of scheduling the real
            // widget plugins rather than a stub: the reading direction the tab
            // strip mirrors under, the wheel accumulator and hover map the
            // virtualized lists scroll from, the font context and key stream the
            // text fields need, and the toast channel the event log raises on.
            .init_resource::<UiDirection>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<HoverMap>()
            .init_resource::<bevy::text::FontCx>()
            .init_resource::<bevy::text::LayoutCx>()
            .add_message::<KeyboardInput>()
            .add_message::<ShowNotification>()
            .init_resource::<sl_viewer_world_api::AvatarState>()
            .init_resource::<sl_viewer_world_api::GroupsModel>()
            .init_resource::<sl_viewer_world_api::AgentRegionPosition>()
            .add_message::<sl_client_bevy::SlCommand>()
            .add_message::<sl_client_bevy::SlEvent>()
            .add_message::<sl_viewer_world_api::OpenAvatarProfile>()
            .add_message::<sl_viewer_world_api::OpenGroupProfile>()
            .add_plugins((
                FloaterPlugin,
                VirtualListPlugin,
                TableWidgetPlugin,
                TabWidgetPlugin,
                ComboWidgetPlugin,
                SearchFieldPlugin,
                TextInputPlugin,
                SettingsBindingPlugin,
            ))
            .add_plugins((
                crate::ui_name_link::NameLinkPlugin,
                crate::experience_log::ExperienceLogPlugin,
                crate::experiences_floater::ExperiencesPlugin,
                crate::experience_profile::ExperienceProfilePlugin,
            ));
        // Every key resolves to itself, which is all a scheduling check needs —
        // and without it `Translator` has no `Localization` to read.
        install_untranslated(&mut app);
        let root = app.world_mut().spawn(Node::default()).id();
        app.insert_resource(UiRoot(root));
        // Two frames: the row pool only exists on a frame after the one that
        // spawned the tables, so the populate / bind passes need the second.
        app.update();
        app.update();
    }

    /// Opening a profile is a **message**, and the window it opens is keyed by
    /// experience — so two experiences are two windows, and neither the open
    /// path nor the per-window systems may fall over on the way.
    #[test]
    fn two_experience_profiles_open_as_two_windows() {
        use crate::experience_profile::{ExperienceProfileState, OpenExperienceProfile};
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{ExperienceKey, Uuid};

        let mut app = App::new();
        app.init_resource::<UiScale>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<bevy::input_focus::InputFocus>()
            .init_resource::<Time>()
            // The seams the host owns, and the price of scheduling the real
            // widget plugins rather than a stub: the reading direction the tab
            // strip mirrors under, the wheel accumulator and hover map the
            // virtualized lists scroll from, the font context and key stream the
            // text fields need, and the toast channel the event log raises on.
            .init_resource::<UiDirection>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<HoverMap>()
            .init_resource::<bevy::text::FontCx>()
            .init_resource::<bevy::text::LayoutCx>()
            .add_message::<KeyboardInput>()
            .add_message::<ShowNotification>()
            .init_resource::<sl_viewer_world_api::AvatarState>()
            .init_resource::<sl_viewer_world_api::GroupsModel>()
            .init_resource::<sl_viewer_world_api::AgentRegionPosition>()
            .add_message::<sl_client_bevy::SlCommand>()
            .add_message::<sl_client_bevy::SlEvent>()
            .add_message::<sl_viewer_world_api::OpenAvatarProfile>()
            .add_message::<sl_viewer_world_api::OpenGroupProfile>()
            .add_plugins((
                FloaterPlugin,
                VirtualListPlugin,
                ComboWidgetPlugin,
                TextInputPlugin,
                crate::ui_name_link::NameLinkPlugin,
                crate::experience_profile::ExperienceProfilePlugin,
            ));
        install_untranslated(&mut app);
        let root = app.world_mut().spawn(Node::default()).id();
        app.insert_resource(UiRoot(root));
        app.update();

        for raw in [0xa1_u128, 0xb2] {
            app.world_mut().write_message(OpenExperienceProfile {
                experience: ExperienceKey::from(Uuid::from_u128(raw)),
            });
        }
        app.update();
        app.update();

        let mut windows = app.world_mut().query::<&ExperienceProfileState>();
        assert_eq!(
            windows.iter(app.world()).count(),
            2,
            "two experiences must open two profile windows"
        );

        // Re-opening one of them raises it rather than spawning a second.
        app.world_mut().write_message(OpenExperienceProfile {
            experience: ExperienceKey::from(Uuid::from_u128(0xa1)),
        });
        app.update();
        let mut windows = app.world_mut().query::<&ExperienceProfileState>();
        assert_eq!(
            windows.iter(app.world()).count(),
            2,
            "re-opening an experience must raise its window, not spawn another"
        );
    }
}
