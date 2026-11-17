//! The **login screen** (`viewer-login-screen`): the panel a viewer started
//! without credentials shows — a grid, an avatar name, a password and a start
//! location — and the front door it opens.
//!
//! # One App, a page over the world
//!
//! The screen is part of the one App the world runs in (`crate::login_flow`
//! says why there cannot be a second): an opaque page across the window, under
//! the floaters so the pre-login menu's Preferences and About open over it,
//! while the permanent chrome ([`ScreenChrome`]: the menu bar, the bottom
//! toolbar) is hidden. It gives the window to the world when the agent's own
//! avatar arrives, and a login that fails before then leaves it up to say why
//! (`crate::session` leaves the window open under the
//! [`LoginScreen`](sl_viewer_world_view::session::LoginScreen) marker).
//!
//! # A login, from the panel
//!
//! **Log In** builds the same `LoginParams` the credentials file does, and
//! everything a login decides beside the session (`LoginScope`: the account's
//! settings identity, the land cache, About's facts, the automation hello) is
//! inserted as it is sent, as an `SlLogin`. A second-factor challenge shows the
//! token row; its answer is the same login again with the token folded in. A
//! refusal, an unreachable grid or a lost circuit is a notice on the panel,
//! and the next **Log In** tries again.
//!
//! # The grids
//!
//! The grid combo offers [`GridList`]: the built-in grids and the user's own.
//! The row under it adds a grid by its login URI, named by the grid's own
//! `get_grid_info` (fetched off the frame thread, as Firestorm's grid manager
//! does), and removes a user grid; a built-in one cannot be removed.
//!
//! The last grid and avatar name are remembered in the global settings.
//!
//! # A remembered password
//!
//! With **Remember password** ticked (`LoginRememberPassword`), a login that
//! arrives keeps its password in the secret store ([`crate::secrets`]: the OS
//! keychain), keyed by the grid and the avatar as `sl-account-dirs` names
//! their directories; choosing that grid and avatar again fills the password
//! field from it, the text never entering the editor. Unticking the box, or a
//! refusal for a bad password, forgets it. Where no keychain can keep it, the
//! line under the box says why, rather than pretending to remember.
//!
//! Reference (Firestorm, read-only): `llpanellogin.cpp`, `panel_login.xml`,
//! `menu_login.xml`, `llstartup.cpp`, `fsgridhandler.cpp`.

use std::sync::mpsc::{Receiver, TryRecvError};

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, FontCx, LayoutCx};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Activate;
use sl_account_dirs::{avatar_dir_name, grid_dir_name};
use sl_client_bevy::grids::{GridInfo, fetch_grid_info};
use sl_client_bevy::{
    LoginParams, LoginRequest, LoginServer, SessionDisconnectReason, SlEvent, SlLogin,
    SlLoginRejected, SlMfaChallenge, SlSessionEvent, SlSessionStatus, StartLocation,
};
use sl_secret_store::{Secret, SecretKey};
use sl_settings::{Scope, SettingValue};
use url::Url;

use crate::floater::{Floater, toggle_floater};
use crate::i18n::{TransArgs, Translator};
use crate::login_flow::{LoginScope, PendingLogin, SecondFactorSource};
use crate::login_grids::GridList;
use crate::menu::{MenuBarDef, MenuCommand, MenuConditions, MenuDef, MenuItemDef, spawn_menu_bar};
use crate::secrets::{SecretAnswer, SecretOp, SecretOutcome, SecretRequest};
use crate::settings::ViewerSettings;
use crate::settings_binding::SettingBinding;
use crate::skin_palette::SkinPalette;
use crate::ui::{ScreenChrome, UiPanelShown, UiRoot, UiScaffoldSystems, column};
use crate::ui_checkbox::{CheckboxSpec, spawn_checkbox};
use crate::ui_combo::{ComboSelection, ComboSpec, SetComboOptions, spawn_combo};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_spawn::{
    ButtonKind, ButtonSpec, LabeledRowSpec, UiLabel, spawn_button, spawn_label, spawn_labeled_row,
};
use crate::ui_text_input::{PasswordField, TextInputKind, TextInputSpec, spawn_text_input};

/// The global setting holding the login URI of the grid last logged in to.
pub(crate) const SETTING_LOGIN_LAST_GRID: &str = "LoginLastGrid";

/// The global setting holding the avatar name last logged in with.
pub(crate) const SETTING_LOGIN_LAST_AVATAR: &str = "LoginLastAvatar";

/// The global setting saying whether a login keeps its password in the
/// secret store.
pub(crate) const SETTING_LOGIN_REMEMBER_PASSWORD: &str = "LoginRememberPassword";

/// The section the login settings are kept under.
const LOGIN_SECTION: &[&str] = &["login"];

/// The element the pre-login menu bar reports its picks under.
const LOGIN_MENU_ELEMENT: &str = "login-menu-bar";

/// The panel's font size, in logical pixels.
const PANEL_FONT: f32 = 14.0;

/// The width of the panel's label column, in logical pixels, so the fields
/// line up.
const LABEL_MIN_WIDTH: f32 = 120.0;

/// The width of a field, in `"0"` advances.
const FIELD_GLYPHS: f32 = 24.0;

/// The last name Second Life gives an account that has only a username.
const DEFAULT_LAST_NAME: &str = "Resident";

/// The start-location combo's options, in order: last location, home, a
/// region.
const START_OPTIONS: [&str; 3] = ["login-start-last", "login-start-home", "login-start-region"];

/// The start combo's index for a named region.
const START_REGION: usize = 2;

/// The Viewer menu before login: Preferences and Exit. (The reference's Show
/// Grid Picker is moot here, where the grid combo is always shown; its Close
/// Window is Exit, for a viewer with one window.)
static LOGIN_VIEWER_MENU: MenuDef = MenuDef {
    label_key: "login-menu-viewer",
    items: &[
        MenuItemDef::Command(MenuCommand::new(
            "menu-bar-preferences",
            "toggle-preferences",
        )),
        MenuItemDef::Command(MenuCommand::new("login-menu-exit", "quit")),
    ],
};

/// The Help menu before login: About.
static LOGIN_HELP_MENU: MenuDef = MenuDef {
    label_key: "menu-bar-help",
    items: &[MenuItemDef::Command(MenuCommand::new(
        "menu-bar-about",
        "toggle-about",
    ))],
};

/// The pre-login menu bar (`menu_login.xml`).
static LOGIN_MENU_BAR: MenuBarDef = MenuBarDef {
    menus: &[&LOGIN_VIEWER_MENU, &LOGIN_HELP_MENU],
};

/// What a viewer on its login screen sends besides what its user types: the
/// viewer it reports itself as.
#[derive(Resource, Debug, Clone)]
pub(crate) struct LoginForm {
    /// The viewer channel reported to the grid.
    pub(crate) channel: String,
    /// The viewer version reported to the grid.
    pub(crate) version: String,
}

/// Something the panel's user asked for, by a button or by `Enter` in a field.
#[derive(Message, Component, Debug, Clone, Copy, PartialEq, Eq)]
enum LoginPanelRequest {
    /// Log in with what the panel holds.
    Connect,
    /// Answer the second-factor challenge with the token field.
    SubmitToken,
    /// Add the grid at the login URI field.
    AddGrid,
    /// Remove the chosen grid.
    RemoveGrid,
}

/// The login page's parts.
#[derive(Resource, Debug, Clone, Copy)]
struct LoginPanelUi {
    /// The page across the window.
    page: Entity,
    /// The grid combo.
    grid: Entity,
    /// The button that removes the chosen grid.
    remove_grid: Entity,
    /// The field a new grid's login URI is typed into.
    grid_uri: Entity,
    /// The avatar name field.
    avatar: Entity,
    /// The password field.
    password: Entity,
    /// The line under Remember password saying why no password can be kept.
    remember_note: Entity,
    /// The start-location combo.
    start: Entity,
    /// The region field, for a start in a named region.
    start_region: Entity,
    /// The Log In button.
    connect: Entity,
    /// The line the panel's notices are written to.
    notice: Entity,
    /// The second-factor row, shown while a challenge waits.
    token_row: Entity,
    /// The second-factor token field.
    token: Entity,
}

/// A notice the panel shows: a Fluent key, with the grid's own words where it
/// gave some.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Notice {
    /// Nothing to say.
    #[default]
    None,
    /// A notice that is only its key.
    Key(&'static str),
    /// A notice whose `$message` is the grid's (or the system's) own words.
    Message(&'static str, String),
}

/// What the panel knows beyond its widgets.
#[derive(Resource, Debug, Default)]
struct LoginPanelState {
    /// The last thing that went wrong, or what the user must do next.
    notice: Notice,
    /// The second-factor challenge a token is waiting for, if one is.
    challenge: Option<WaitingChallenge>,
    /// What the notice line shows, so it is rewritten only on a change.
    shown: Option<Notice>,
    /// The remembered password's part.
    saved: SavedPassword,
}

/// Where the remembered password stands.
#[derive(Debug, Default)]
struct SavedPassword {
    /// The grid and avatar the panel shows, whose saved password was asked
    /// for.
    asked: Option<SecretKey>,
    /// Whether the password field holds what the store gave for `asked`.
    filled: bool,
    /// The login under way: the grid and avatar, and the password it was sent
    /// with, kept once it arrives if the box is ticked.
    attempt: Option<(SecretKey, Secret)>,
    /// Why the store could not keep a password, said under the box.
    problem: Option<String>,
    /// What the line under the box shows, so it is rewritten only on a
    /// change; `None` before it is first written.
    shown: Option<ShownProblem>,
}

/// The problem the line under Remember password was last written with, or
/// none.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShownProblem(Option<String>);

/// A second-factor challenge waiting for the token the user types.
#[derive(Debug, Clone)]
struct WaitingChallenge {
    /// The hash the challenge carried, which the answer sends back.
    mfa_hash: Option<String>,
}

/// A grid being added, waiting for its `get_grid_info`.
#[derive(Debug)]
struct GridFetch {
    /// The login URI typed in.
    login_uri: Url,
    /// The fetch thread's answer.
    answer: Receiver<Result<GridInfo, String>>,
}

/// The login screen's runtime: the page, its grids, the login it sends and
/// the progress it shows. Every system waits for [`LoginForm`], which only a
/// viewer started on its login screen holds.
pub(crate) struct LoginScreenPlugin;

impl Plugin for LoginScreenPlugin {
    fn build(&self, app: &mut App) {
        let on_screen = resource_exists::<LoginForm>;
        app.add_message::<LoginPanelRequest>()
            .init_resource::<LoginPanelState>()
            .add_systems(
                Startup,
                spawn_login_page
                    .after(UiScaffoldSystems::SpawnRoot)
                    .run_if(on_screen),
            )
            .add_systems(
                Update,
                (
                    submit_on_enter,
                    connect,
                    submit_token,
                    add_grid,
                    remove_grid,
                    follow_the_login,
                    ask_for_the_saved_password,
                    take_the_saved_password,
                    keep_the_password,
                    show_the_page,
                    reflect_the_panel,
                    write_the_notice,
                    write_the_remember_note,
                    handle_login_menu,
                )
                    .chain()
                    .run_if(resource_exists::<LoginPanelUi>),
            );
    }
}

/// Register the login screen's settings: the last grid and avatar name, and
/// whether a password is remembered.
pub(crate) fn register_settings(settings: &mut ViewerSettings) {
    settings.register_in(
        LOGIN_SECTION,
        SETTING_LOGIN_LAST_GRID,
        SettingValue::String(String::new()),
        "setting-desc-LoginLastGrid",
    );
    settings.register_in(
        LOGIN_SECTION,
        SETTING_LOGIN_LAST_AVATAR,
        SettingValue::String(String::new()),
        "setting-desc-LoginLastAvatar",
    );
    settings.register_in(
        LOGIN_SECTION,
        SETTING_LOGIN_REMEMBER_PASSWORD,
        SettingValue::Bool(false),
        "setting-desc-LoginRememberPassword",
    );
}

/// Whether Remember password is ticked.
fn remembering(settings: &ViewerSettings) -> bool {
    settings
        .store()
        .get_bool(SETTING_LOGIN_REMEMBER_PASSWORD)
        .unwrap_or(false)
}

/// The secret-store key of `first last`'s password on the grid at
/// `login_uri`.
fn password_key(login_uri: &Url, first: &str, last: &str) -> SecretKey {
    SecretKey::login_password(&grid_dir_name(login_uri), &avatar_dir_name(first, last))
}

/// A string setting's value, or empty when it is not set.
fn stored_string(settings: &ViewerSettings, name: &str) -> String {
    settings
        .store()
        .get_str(name)
        .map(str::to_owned)
        .unwrap_or_default()
}

/// The start combo's index and region field for a stored start location.
fn start_choice(stored: &str) -> (usize, String) {
    match stored.parse::<StartLocation>() {
        Ok(StartLocation::Home) => (1, String::new()),
        Ok(StartLocation::Region { region, .. }) => (START_REGION, region),
        Ok(StartLocation::Last) | Err(_) => (0, String::new()),
    }
}

/// Spawn the login page, the grid list and the panel's parts.
fn spawn_login_page(
    mut commands: Commands,
    root: Res<UiRoot>,
    paths: Res<crate::assembly::ViewerPaths>,
    settings: Res<ViewerSettings>,
) {
    let grids = GridList::load(paths.grids_file());
    let last_grid = stored_string(&settings, SETTING_LOGIN_LAST_GRID);
    let grid_index = last_grid
        .parse::<Url>()
        .ok()
        .and_then(|uri| grids.position(&uri))
        .unwrap_or(0);
    let last_avatar = stored_string(&settings, SETTING_LOGIN_LAST_AVATAR);
    let (start_index, start_region) = start_choice(&stored_string(
        &settings,
        crate::preferences_general::SETTING_LOGIN_START_LOCATION,
    ));
    let palette = SkinPalette::default();

    let page = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            // Opaque: nothing of the world behind it is drawn yet.
            BackgroundColor(palette.surface_bg.with_alpha(1.0)),
            UiPanelShown(true),
            Name::new("login-page"),
            ChildOf(root.0),
        ))
        .id();
    let bar = spawn_menu_bar(
        &mut commands,
        page,
        ElementCx {
            font_size: crate::menu_bar::TOP_BAR_FONT,
            ..ElementCx::new()
        },
        &LOGIN_MENU_BAR,
        LOGIN_MENU_ELEMENT,
    );
    commands.entity(bar).insert(MenuConditions::default());

    // The panel itself, centred on the page.
    let centre = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            ChildOf(page),
        ))
        .id();
    let panel = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(16.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..column(Val::Px(8.0))
            },
            BackgroundColor(palette.card_bg),
            BorderColor::all(palette.surface_border),
            Name::new("login-panel"),
            ChildOf(centre),
        ))
        .id();

    let labeled = |commands: &mut Commands, key: &'static str| {
        spawn_labeled_row(
            commands,
            panel,
            LabeledRowSpec::new(UiLabel::key(key))
                .font_size(PANEL_FONT)
                .label_color(palette.text_muted)
                .label_min_width(Val::Px(LABEL_MIN_WIDTH)),
        )
        .row
    };

    // The grid, and the button that removes a user grid.
    let grid_row = labeled(&mut commands, "login-grid-label");
    let grid_labels: Vec<String> = grids.grids().iter().map(|grid| grid.name.clone()).collect();
    let grid = spawn_combo(
        &mut commands,
        grid_row,
        &ComboSpec {
            element: "login-grid",
            labels: &grid_labels,
            active: grid_index,
            tab_index: 1,
            font_size: PANEL_FONT,
            translate_labels: false,
        },
    );
    let remove_grid = spawn_request_button(
        &mut commands,
        grid_row,
        "login-remove-grid",
        LoginPanelRequest::RemoveGrid,
        2,
    );

    // A new grid by its login URI.
    let add_row = labeled(&mut commands, "login-add-grid-label");
    let grid_uri = spawn_text_input(
        &mut commands,
        add_row,
        &TextInputSpec {
            font_size: PANEL_FONT,
            width_glyphs: FIELD_GLYPHS,
            tab_index: 3,
            ..TextInputSpec::new("login-grid-uri", TextInputKind::Line)
        },
    );
    let _add = spawn_request_button(
        &mut commands,
        add_row,
        "login-add-grid",
        LoginPanelRequest::AddGrid,
        4,
    );

    let avatar_row = labeled(&mut commands, "login-avatar-label");
    let avatar = spawn_text_input(
        &mut commands,
        avatar_row,
        &TextInputSpec {
            initial: last_avatar,
            font_size: PANEL_FONT,
            width_glyphs: FIELD_GLYPHS,
            tab_index: 5,
            ..TextInputSpec::new("login-avatar", TextInputKind::Line)
        },
    );
    let password_row = labeled(&mut commands, "login-password-label");
    let password = spawn_text_input(
        &mut commands,
        password_row,
        &TextInputSpec {
            font_size: PANEL_FONT,
            width_glyphs: FIELD_GLYPHS,
            tab_index: 6,
            ..TextInputSpec::new("login-password", TextInputKind::Password)
        },
    );
    // Remember password, under the fields it belongs to, and the line that
    // says why no password can be kept, when none can.
    let remember_row = spawn_labeled_row(
        &mut commands,
        panel,
        LabeledRowSpec::new(UiLabel::literal(String::new()))
            .font_size(PANEL_FONT)
            .label_min_width(Val::Px(LABEL_MIN_WIDTH)),
    )
    .row;
    let remember = spawn_checkbox(
        &mut commands,
        remember_row,
        &CheckboxSpec {
            element: "login-remember",
            label: "login-remember-password".to_owned(),
            tab_index: 7,
            font_size: PANEL_FONT,
            translate_label: true,
        },
    )
    .checkbox;
    commands
        .entity(remember)
        .insert(SettingBinding::global(SETTING_LOGIN_REMEMBER_PASSWORD));
    let remember_note = spawn_label(
        &mut commands,
        remember_row,
        UiLabel::literal(String::new()),
        palette.text_muted,
        PANEL_FONT,
    );
    commands
        .entity(remember_note)
        .insert(Name::new("login-remember-note"));

    let start_row = labeled(&mut commands, "login-start-label");
    let start_labels: Vec<String> = START_OPTIONS.iter().map(|&key| key.to_owned()).collect();
    let start = spawn_combo(
        &mut commands,
        start_row,
        &ComboSpec {
            element: "login-start",
            labels: &start_labels,
            active: start_index,
            tab_index: 8,
            font_size: PANEL_FONT,
            translate_labels: true,
        },
    );
    let start_region = spawn_text_input(
        &mut commands,
        start_row,
        &TextInputSpec {
            initial: start_region,
            font_size: PANEL_FONT,
            width_glyphs: 14.0,
            tab_index: 9,
            name_key: Some("login-start-region-name"),
            ..TextInputSpec::new("login-start-region", TextInputKind::Line)
        },
    );

    let connect = spawn_button(
        &mut commands,
        panel,
        ButtonSpec::bordered(UiLabel::key("login-connect"), "login-connect")
            .kind(ButtonKind::Headless)
            .tab_index(10)
            .font_size(PANEL_FONT)
            .primary(),
    )
    .button;
    commands
        .entity(connect)
        .insert(LoginPanelRequest::Connect)
        .observe(on_request_button);

    // The second factor, shown while a challenge waits for its token.
    let token_row = spawn_labeled_row(
        &mut commands,
        panel,
        LabeledRowSpec::new(UiLabel::key("login-token-label"))
            .font_size(PANEL_FONT)
            .label_color(palette.text_muted)
            .label_min_width(Val::Px(LABEL_MIN_WIDTH))
            .name("login-token-row"),
    )
    .row;
    commands.entity(token_row).insert(UiPanelShown(false));
    let token = spawn_text_input(
        &mut commands,
        token_row,
        &TextInputSpec {
            font_size: PANEL_FONT,
            width_glyphs: 10.0,
            tab_index: 11,
            ..TextInputSpec::new("login-token", TextInputKind::Line)
        },
    );
    let _submit = spawn_request_button(
        &mut commands,
        token_row,
        "login-submit-token",
        LoginPanelRequest::SubmitToken,
        12,
    );

    let notice = spawn_label(
        &mut commands,
        panel,
        UiLabel::literal(String::new()),
        palette.text_primary,
        PANEL_FONT,
    );
    commands.entity(notice).insert(Name::new("login-notice"));

    commands.insert_resource(grids);
    commands.insert_resource(LoginPanelUi {
        page,
        grid,
        remove_grid,
        grid_uri,
        avatar,
        password,
        remember_note,
        start,
        start_region,
        connect,
        notice,
        token_row,
        token,
    });
}

/// Spawn one of the panel's bordered buttons, labelled by `key`, which asks
/// for `request` when activated.
fn spawn_request_button(
    commands: &mut Commands,
    parent: Entity,
    key: &'static str,
    request: LoginPanelRequest,
    tab_index: i32,
) -> Entity {
    let button = spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(UiLabel::key(key), key)
            .kind(ButtonKind::Headless)
            .tab_index(tab_index)
            .font_size(PANEL_FONT),
    )
    .button;
    commands
        .entity(button)
        .insert(request)
        .observe(on_request_button);
    button
}

/// A panel button was activated (a click, or `Enter` / `Space` on it): ask for
/// what it does.
fn on_request_button(
    activate: On<Activate>,
    buttons: Query<&LoginPanelRequest>,
    mut requests: MessageWriter<LoginPanelRequest>,
) {
    if let Ok(request) = buttons.get(activate.entity) {
        requests.write(*request);
    }
}

/// `Enter` in one of the panel's fields does what its row's button does: in
/// the name or password field it logs in, in the token field it answers the
/// challenge, in the login URI field it adds the grid.
fn submit_on_enter(
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    ui: Res<LoginPanelUi>,
    fields: Query<&EditableText>,
    mut requests: MessageWriter<LoginPanelRequest>,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    let Some(focused) = focus.get() else {
        return;
    };
    // An IME composing in the field owns its `Enter`.
    if fields.get(focused).is_ok_and(EditableText::is_composing) {
        return;
    }
    let request = if focused == ui.avatar || focused == ui.password {
        LoginPanelRequest::Connect
    } else if focused == ui.token {
        LoginPanelRequest::SubmitToken
    } else if focused == ui.grid_uri {
        LoginPanelRequest::AddGrid
    } else {
        return;
    };
    requests.write(request);
    keyboard.clear_just_pressed(KeyCode::Enter);
}

/// The text of the field `entity`, trimmed, or empty.
fn field_text(fields: &Query<&EditableText>, entity: Entity) -> String {
    fields
        .get(entity)
        .map(|field| field.value().to_string().trim().to_owned())
        .unwrap_or_default()
}

/// Split an avatar name as the login form takes it: `First Last`, a username
/// alone (whose last name is `Resident`), or Second Life's `first.last`.
/// `None` for an empty name or one of more than two words.
fn split_avatar_name(name: &str) -> Option<(String, String)> {
    let mut words = name.split_whitespace();
    let first = words.next()?;
    let last = words.next();
    if words.next().is_some() {
        return None;
    }
    let (first, last) = match last {
        Some(last) => (first, last),
        None => first.split_once('.').unwrap_or((first, DEFAULT_LAST_NAME)),
    };
    if first.is_empty() || last.is_empty() {
        return None;
    }
    Some((first.to_owned(), last.to_owned()))
}

/// What the panel's start location says, or the notice that says what is
/// missing.
fn chosen_start(choice: usize, region: &str) -> Result<StartLocation, Notice> {
    match choice {
        0 => Ok(StartLocation::Last),
        1 => Ok(StartLocation::Home),
        _ if region.is_empty() => Err(Notice::Key("login-notice-no-region")),
        _ => format!("uri:{region}&128&128&0").parse().map_err(
            |error: sl_client_bevy::StartLocationParseError| {
                Notice::Message("login-notice-bad-region", error.to_string())
            },
        ),
    }
}

/// What a login sends, read off the panel.
#[derive(bevy::ecs::system::SystemParam)]
struct PanelInput<'w, 's> {
    /// The panel's parts.
    ui: Res<'w, LoginPanelUi>,
    /// The text fields.
    fields: Query<'w, 's, &'static EditableText>,
    /// The password.
    passwords: Query<'w, 's, &'static PasswordField>,
    /// The combos' choices.
    combos: Query<'w, 's, &'static ComboSelection>,
    /// The grids.
    grids: Res<'w, GridList>,
    /// The viewer this login reports.
    form: Res<'w, LoginForm>,
    /// Where the viewer stores what a login decides.
    paths: Res<'w, crate::assembly::ViewerPaths>,
}

/// Log in with what the panel holds, when asked and nothing is live yet.
fn connect(
    mut requests: MessageReader<LoginPanelRequest>,
    input: PanelInput,
    status: Res<SlSessionStatus>,
    mut state: ResMut<LoginPanelState>,
    mut settings: ResMut<ViewerSettings>,
    mut logins: MessageWriter<SlLogin>,
    mut commands: Commands,
) {
    if !requests
        .read()
        .any(|request| *request == LoginPanelRequest::Connect)
    {
        return;
    }
    if status.is_live() {
        return;
    }
    let ui = &input.ui;
    let combo = |entity: Entity| input.combos.get(entity).map_or(0, |choice| choice.active);
    let Some(grid) = input.grids.grids().get(combo(ui.grid)) else {
        state.notice = Notice::Key("login-notice-no-grid");
        return;
    };
    let avatar = field_text(&input.fields, ui.avatar);
    let Some((first, last)) = split_avatar_name(&avatar) else {
        state.notice = Notice::Key("login-notice-no-avatar");
        return;
    };
    let password = input
        .passwords
        .get(ui.password)
        .map(|field| field.secret().to_owned())
        .unwrap_or_default();
    if password.is_empty() {
        state.notice = Notice::Key("login-notice-no-password");
        return;
    }
    let start = match chosen_start(combo(ui.start), &field_text(&input.fields, ui.start_region)) {
        Ok(start) => start,
        Err(notice) => {
            state.notice = notice;
            return;
        }
    };
    state.saved.attempt = Some((
        password_key(&grid.login_uri, &first, &last),
        Secret::new(password.clone()),
    ));
    let params = LoginParams {
        login_uri: grid.login_uri.clone(),
        request: LoginRequest::new(
            first,
            last,
            password,
            start,
            input.form.channel.clone(),
            input.form.version.clone(),
        ),
    };
    info!(
        "logging in as {} {} to {} from the login screen",
        params.request.first_name, params.request.last_name, params.login_uri
    );
    let scope = LoginScope::of(&params, &input.paths);
    let pending = PendingLogin {
        params,
        account_dirs: scope.account_dirs.clone(),
    };
    logins.write(pending.message());
    commands.insert_resource(pending);
    // What the login decides beside the session, in place of the placeholders
    // the screen held; the automation hello is only kept where it is read.
    commands.queue(move |world: &mut World| {
        let automated = world.contains_resource::<sl_viewer_automation::AutomationIdentity>();
        scope.insert(world, automated);
    });
    settings.set(
        Scope::Global,
        SETTING_LOGIN_LAST_GRID,
        SettingValue::String(grid.login_uri.to_string()),
    );
    settings.set(
        Scope::Global,
        SETTING_LOGIN_LAST_AVATAR,
        SettingValue::String(avatar),
    );
    state.notice = Notice::None;
    state.challenge = None;
}

/// Answer the waiting second-factor challenge with the token field: the same
/// login again, with the token folded in.
fn submit_token(
    mut requests: MessageReader<LoginPanelRequest>,
    ui: Res<LoginPanelUi>,
    fields: Query<&EditableText>,
    status: Res<SlSessionStatus>,
    pending: Option<ResMut<PendingLogin>>,
    mut state: ResMut<LoginPanelState>,
    mut logins: MessageWriter<SlLogin>,
) {
    if !requests
        .read()
        .any(|request| *request == LoginPanelRequest::SubmitToken)
    {
        return;
    }
    let (Some(WaitingChallenge { mfa_hash }), Some(mut pending)) =
        (state.challenge.clone(), pending)
    else {
        return;
    };
    if status.is_live() {
        return;
    }
    let token = field_text(&fields, ui.token);
    if token.is_empty() {
        state.notice = Notice::Key("login-notice-no-token");
        return;
    }
    pending.params.request = pending.params.request.clone().with_mfa(token, mfa_hash);
    logins.write(pending.message());
    state.challenge = None;
    state.notice = Notice::None;
}

/// The grid list and the combo that shows it, which adding or removing a grid
/// changes together.
#[derive(bevy::ecs::system::SystemParam)]
struct GridEdit<'w, 's> {
    /// The grids.
    grids: ResMut<'w, GridList>,
    /// The combos' choices, the grid combo's among them.
    combos: Query<'w, 's, &'static mut ComboSelection>,
    /// The combo's new options.
    options: MessageWriter<'w, SetComboOptions>,
}

impl GridEdit<'_, '_> {
    /// Show the list in the grid combo, with the grid at `index` chosen.
    fn show(&mut self, combo: Entity, index: usize) {
        self.options.write(SetComboOptions::new(
            combo,
            self.grids
                .grids()
                .iter()
                .map(|grid| grid.name.clone())
                .collect(),
        ));
        if let Ok(mut choice) = self.combos.get_mut(combo) {
            choice.active = index;
        }
    }
}

/// The text fields, and the text contexts a programmatic edit of one lays it
/// out again with.
#[derive(bevy::ecs::system::SystemParam)]
struct FieldEdit<'w, 's> {
    /// The text fields.
    fields: Query<'w, 's, &'static mut EditableText>,
    /// The font context.
    font_cx: ResMut<'w, FontCx>,
    /// The layout context.
    layout_cx: ResMut<'w, LayoutCx>,
}

/// Add the grid at the login URI field: fetch its `get_grid_info` on a thread
/// of its own, then list it under the name it gives (or its host, when it
/// answers none) and choose it.
fn add_grid(
    mut requests: MessageReader<LoginPanelRequest>,
    ui: Res<LoginPanelUi>,
    mut text: FieldEdit,
    mut edit: GridEdit,
    mut state: ResMut<LoginPanelState>,
    mut fetching: Local<Option<GridFetch>>,
) {
    let asked = requests
        .read()
        .any(|request| *request == LoginPanelRequest::AddGrid);
    if asked && fetching.is_none() {
        let typed = text
            .fields
            .get(ui.grid_uri)
            .map(|field| field.value().to_string().trim().to_owned())
            .unwrap_or_default();
        match typed.parse::<Url>() {
            Ok(login_uri) => {
                let (sender, answer) = std::sync::mpsc::channel();
                let fetched = login_uri.clone();
                let spawned = std::thread::Builder::new()
                    .name("sl-viewer-grid-info".to_owned())
                    .spawn(move || {
                        // Nobody is left to tell if the viewer has gone.
                        sender
                            .send(fetch_grid_info(&fetched).map_err(|error| error.to_string()))
                            .ok();
                    });
                match spawned {
                    Ok(_thread) => {
                        *fetching = Some(GridFetch { login_uri, answer });
                        state.notice = Notice::Key("login-notice-fetching-grid");
                    }
                    Err(error) => {
                        state.notice =
                            Notice::Message("login-notice-grid-failed", error.to_string());
                    }
                }
            }
            Err(error) => {
                state.notice = Notice::Message("login-notice-bad-grid-uri", error.to_string());
            }
        }
    }
    let Some(waiting) = fetching.as_ref() else {
        return;
    };
    let answer = match waiting.answer.try_recv() {
        Ok(answer) => answer,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => {
            Err("the grid-info thread ended without an answer".to_owned())
        }
    };
    let login_uri = waiting.login_uri.clone();
    *fetching = None;
    let info = match answer {
        Ok(info) => info,
        Err(error) => {
            state.notice = Notice::Message("login-notice-grid-failed", error);
            return;
        }
    };
    let name = info
        .grid_name()
        .map(str::to_owned)
        .or_else(|| login_uri.host_str().map(str::to_owned))
        .unwrap_or_else(|| login_uri.to_string());
    state.notice = Notice::Message("login-notice-grid-added", name.clone());
    let index = match edit.grids.add(name, login_uri.clone()) {
        Ok(index) => index,
        Err(error) => {
            // Listed for this run regardless; say it will not be kept.
            state.notice = Notice::Message("login-notice-grid-unsaved", error.to_string());
            edit.grids.position(&login_uri).unwrap_or(0)
        }
    };
    edit.show(ui.grid, index);
    let FieldEdit {
        fields,
        font_cx,
        layout_cx,
    } = &mut text;
    if let Ok(mut field) = fields.get_mut(ui.grid_uri) {
        clear_field(&mut field, font_cx, layout_cx);
    }
}

/// Empty a field the way a programmatic edit must
/// ([`crate::web_floater`]'s idiom): set the text, then lay it out again with
/// the caret at its end.
fn clear_field(field: &mut EditableText, font_cx: &mut FontCx, layout_cx: &mut LayoutCx) {
    field.editor.set_text("");
    let mut driver = field.editor.driver(font_cx, layout_cx);
    driver.refresh_layout();
    driver.move_to_text_end();
}

/// Remove the chosen grid, when it is the user's own.
fn remove_grid(
    mut requests: MessageReader<LoginPanelRequest>,
    ui: Res<LoginPanelUi>,
    mut edit: GridEdit,
    mut state: ResMut<LoginPanelState>,
) {
    if !requests
        .read()
        .any(|request| *request == LoginPanelRequest::RemoveGrid)
    {
        return;
    }
    let Ok(chosen) = edit.combos.get(ui.grid).map(|choice| choice.active) else {
        return;
    };
    match edit.grids.remove(chosen) {
        Ok(removed) => {
            state.notice = Notice::Message("login-notice-grid-removed", removed.name);
            edit.show(ui.grid, 0);
        }
        Err(error) => {
            state.notice = Notice::Message("login-notice-grid-not-removed", error.to_string());
        }
    }
}

/// The notice for a login that ended before the agent arrived.
fn ended_notice(reason: &SessionDisconnectReason) -> Notice {
    match reason {
        SessionDisconnectReason::LoginFailed { message, .. } => {
            Notice::Message("login-notice-refused", message.clone())
        }
        SessionDisconnectReason::Timeout => Notice::Key("login-notice-timeout"),
        SessionDisconnectReason::HandshakeFailed => Notice::Key("login-notice-handshake"),
        SessionDisconnectReason::ProtocolError => Notice::Key("login-notice-unreachable"),
        SessionDisconnectReason::Kicked { message } => {
            Notice::Message("login-notice-kicked", message.clone())
        }
        // A reason added after this panel was written still says it ended.
        other => Notice::Message("login-notice-ended", format!("{other:?}")),
    }
}

/// Fold what the login brings into the panel's notice: a refusal, a lost
/// login, or a second-factor challenge to answer here (a viewer with a
/// second-factor source of its own answers that itself). A refusal for a bad
/// password forgets the password kept for that account.
fn follow_the_login(
    (mut events, mut rejected, mut challenges): (
        MessageReader<SlEvent>,
        MessageReader<SlLoginRejected>,
        MessageReader<SlMfaChallenge>,
    ),
    second_factor: Option<Res<SecondFactorSource>>,
    session: Res<crate::session::ViewerSession>,
    mut state: ResMut<LoginPanelState>,
    mut secrets: MessageWriter<SecretRequest>,
) {
    for event in events.read() {
        let SlSessionEvent::Disconnected(reason) = &event.0 else {
            continue;
        };
        if session.agent_in_world() {
            continue;
        }
        state.notice = ended_notice(reason);
        if let SessionDisconnectReason::LoginFailed { reason, .. } = reason
            && reason == LoginServer::BAD_CREDENTIALS_REASON
            && let Some((key, _refused)) = state.saved.attempt.take()
        {
            state.saved.filled = false;
            secrets.write(SecretRequest {
                key,
                op: SecretOp::Forget,
            });
        }
    }
    for rejection in rejected.read() {
        state.notice = Notice::Message("login-notice-refused", rejection.0.message.clone());
    }
    for challenge in challenges.read() {
        if second_factor.is_some() {
            continue;
        }
        state.challenge = Some(WaitingChallenge {
            mfa_hash: challenge.0.mfa_hash.clone(),
        });
        state.notice = Notice::Message("login-notice-token", challenge.0.message.clone());
    }
}

/// The password field, and the text contexts filling it lays it out again
/// with.
#[derive(bevy::ecs::system::SystemParam)]
struct PasswordFill<'w, 's> {
    /// The text fields' editors.
    fields: Query<'w, 's, &'static mut EditableText>,
    /// The password fields' secrets.
    passwords: Query<'w, 's, &'static mut PasswordField>,
    /// The font context.
    font_cx: ResMut<'w, FontCx>,
    /// The layout context.
    layout_cx: ResMut<'w, LayoutCx>,
}

impl PasswordFill<'_, '_> {
    /// Fill the password field `field` with `secret`, or empty it with `""`.
    fn fill(&mut self, field: Entity, secret: &str) {
        if let (Ok(mut editable), Ok(mut password)) =
            (self.fields.get_mut(field), self.passwords.get_mut(field))
        {
            password.fill(
                secret,
                &mut editable,
                &mut self.font_cx,
                &mut self.layout_cx,
            );
        }
    }

    /// The text of the field `field`, trimmed, or empty.
    fn text(&self, field: Entity) -> String {
        self.fields
            .get(field)
            .map(|editable| editable.value().to_string().trim().to_owned())
            .unwrap_or_default()
    }
}

/// The secret-store key of the grid chosen at `grid_choice` and the avatar
/// named `avatar`, or `None` while either is missing.
fn panel_key(grids: &GridList, grid_choice: usize, avatar: &str) -> Option<SecretKey> {
    let grid = grids.grids().get(grid_choice)?;
    let (first, last) = split_avatar_name(avatar)?;
    Some(password_key(&grid.login_uri, &first, &last))
}

/// While Remember password is ticked, ask the store for the password of the
/// grid and avatar the panel shows, whenever they change (the name once its
/// field is left, not at every key). Unticking it forgets that password.
fn ask_for_the_saved_password(
    (ui, settings, focus): (Res<LoginPanelUi>, Res<ViewerSettings>, Res<InputFocus>),
    (grids, combos): (Res<GridList>, Query<&ComboSelection>),
    mut fill: PasswordFill,
    mut state: ResMut<LoginPanelState>,
    mut secrets: MessageWriter<SecretRequest>,
    mut was_ticked: Local<Option<bool>>,
) {
    let ticked = remembering(&settings);
    let untoggled = was_ticked.replace(ticked) == Some(true) && !ticked;
    let grid_choice = combos.get(ui.grid).map_or(0, |choice| choice.active);
    let key = panel_key(&grids, grid_choice, &fill.text(ui.avatar));
    if untoggled && let Some(key) = key.clone() {
        secrets.write(SecretRequest {
            key,
            op: SecretOp::Forget,
        });
    }
    if !ticked {
        state.saved.asked = None;
        return;
    }
    if focus.get() == Some(ui.avatar) || key == state.saved.asked {
        return;
    }
    // A password filled for the account shown before is not this one's.
    if state.saved.filled {
        fill.fill(ui.password, "");
        state.saved.filled = false;
    }
    state.saved.asked.clone_from(&key);
    if let Some(key) = key {
        secrets.write(SecretRequest {
            key,
            op: SecretOp::Load,
        });
    }
}

/// Fill the password field with the password the store found for the
/// account the panel shows (unless its user has typed one of their own), and
/// note why, when the store could not keep or read one.
fn take_the_saved_password(
    mut answers: MessageReader<SecretAnswer>,
    ui: Res<LoginPanelUi>,
    mut fill: PasswordFill,
    mut state: ResMut<LoginPanelState>,
) {
    for answer in answers.read() {
        match &answer.outcome {
            SecretOutcome::Loaded(found) => {
                state.saved.problem = None;
                let Some(secret) = found else {
                    continue;
                };
                if state.saved.asked.as_ref() != Some(&answer.key) {
                    continue;
                }
                let typed = fill
                    .passwords
                    .get(ui.password)
                    .is_ok_and(|field| !field.secret().is_empty());
                if typed && !state.saved.filled {
                    continue;
                }
                fill.fill(ui.password, secret.expose());
                state.saved.filled = true;
            }
            SecretOutcome::Saved | SecretOutcome::Forgotten => state.saved.problem = None,
            SecretOutcome::Failed(error) => {
                warn!("the login screen's password could not be kept or read: {error}");
                state.saved.problem = Some(error.to_string());
            }
        }
    }
}

/// Once the login arrives, keep the password it was sent with, if Remember
/// password is ticked.
fn keep_the_password(
    session: Res<crate::session::ViewerSession>,
    settings: Res<ViewerSettings>,
    mut state: ResMut<LoginPanelState>,
    mut secrets: MessageWriter<SecretRequest>,
) {
    if !session.agent_in_world() {
        return;
    }
    let Some((key, password)) = state.saved.attempt.take() else {
        return;
    };
    if remembering(&settings) {
        secrets.write(SecretRequest {
            key,
            op: SecretOp::Save(password),
        });
    }
}

/// Show the page until the agent arrives, and the permanent chrome from then
/// on.
fn show_the_page(
    ui: Res<LoginPanelUi>,
    session: Res<crate::session::ViewerSession>,
    mut pages: Query<&mut UiPanelShown, Without<ScreenChrome>>,
    mut chrome: Query<(Entity, Option<&mut UiPanelShown>), With<ScreenChrome>>,
    mut commands: Commands,
) {
    let on_screen = !session.agent_in_world();
    if let Ok(mut shown) = pages.get_mut(ui.page)
        && shown.0 != on_screen
    {
        shown.0 = on_screen;
    }
    for (entity, shown) in &mut chrome {
        match shown {
            Some(mut shown) if shown.0 == on_screen => shown.0 = !on_screen,
            Some(_) => {}
            None => {
                commands.entity(entity).insert(UiPanelShown(!on_screen));
            }
        }
    }
}

/// Keep the panel's controls in step with where the login stands: Log In
/// refused while a login is live, the token row shown while a challenge
/// waits, and Remove refused for a built-in grid.
fn reflect_the_panel(
    (ui, status, state): (
        Res<LoginPanelUi>,
        Res<SlSessionStatus>,
        Res<LoginPanelState>,
    ),
    (grids, combos): (Res<GridList>, Query<&ComboSelection>),
    mut rows: Query<&mut UiPanelShown>,
    disabled: Query<Has<InteractionDisabled>>,
    mut commands: Commands,
) {
    let waiting = state.challenge.is_some() && !status.is_live();
    if let Ok(mut shown) = rows.get_mut(ui.token_row)
        && shown.0 != waiting
    {
        shown.0 = waiting;
    }
    let builtin = combos
        .get(ui.grid)
        .ok()
        .and_then(|choice| grids.grids().get(choice.active))
        .is_none_or(|grid| grid.builtin);
    for (entity, refuse) in [(ui.connect, status.is_live()), (ui.remove_grid, builtin)] {
        if disabled.get(entity).is_ok_and(|held| held != refuse) {
            if refuse {
                commands.entity(entity).insert(InteractionDisabled);
            } else {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        }
    }
}

/// What the notice line says now: the login's progress while one is live,
/// else the panel's last notice.
fn current_notice(status: SlSessionStatus, in_world: bool, state: &LoginPanelState) -> Notice {
    match status {
        SlSessionStatus::LoggingIn => Notice::Key("login-notice-logging-in"),
        SlSessionStatus::Running if !in_world => Notice::Key("login-notice-connecting"),
        SlSessionStatus::Idle | SlSessionStatus::Running | SlSessionStatus::Ended => {
            state.notice.clone()
        }
    }
}

/// Write the notice line when what it says changes, or the locale does.
fn write_the_notice(
    ui: Res<LoginPanelUi>,
    status: Res<SlSessionStatus>,
    session: Res<crate::session::ViewerSession>,
    mut state: ResMut<LoginPanelState>,
    translator: Translator,
    mut texts: Query<&mut Text>,
) {
    let notice = current_notice(*status, session.agent_in_world(), &state);
    if state.shown.as_ref() == Some(&notice) && !translator.changed() {
        return;
    }
    let text = match &notice {
        Notice::None => String::new(),
        Notice::Key(key) => translator.get(key),
        Notice::Message(key, message) => {
            translator.format(key, &TransArgs::new().text("message", message))
        }
    };
    if let Ok(mut shown) = texts.get_mut(ui.notice) {
        shown.0 = text;
    }
    state.shown = Some(notice);
}

/// Write the line under Remember password when what it says changes, or the
/// locale does: why no password can be kept, or nothing.
fn write_the_remember_note(
    ui: Res<LoginPanelUi>,
    mut state: ResMut<LoginPanelState>,
    translator: Translator,
    mut texts: Query<&mut Text>,
) {
    let problem = ShownProblem(state.saved.problem.clone());
    if state.saved.shown.as_ref() == Some(&problem) && !translator.changed() {
        return;
    }
    let text = problem.0.as_ref().map_or_else(String::new, |reason| {
        translator.format(
            "login-remember-unavailable",
            &TransArgs::new().text("message", reason),
        )
    });
    if let Ok(mut shown) = texts.get_mut(ui.remember_note) {
        shown.0 = text;
    }
    state.saved.shown = Some(problem);
}

/// Route the pre-login menu's picks: Preferences, About, and Exit (a quit,
/// which with nothing live exits at once).
fn handle_login_menu(
    mut actions: MessageReader<UiAction>,
    floaters: Query<(Entity, &Floater)>,
    mut panels: Query<&mut UiPanelShown>,
    mut quit: MessageWriter<crate::session::QuitRequested>,
) {
    for action in actions.read() {
        if action.element != LOGIN_MENU_ELEMENT {
            continue;
        }
        match action.action {
            "toggle-preferences" => toggle_floater(
                &floaters,
                &mut panels,
                crate::preferences::PREFERENCES_FLOATER_ID,
            ),
            "toggle-about" => toggle_floater(
                &floaters,
                &mut panels,
                crate::about_floater::ABOUT_FLOATER_ID,
            ),
            "quit" => {
                quit.write(crate::session::QuitRequested);
            }
            _other => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Notice, START_REGION, chosen_start, split_avatar_name, start_choice};
    use pretty_assertions::assert_eq;
    use sl_client_bevy::StartLocation;

    /// A name is `First Last`, a username alone, or `first.last`.
    #[test]
    fn an_avatar_name_splits_as_the_login_form_takes_it() {
        let split = |name: &str| split_avatar_name(name);
        let pair = |first: &str, last: &str| Some((first.to_owned(), last.to_owned()));
        assert_eq!(split("Avatar Tester"), pair("Avatar", "Tester"));
        assert_eq!(split("  Avatar   Tester "), pair("Avatar", "Tester"));
        assert_eq!(split("avatartester"), pair("avatartester", "Resident"));
        assert_eq!(split("avatar.tester"), pair("avatar", "tester"));
        assert_eq!(split(""), None);
        assert_eq!(split("one two three"), None);
        assert_eq!(split(".tester"), None);
    }

    /// The start combo reads back as the location it names, and a region
    /// start needs a region.
    #[test]
    fn the_start_combo_names_a_start_location() {
        assert_eq!(chosen_start(0, ""), Ok(StartLocation::Last));
        assert_eq!(chosen_start(1, "ignored"), Ok(StartLocation::Home));
        assert_eq!(
            chosen_start(START_REGION, ""),
            Err(Notice::Key("login-notice-no-region"))
        );
        assert!(matches!(
            chosen_start(START_REGION, "Default Region"),
            Ok(StartLocation::Region { region, .. }) if region == "Default Region"
        ));
        assert_eq!(start_choice("home"), (1, String::new()));
        assert_eq!(
            start_choice("uri:Default Region&128&128&0"),
            (START_REGION, "Default Region".to_owned())
        );
        assert_eq!(start_choice("nonsense"), (0, String::new()));
    }
}
