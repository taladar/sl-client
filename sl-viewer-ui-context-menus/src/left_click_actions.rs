//! A left click's **click action**, carried out (`viewer-left-click-click-actions`).
//!
//! The left-click pick (`crate::hud_pick`) decides what a click on a prim does
//! ([`crate::world_api::left_click`]) and, for a click action, writes a
//! [`ClickActionRequest`] instead of touching. This answers it with the same
//! machinery the object pie's slices use, which is why it lives beside them:
//!
//! - **Sit** — `AgentRequestSit` on the picked prim, at the offset a *Sit Here*
//!   on the same spot would send (`object_menu::sit_here_offset`), the reference's
//!   `handle_object_sit_or_stand` with the pick's `mObjectOffset`.
//! - **Open** — the object's (root's) contents in the Object Contents floater,
//!   the pie's *Open*.
//! - **Zoom** — the camera frames the picked prim ([`FrameObject`]), the
//!   reference's `CLICK_ACTION_ZOOM`, which fits the prim's bounding box to the
//!   view.
//! - **Open media** — the parcel's media URL in the web browser, the
//!   reference's `handle_click_action_open_media` once no media plays on the
//!   face (none ever does here: parcel media is not played yet,
//!   `viewer-video-playback`).
//! - **Pay**, **buy** and **play** have nothing behind them yet — no pay
//!   dialog, no buy floater (`viewer-money-economy-ui`) and no parcel media
//!   player — so each says so in a notification rather than doing nothing or
//!   touching, which is what the reference would not do either.

use bevy::prelude::*;
use sl_client_bevy::{Command, SlAgentParcel, SlCommand};

use crate::edit_contents::OpenObjectContents;
use crate::intents::OpenWebBrowser;
use crate::notifications::ShowNotification;
use crate::object_menu::{SeatPoses, sit_here_offset};
use crate::objects::ObjectRayHit;
use crate::world_api::{ClickActionRequest, ClickedAction, FrameObject, ObjectState};

/// The notification a pay click action raises while there is no pay dialog.
pub const PAY_UNAVAILABLE: &str = "ClickActionPayUnavailable";

/// The notification a buy click action raises while there is no buy floater.
pub const BUY_UNAVAILABLE: &str = "ClickActionBuyUnavailable";

/// The notification a play click action raises while parcel media does not
/// play.
pub const PLAY_UNAVAILABLE: &str = "ClickActionPlayUnavailable";

/// Register the click-action answer (called from the object menu's plugin).
pub(crate) fn build(app: &mut App) {
    app.add_message::<ClickActionRequest>()
        .add_message::<FrameObject>()
        .add_message::<OpenWebBrowser>()
        .add_message::<OpenObjectContents>()
        .add_message::<ShowNotification>()
        .add_systems(Update, carry_out_click_actions);
}

/// Where a click action goes, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
struct ClickActionOut<'w> {
    /// The wire: a sit.
    commands: MessageWriter<'w, SlCommand>,
    /// The Object Contents floater an open opens.
    open_contents: MessageWriter<'w, OpenObjectContents>,
    /// The camera framing a zoom asks for.
    frames: MessageWriter<'w, FrameObject>,
    /// The web browser an open-media opens.
    browser: MessageWriter<'w, OpenWebBrowser>,
    /// The notification an action with nothing behind it raises.
    notify: MessageWriter<'w, ShowNotification>,
}

/// Carry out each left click's click action. See the
/// [module documentation](self).
fn carry_out_click_actions(
    mut requests: MessageReader<ClickActionRequest>,
    state: Res<ObjectState>,
    parcel: Option<Res<SlAgentParcel>>,
    seats: SeatPoses,
    globals: Query<&GlobalTransform>,
    mut out: ClickActionOut,
) {
    for request in requests.read() {
        let summary = request.summary;
        match request.action {
            ClickedAction::Sit => {
                let hit = ObjectRayHit {
                    summary,
                    surface: request.surface.clone(),
                };
                out.commands.write(SlCommand(Command::Sit {
                    target: summary.picked_full,
                    offset: sit_here_offset(&hit, &state, &seats.seat_poses, &seats.seat_cameras),
                }));
            }
            ClickedAction::Open => {
                out.open_contents.write(OpenObjectContents {
                    scoped: summary.root_scoped,
                    full: summary.root_full,
                });
            }
            ClickedAction::Zoom => {
                let Some(tracked) = state.objects().get(&summary.picked_scoped) else {
                    continue;
                };
                let Ok(boxed) = globals.get(tracked.geometry) else {
                    continue;
                };
                // The geometry holder carries the prim's scale, so its unit
                // cube is the prim's bounding box.
                let boxed = boxed.affine();
                let center = boxed.transform_point3(Vec3::ZERO);
                let corner = boxed.transform_point3(Vec3::splat(0.5));
                out.frames.write(FrameObject {
                    center,
                    radius: center.distance(corner),
                    passes_through: vec![tracked.entity],
                });
            }
            ClickedAction::OpenMedia => {
                let url = parcel
                    .as_ref()
                    .and_then(|parcel| parcel.current.as_ref())
                    .and_then(|parcel| parcel.media_url.as_ref());
                if let Some(url) = url {
                    out.browser
                        .write(OpenWebBrowser::new_window(Some(url.to_string())));
                } else {
                    info!("left click: open media, but the parcel has no media URL");
                }
            }
            ClickedAction::Pay => {
                out.notify.write(ShowNotification::new(PAY_UNAVAILABLE));
            }
            ClickedAction::Buy => {
                out.notify.write(ShowNotification::new(BUY_UNAVAILABLE));
            }
            ClickedAction::Play => {
                out.notify.write(ShowNotification::new(PLAY_UNAVAILABLE));
            }
        }
    }
}
