//! The scene a scripted avatar plays in: four prims whose scripts ask an avatar
//! something, and the avatar that answers them.
//!
//! Each prim is one of the conversations a script can only have with an avatar
//! on the other end of a circuit ([`crate::scripted_avatars`]):
//!
//! - the **pose ball** ([`POSE_BALL`]) asks whoever sits on it for permission
//!   to animate them — a grant both grids give a sitter without putting the
//!   question to it — and plays the animation it carries ([`POSE`]) on them;
//! - the **vendor** ([`VENDOR`]) is clicked to pay, offers L$50, and thanks
//!   whoever pays it, by name and amount;
//! - the **permission prim** ([`PERMISSION_PRIM`]) asks whoever touches it for
//!   [`PERMISSION_PRIM_ASKS`] and says what it was granted;
//! - the **menu** ([`MENU`]) shows whoever touches it a dialog of three colours
//!   ([`MENU_BUTTONS`]) and says which one came back.
//!
//! Every answer a script hears is said on channel 0, so a case reads what the
//! script saw from the chat a client is told, and the avatar's transcript says
//! what it answered.
//!
//! [`sitter`] is the avatar the named scene `scripted-avatar` brings: it sits
//! on the pose ball and grants whatever it is asked, so a frame of the scene
//! shows an avatar seated and posing.
//!
//! The pose is the pose ball's own animation, as a real pose ball's is, and
//! not a built-in: a built-in like `dance1` is fetched by a viewer over
//! `ViewerAsset` like any uploaded animation, and the fake grid serves no
//! Linden animation assets, so a viewer of ours could not play one here.

use sl_proto::{AssetKey, ClickAction, RegionLocalObjectId};
use sl_types::key::{AgentKey, InventoryKey, ObjectKey, OwnerKey};
use sl_types::lsl::Vector;

use super::RegionFixture;
use crate::scripted_avatars::{AnswerPolicy, AvatarAction, ScriptedAvatar, Target};

/// The pose ball's name.
pub const POSE_BALL: &str = "Pose ball";

/// The vendor's name.
pub const VENDOR: &str = "Vendor";

/// The permission prim's name.
pub const PERMISSION_PRIM: &str = "Permission prim";

/// The menu's name.
pub const MENU: &str = "Menu";

/// The name of the animation in the pose ball's contents.
pub const POSE: &str = "Pose";

/// The asset of [`POSE`]: two seconds of the torso twisting and back,
/// looping ([`sl_test_assets::anim::chest_twist_animation_asset`]), so two
/// frames a second apart differ.
pub const POSE_ANIMATION: uuid::Uuid = uuid::Uuid::from_u128(0x5A_0001);

/// What the vendor asks to be paid, in L$.
pub const VENDOR_PRICE: i32 = 50;

/// What the permission prim asks for: `PERMISSION_TAKE_CONTROLS |
/// PERMISSION_TRACK_CAMERA`, neither of which it then uses.
pub const PERMISSION_PRIM_ASKS: i32 = 0x404;

/// The menu's buttons, in order.
pub const MENU_BUTTONS: [&str; 3] = ["Red", "Green", "Blue"];

/// The hidden channel the menu's dialog answers on.
pub const MENU_CHANNEL: i32 = -7071;

/// Who owns the four prims: no account, so nobody's balance moves but the
/// payer's and the owner's ledger entry.
const PROPS_OWNER: u128 = 0x5A_0000;

/// The pose ball's script: a sit target half a metre up, and on a sitter, a
/// request to animate them; granted, it stops the stock sit and plays
/// [`POSE`].
pub const POSE_BALL_SOURCE: &str = "default
{
    state_entry()
    {
        llSitTarget(<0.0, 0.0, 0.5>, ZERO_ROTATION);
    }

    changed(integer change)
    {
        if (change & CHANGED_LINK)
        {
            key sitter = llAvatarOnSitTarget();
            if (sitter != NULL_KEY)
            {
                llRequestPermissions(sitter, PERMISSION_TRIGGER_ANIMATION);
            }
        }
    }

    run_time_permissions(integer granted)
    {
        if (granted & PERMISSION_TRIGGER_ANIMATION)
        {
            llStopAnimation(\"sit\");
            llStartAnimation(\"Pose\");
            llSay(0, \"Posing \" + llKey2Name(llGetPermissionsKey()));
        }
    }
}
";

/// The vendor's script: clicked to pay, L$50 offered, and every payment
/// thanked.
pub const VENDOR_SOURCE: &str = "default
{
    state_entry()
    {
        llSetClickAction(CLICK_ACTION_PAY);
        llSetPayPrice(50, [50, 100, PAY_HIDE, PAY_HIDE]);
    }

    money(key payer, integer amount)
    {
        llSay(0, \"Thank you \" + llKey2Name(payer) + \" for L$\" + (string)amount);
    }
}
";

/// The permission prim's script: asks the toucher for
/// [`PERMISSION_PRIM_ASKS`] and says what came back.
pub const PERMISSION_PRIM_SOURCE: &str = "default
{
    touch_start(integer count)
    {
        llRequestPermissions(llDetectedKey(0), PERMISSION_TAKE_CONTROLS | PERMISSION_TRACK_CAMERA);
    }

    run_time_permissions(integer granted)
    {
        llSay(0, \"Granted \" + (string)granted);
    }
}
";

/// The menu's script: a dialog of [`MENU_BUTTONS`] for the toucher on
/// [`MENU_CHANNEL`], and every answer said aloud.
pub const MENU_SOURCE: &str = "default
{
    state_entry()
    {
        llListen(-7071, \"\", NULL_KEY, \"\");
    }

    touch_start(integer count)
    {
        llDialog(llDetectedKey(0), \"Pick a colour\", [\"Red\", \"Green\", \"Blue\"], -7071);
    }

    listen(integer channel, string name, key id, string message)
    {
        llSay(0, name + \" chose \" + message);
    }
}
";

/// One prim of the scene: its region-local id (its keys are minted from it),
/// its name, where it stands, its edge and its script.
struct Prop {
    /// Its region-local id.
    local_id: u32,
    /// Its name.
    name: &'static str,
    /// Its position's x, in region metres; the props stand in a row.
    x: f32,
    /// Its cube's edge, in metres.
    edge: f32,
    /// Its script's source.
    source: &'static str,
    /// What clicking it does, as it stands before its script first runs.
    click: ClickAction,
}

/// The four props, west to east.
const PROPS: [Prop; 4] = [
    Prop {
        local_id: 0x5A1,
        name: POSE_BALL,
        x: 126.0,
        edge: 0.3,
        source: POSE_BALL_SOURCE,
        click: ClickAction::Touch,
    },
    Prop {
        local_id: 0x5A2,
        name: VENDOR,
        x: 128.0,
        edge: 0.6,
        source: VENDOR_SOURCE,
        click: ClickAction::Pay,
    },
    Prop {
        local_id: 0x5A3,
        name: PERMISSION_PRIM,
        x: 130.0,
        edge: 0.6,
        source: PERMISSION_PRIM_SOURCE,
        click: ClickAction::Touch,
    },
    Prop {
        local_id: 0x5A4,
        name: MENU,
        x: 132.0,
        edge: 0.6,
        source: MENU_SOURCE,
        click: ClickAction::Touch,
    },
];

/// How far north of the region's centre the row stands, in metres: in front
/// of an avatar arriving at the centre and within a touch's reach.
const ROW_NORTH_M: f32 = 3.0;

/// The id a prop's key of kind `kind` is minted from.
fn minted(prop: &Prop, kind: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128((u128::from(prop.local_id) << 8) | kind)
}

/// The prop called `name`.
fn prop(name: &str) -> Option<&'static Prop> {
    PROPS.iter().find(|prop| prop.name == name)
}

/// The key of the prop called `name` ([`POSE_BALL`], [`VENDOR`],
/// [`PERMISSION_PRIM`] or [`MENU`]), or `None` for any other name.
#[must_use]
pub fn key(name: &str) -> Option<ObjectKey> {
    prop(name).map(|prop| ObjectKey::from(minted(prop, 0x5)))
}

/// Where the prop called `name` stands, or `None` for any other name.
#[must_use]
pub fn position(name: &str) -> Option<Vector> {
    prop(name).map(|prop| Vector {
        x: prop.x,
        y: 128.0 + ROW_NORTH_M,
        z: f32::from(crate::scenario::STOCK_TERRAIN_HEIGHT_M) + prop.edge / 2.0,
    })
}

/// The scene: the four props on the stock ground, each script running from
/// the start.
#[must_use]
pub fn region() -> RegionFixture {
    let owner = AgentKey::from(uuid::Uuid::from_u128(PROPS_OWNER));
    let mut fixture = RegionFixture::new();
    for prop in &PROPS {
        let local_id = RegionLocalObjectId(prop.local_id);
        let full_id = ObjectKey::from(minted(prop, 0x5));
        let asset = AssetKey::from(minted(prop, 0x6));
        let mut object = crate::world::box_prim(
            local_id,
            full_id,
            owner,
            position(prop.name).unwrap_or_else(|| Vector {
                x: prop.x,
                y: 128.0,
                z: f32::from(crate::scenario::STOCK_TERRAIN_HEIGHT_M),
            }),
            Vector {
                x: prop.edge,
                y: prop.edge,
                z: prop.edge,
            },
        );
        object.click_action = prop.click.to_code();
        let mut properties = crate::world::default_object_properties(&object);
        prop.name.clone_into(&mut properties.name);
        object.properties = Some(properties);
        fixture.world.objects.push(object);
        let item = sl_proto::TaskInventoryItem {
            item_id: InventoryKey::from(minted(prop, 0x7)),
            parent_task: full_id,
            asset_id: Some(asset),
            name: format!("{} script", prop.name),
            owner: OwnerKey::Agent(owner),
            creator_id: owner,
            last_owner_id: owner,
            ..crate::scenario::stock_script_item()
        };
        let mut contents = crate::world::TaskInventory::stated(1, vec![item.clone()]);
        let _running = contents.running.insert(item.item_id);
        if fixture
            .world
            .add_task_inventory(local_id, contents)
            .is_err()
        {
            tracing::error!(
                name = prop.name,
                "a scripted-avatar prop is not in its region"
            );
        }
        let _previous = fixture
            .assets
            .insert(asset, prop.source.as_bytes().to_vec());
    }
    give_the_pose(&mut fixture, owner);
    fixture
}

/// Puts [`POSE`] in the pose ball's contents, and its bytes in the scene's
/// assets.
fn give_the_pose(fixture: &mut RegionFixture, owner: AgentKey) {
    let Some((ball, key)) = prop(POSE_BALL).zip(key(POSE_BALL)) else {
        return;
    };
    let asset = AssetKey::from(POSE_ANIMATION);
    let item = sl_proto::TaskInventoryItem {
        item_id: InventoryKey::from(minted(ball, 0x8)),
        parent_task: key,
        asset_id: Some(asset),
        asset_type: sl_proto::AssetType::Animation,
        inv_type: sl_proto::InventoryType::Animation,
        name: POSE.to_owned(),
        owner: OwnerKey::Agent(owner),
        creator_id: owner,
        last_owner_id: owner,
        ..crate::scenario::stock_script_item()
    };
    let added = fixture
        .world
        .objects
        .contents_mut(RegionLocalObjectId(ball.local_id))
        .map(|contents| contents.items.push(item));
    if added.is_none() {
        tracing::error!("the pose ball is not in its region");
    }
    let _previous = fixture
        .assets
        .insert(asset, sl_test_assets::anim::chest_twist_animation_asset());
}

/// The avatar the named scene brings: dressed, it sits on the pose ball ten
/// ticks after it arrives (the props' scripts have set themselves up by then)
/// and grants whatever it is asked — so it ends up seated and posing.
#[must_use]
pub fn sitter() -> ScriptedAvatar {
    ScriptedAvatar::new("Scripted", "Sitter")
        .dressed()
        .answering(AnswerPolicy::agreeable())
        .after_ticks(10, AvatarAction::Sit(Target::Named(POSE_BALL.to_owned())))
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// Every prop is in the scene under its name and key, clicked as stated,
    /// with its one script running.
    #[test]
    fn every_prop_stands_named_and_scripted() {
        let fixture = region();
        for prop in &PROPS {
            let local_id = RegionLocalObjectId(prop.local_id);
            let object = fixture.world.objects.get(local_id);
            assert_eq!(
                object.as_ref().map(|object| object.full_id),
                key(prop.name),
                "{}",
                prop.name
            );
            assert_eq!(
                object
                    .as_ref()
                    .and_then(|object| object.properties.as_ref())
                    .map(|properties| properties.name.clone()),
                Some(prop.name.to_owned())
            );
            assert_eq!(
                fixture
                    .world
                    .objects
                    .contents(local_id)
                    .map(|contents| contents.running.len()),
                Some(1),
                "{}",
                prop.name
            );
        }
        assert_eq!(
            fixture
                .world
                .objects
                .get(RegionLocalObjectId(0x5A2))
                .map(|vendor| ClickAction::from_code(vendor.click_action)),
            Some(ClickAction::Pay)
        );
    }
}
