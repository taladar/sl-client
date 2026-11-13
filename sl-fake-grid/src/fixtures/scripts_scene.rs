//! The scripts scene: one prim per scripted behaviour, each in a row at a
//! named landmark with its script running, the behaviour counterpart of the
//! rendering [`catalogue`](super::catalogue()).
//!
//! West to east:
//!
//! - the **sensor** ([`SENSOR`]) sweeps four metres around itself and says who
//!   came near and who went away;
//! - the **touch prim** ([`TOUCH`]) says who touched it, which face and where;
//! - the **dialog prim** ([`DIALOG`]) menus the toucher for a colour, takes it
//!   and says who chose it;
//! - the **listener** ([`LISTENER`]) answers `hello` in local chat and takes
//!   `spin` and `stop` on channel [`LISTENER_CHANNEL`];
//! - the **vendor** ([`VENDOR`]) is clicked to pay and thanks its payer;
//! - the **pose ball** ([`POSE_BALL`]) seats, asks to animate its sitter and
//!   plays its own [`POSE`] on them;
//! - the **rezzer** ([`REZZER`]) rezzes the scripted [`SPARK`] it holds, which
//!   says the start parameter it was given and dies;
//! - the **notecard reader** ([`NOTECARD_READER`]) reads its notecard
//!   ([`NOTECARD`]) over the dataserver, line by line;
//! - the **timer prim** ([`TIMER`]) spins and steps through four colours and
//!   heights once a second — the prim a frame pair of the scene can compare;
//! - the **physical box** ([`PHYSICAL`]) falls on to its [`PLINTH`], says what
//!   it hit and, touched, is lifted and dropped again;
//! - the **linkset** ([`LINKSET`] and its two children) pings its children by
//!   link message when touched and says which link answered;
//! - the **broken script prim** ([`BROKEN`]) holds a script that does not
//!   compile, so a script editor's error path has a specimen. The props
//!   belong to an owner nobody logs in as, so this one prim and its script
//!   grant everyone modify: anyone's editor can save the script and be shown
//!   why it does not compile. That is an affordance of the fixture. OpenSim
//!   honours such a mask; Second Life never lets an owner grant it.
//!
//! The scripts are LSL files beside this module (`scripts_scene/`), not
//! strings in Rust, so each can be opened in a viewer's editor, saved back, and
//! run on a live grid as it stands. Every line a script says is said on
//! channel 0, so a person in the region, a test reading the region's chat and
//! a scripted avatar waiting for a line all read the same thing.
//!
//! The prims need nobody to come up and run: [`region`] is the scene a person
//! drives by hand. [`visitor`] is the avatar the named scene `scripts-avatars`
//! brings, which walks past the sensor, touches, menus, talks to, pays, rezzes,
//! reads, drops and pings its way along the row and ends seated on the pose
//! ball, posing.

use sl_proto::{AssetKey, AssetType, ClickAction, InventoryType, RegionLocalObjectId};
use sl_types::key::{AgentKey, InventoryKey, ObjectKey, OwnerKey};
use sl_types::lsl::Vector;

use super::RegionFixture;
use crate::scenario::HeldObject;
use crate::scripted_avatars::{AnswerPolicy, AvatarAction, DialogChoice, ScriptedAvatar, Target};

/// The sensor's name.
pub const SENSOR: &str = "Sensor";

/// The touch prim's name.
pub const TOUCH: &str = "Touch prim";

/// The dialog prim's name.
pub const DIALOG: &str = "Dialog prim";

/// The listener's name.
pub const LISTENER: &str = "Listener";

/// The vendor's name.
pub const VENDOR: &str = "Vendor";

/// The pose ball's name.
pub const POSE_BALL: &str = "Pose ball";

/// The rezzer's name.
pub const REZZER: &str = "Rezzer";

/// The notecard reader's name.
pub const NOTECARD_READER: &str = "Notecard reader";

/// The timer prim's name.
pub const TIMER: &str = "Timer prim";

/// The physical box's name.
pub const PHYSICAL: &str = "Physical box";

/// The name of the unscripted slab the physical box lands on.
pub const PLINTH: &str = "Plinth";

/// The linkset root's name.
pub const LINKSET: &str = "Linkset root";

/// The names of the linkset's two children, link numbers 2 and 3.
pub const LINKSET_CHILDREN: [&str; 2] = ["Link two", "Link three"];

/// The broken script prim's name.
pub const BROKEN: &str = "Broken script prim";

/// The name of the object the rezzer holds and rezzes.
pub const SPARK: &str = "Spark";

/// The name of the notecard the notecard reader reads.
pub const NOTECARD: &str = "Greetings";

/// The name of the animation in the pose ball's contents.
pub const POSE: &str = "Pose";

/// The hidden channel the listener takes its commands on.
pub const LISTENER_CHANNEL: i32 = 42;

/// The asset of [`POSE`]: the torso twisting and back, looping
/// ([`sl_test_assets::anim::chest_twist_animation_asset`]).
pub const POSE_ANIMATION: uuid::Uuid = uuid::Uuid::from_u128(0x5C_0001);

/// The sensor's script.
pub const SENSOR_SOURCE: &str = include_str!("scripts_scene/sensor.lsl");

/// The touch prim's script.
pub const TOUCH_SOURCE: &str = include_str!("scripts_scene/touch.lsl");

/// The dialog prim's script.
pub const DIALOG_SOURCE: &str = include_str!("scripts_scene/dialog.lsl");

/// The listener's script.
pub const LISTENER_SOURCE: &str = include_str!("scripts_scene/listen.lsl");

/// The vendor's script.
pub const VENDOR_SOURCE: &str = include_str!("scripts_scene/vendor.lsl");

/// The pose ball's script.
pub const POSE_BALL_SOURCE: &str = include_str!("scripts_scene/pose_ball.lsl");

/// The rezzer's script.
pub const REZZER_SOURCE: &str = include_str!("scripts_scene/rezzer.lsl");

/// The script of the [`SPARK`] the rezzer rezzes.
pub const SPARK_SOURCE: &str = include_str!("scripts_scene/spark.lsl");

/// The notecard reader's script.
pub const NOTECARD_READER_SOURCE: &str = include_str!("scripts_scene/notecard_reader.lsl");

/// The text of the notecard the notecard reader reads.
pub const NOTECARD_TEXT: &str = include_str!("scripts_scene/greetings.txt");

/// The timer prim's script.
pub const TIMER_SOURCE: &str = include_str!("scripts_scene/timer.lsl");

/// The physical box's script.
pub const PHYSICAL_SOURCE: &str = include_str!("scripts_scene/physical.lsl");

/// The linkset root's script.
pub const LINKSET_ROOT_SOURCE: &str = include_str!("scripts_scene/linkset_root.lsl");

/// The script in each of the linkset's children.
pub const LINKSET_CHILD_SOURCE: &str = include_str!("scripts_scene/linkset_child.lsl");

/// The broken script prim's script, which does not compile.
pub const BROKEN_SOURCE: &str = include_str!("scripts_scene/broken.lsl");

/// Who owns the scene's prims: no account, so a payment moves nobody's
/// balance but the payer's.
const PROPS_OWNER: u128 = 0x5C_0000;

/// How far north of the region's centre the row stands, in metres: in front
/// of an avatar arriving at the centre, and far enough that one walking along
/// the row a little south of it passes clear of its prims.
const ROW_NORTH_M: f32 = 4.0;

/// How high above the ground the physical box starts, in metres, before its
/// script makes it physical and it falls on to its plinth.
const PHYSICAL_DROP_M: f32 = 3.0;

/// How thick the plinth is, in metres.
const PLINTH_HEIGHT_M: f32 = 0.3;

/// One prim of the row: its region-local id (its keys are minted from it),
/// its name, where along the row it stands, its edge, its script and what
/// clicking it does before its script first runs.
struct Prop {
    /// Its region-local id.
    local_id: u32,
    /// Its name.
    name: &'static str,
    /// Its position's x, in region metres.
    x: f32,
    /// Its cube's edge, in metres.
    edge: f32,
    /// Its script's name in its contents.
    script: &'static str,
    /// Its script's source.
    source: &'static str,
    /// What clicking it does.
    click: ClickAction,
}

/// The row, west to east. The physical box stands over its plinth, and the
/// linkset root carries two children; both are dressed further in [`region`].
const PROPS: [Prop; 12] = [
    Prop {
        local_id: 0x5C1,
        name: SENSOR,
        x: 114.0,
        edge: 0.5,
        script: "sensor.lsl",
        source: SENSOR_SOURCE,
        click: ClickAction::Touch,
    },
    Prop {
        local_id: 0x5C2,
        name: TOUCH,
        x: 120.0,
        edge: 0.5,
        script: "touch.lsl",
        source: TOUCH_SOURCE,
        click: ClickAction::Touch,
    },
    Prop {
        local_id: 0x5C3,
        name: DIALOG,
        x: 122.0,
        edge: 0.5,
        script: "dialog.lsl",
        source: DIALOG_SOURCE,
        click: ClickAction::Touch,
    },
    Prop {
        local_id: 0x5C4,
        name: LISTENER,
        x: 124.0,
        edge: 0.5,
        script: "listen.lsl",
        source: LISTENER_SOURCE,
        click: ClickAction::Touch,
    },
    Prop {
        local_id: 0x5C5,
        name: VENDOR,
        x: 126.0,
        edge: 0.6,
        script: "vendor.lsl",
        source: VENDOR_SOURCE,
        click: ClickAction::Pay,
    },
    Prop {
        local_id: 0x5C6,
        name: POSE_BALL,
        x: 128.0,
        edge: 0.3,
        script: "pose_ball.lsl",
        source: POSE_BALL_SOURCE,
        click: ClickAction::Sit,
    },
    Prop {
        local_id: 0x5C7,
        name: REZZER,
        x: 130.0,
        edge: 0.5,
        script: "rezzer.lsl",
        source: REZZER_SOURCE,
        click: ClickAction::Touch,
    },
    Prop {
        local_id: 0x5C8,
        name: NOTECARD_READER,
        x: 132.0,
        edge: 0.5,
        script: "notecard_reader.lsl",
        source: NOTECARD_READER_SOURCE,
        click: ClickAction::Touch,
    },
    Prop {
        local_id: 0x5C9,
        name: TIMER,
        x: 134.0,
        edge: 0.5,
        script: "timer.lsl",
        source: TIMER_SOURCE,
        click: ClickAction::Touch,
    },
    Prop {
        local_id: 0x5CA,
        name: PHYSICAL,
        x: 136.0,
        edge: 0.4,
        script: "physical.lsl",
        source: PHYSICAL_SOURCE,
        click: ClickAction::Touch,
    },
    Prop {
        local_id: 0x5CB,
        name: LINKSET,
        x: 138.0,
        edge: 0.5,
        script: "linkset_root.lsl",
        source: LINKSET_ROOT_SOURCE,
        click: ClickAction::Touch,
    },
    Prop {
        local_id: 0x5CC,
        name: BROKEN,
        x: 140.0,
        edge: 0.5,
        script: "broken.lsl",
        source: BROKEN_SOURCE,
        click: ClickAction::Touch,
    },
];

/// The plinth's region-local id.
const PLINTH_LOCAL_ID: u32 = 0x5CD;

/// The linkset children's region-local ids, link numbers 2 and 3.
const LINKSET_CHILD_LOCAL_IDS: [u32; 2] = [0x5CE, 0x5CF];

/// The region-local id the spark's record is written under before it is
/// held; a rez mints it a fresh one.
const SPARK_LOCAL_ID: u32 = 0x5D0;

/// The id a prim's key of kind `kind` is minted from.
fn minted(local_id: u32, kind: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128((u128::from(local_id) << 8) | kind)
}

/// The prop called `name`.
fn prop(name: &str) -> Option<&'static Prop> {
    PROPS.iter().find(|prop| prop.name == name)
}

/// The ground's height in the scene, in metres.
fn ground() -> f32 {
    f32::from(crate::scenario::STOCK_TERRAIN_HEIGHT_M)
}

/// The y of the row, in region metres.
const fn row_y() -> f32 {
    128.0 + ROW_NORTH_M
}

/// The key of the scene's prim called `name` — a prop of the row, the
/// [`PLINTH`] or one of the [`LINKSET_CHILDREN`] — or `None` for any other
/// name.
#[must_use]
pub fn key(name: &str) -> Option<ObjectKey> {
    let local_id = prop(name).map(|prop| prop.local_id).or_else(|| {
        if name == PLINTH {
            return Some(PLINTH_LOCAL_ID);
        }
        LINKSET_CHILDREN
            .iter()
            .position(|child| *child == name)
            .and_then(|index| LINKSET_CHILD_LOCAL_IDS.get(index).copied())
    })?;
    Some(ObjectKey::from(minted(local_id, 0x5)))
}

/// The script item in the prop called `name`, or `None` for a name not in
/// the row.
#[must_use]
pub fn script_item_key(name: &str) -> Option<InventoryKey> {
    prop(name).map(|prop| InventoryKey::from(minted(prop.local_id, 0x7)))
}

/// Where the prop called `name` stands as the scene starts — the physical box
/// over its plinth, before it falls — or `None` for a name not in the row.
#[must_use]
pub fn position(name: &str) -> Option<Vector> {
    let prop = prop(name)?;
    let lift = if name == PHYSICAL {
        PLINTH_HEIGHT_M + PHYSICAL_DROP_M
    } else {
        0.0
    };
    Some(Vector {
        x: prop.x,
        y: row_y(),
        z: ground() + lift + prop.edge / 2.0,
    })
}

/// Where the plinth stands.
#[must_use]
pub fn plinth_position() -> Vector {
    Vector {
        x: prop(PHYSICAL).map_or(136.0, |prop| prop.x),
        y: row_y(),
        z: ground() + PLINTH_HEIGHT_M / 2.0,
    }
}

/// The scene: the row on the stock ground, every script running from the
/// start (the broken one has nothing to run).
#[must_use]
pub fn region() -> RegionFixture {
    let owner = AgentKey::from(uuid::Uuid::from_u128(PROPS_OWNER));
    let mut fixture = RegionFixture::new();
    for prop in &PROPS {
        let at = position(prop.name).unwrap_or_else(|| Vector {
            x: prop.x,
            y: row_y(),
            z: ground(),
        });
        let mut object = named_box(prop.local_id, prop.name, owner, at, cube(prop.edge));
        object.click_action = prop.click.to_code();
        let mut script = script_item(prop.local_id, 0x7, prop.script, owner);
        if prop.name == BROKEN {
            if let Some(properties) = object.properties.as_mut() {
                properties.permissions.everyone |= sl_proto::Permissions::MODIFY;
            }
            script.permissions.everyone |= sl_proto::Permissions::MODIFY;
        }
        fixture.world.objects.push(object);
        stock(&mut fixture, prop.local_id, vec![(script, prop.source)]);
    }
    add_plinth(&mut fixture, owner);
    add_linkset_children(&mut fixture, owner);
    give_the_pose(&mut fixture, owner);
    give_the_notecard(&mut fixture, owner);
    give_the_spark(&mut fixture, owner);
    fixture
}

/// An edge-long cube's scale.
const fn cube(edge: f32) -> Vector {
    Vector {
        x: edge,
        y: edge,
        z: edge,
    }
}

/// A box prim of the scene, keyed from `local_id`, named `name`.
fn named_box(
    local_id: u32,
    name: &str,
    owner: AgentKey,
    at: Vector,
    scale: Vector,
) -> sl_proto::Object {
    let mut object = crate::world::box_prim(
        RegionLocalObjectId(local_id),
        ObjectKey::from(minted(local_id, 0x5)),
        owner,
        at,
        scale,
    );
    let mut properties = crate::world::default_object_properties(&object);
    name.clone_into(&mut properties.name);
    object.properties = Some(properties);
    object
}

/// A script item called `name` in the prim `local_id`, its item and asset
/// keys minted from the prim's with `kind` and the kind after it.
fn script_item(
    local_id: u32,
    kind: u128,
    name: &str,
    owner: AgentKey,
) -> sl_proto::TaskInventoryItem {
    sl_proto::TaskInventoryItem {
        item_id: InventoryKey::from(minted(local_id, kind)),
        parent_task: ObjectKey::from(minted(local_id, 0x5)),
        asset_id: Some(AssetKey::from(minted(local_id, kind.saturating_add(1)))),
        name: name.to_owned(),
        owner: OwnerKey::Agent(owner),
        creator_id: owner,
        last_owner_id: owner,
        ..crate::scenario::stock_script_item()
    }
}

/// The contents of a prim holding `scripts`, each running, its source the
/// bytes of its asset.
fn running_contents(
    fixture: &mut RegionFixture,
    scripts: Vec<(sl_proto::TaskInventoryItem, &str)>,
) -> crate::world::TaskInventory {
    let mut items = Vec::new();
    let mut running = Vec::new();
    for (item, source) in scripts {
        if let Some(asset) = item.asset_id {
            let _previous = fixture.assets.insert(asset, source.as_bytes().to_vec());
        }
        running.push(item.item_id);
        items.push(item);
    }
    let mut contents = crate::world::TaskInventory::stated(1, items);
    contents.running.extend(running);
    contents
}

/// Puts `scripts` in the prim `local_id`, each running.
fn stock(
    fixture: &mut RegionFixture,
    local_id: u32,
    scripts: Vec<(sl_proto::TaskInventoryItem, &str)>,
) {
    let contents = running_contents(fixture, scripts);
    if fixture
        .world
        .add_task_inventory(RegionLocalObjectId(local_id), contents)
        .is_err()
    {
        tracing::error!(local_id, "a scripts-scene prim is not in its region");
    }
}

/// Adds an item to the contents of the prop called `name`, and its bytes, if
/// it has any, to the scene's assets.
fn add_to(
    fixture: &mut RegionFixture,
    name: &str,
    item: sl_proto::TaskInventoryItem,
    bytes: Option<Vec<u8>>,
) {
    if let Some((asset, bytes)) = item.asset_id.zip(bytes) {
        let _previous = fixture.assets.insert(asset, bytes);
    }
    let added = prop(name).and_then(|prop| {
        fixture
            .world
            .objects
            .contents_mut(RegionLocalObjectId(prop.local_id))
            .map(|contents| contents.items.push(item))
    });
    if added.is_none() {
        tracing::error!(name, "a scripts-scene prop is not in its region");
    }
}

/// The unscripted slab under the physical box.
fn add_plinth(fixture: &mut RegionFixture, owner: AgentKey) {
    let plinth = named_box(
        PLINTH_LOCAL_ID,
        PLINTH,
        owner,
        plinth_position(),
        Vector {
            x: 1.0,
            y: 1.0,
            z: PLINTH_HEIGHT_M,
        },
    );
    fixture.world.objects.push(plinth);
}

/// The linkset root's two children, stacked on it, each with the child
/// script.
fn add_linkset_children(fixture: &mut RegionFixture, owner: AgentKey) {
    let Some(root) = prop(LINKSET) else {
        return;
    };
    let edge = 0.4;
    let mut offset = root.edge / 2.0 + edge / 2.0;
    for (local_id, name) in LINKSET_CHILD_LOCAL_IDS.iter().zip(LINKSET_CHILDREN) {
        let mut child = named_box(
            *local_id,
            name,
            owner,
            Vector {
                x: 0.0,
                y: 0.0,
                z: offset,
            },
            cube(edge),
        );
        child.parent_id = RegionLocalObjectId(root.local_id);
        fixture.world.objects.push(child);
        let script = script_item(*local_id, 0x7, "linkset_child.lsl", owner);
        stock(fixture, *local_id, vec![(script, LINKSET_CHILD_SOURCE)]);
        offset += edge;
    }
}

/// Puts [`POSE`] in the pose ball's contents, and its bytes in the scene's
/// assets.
fn give_the_pose(fixture: &mut RegionFixture, owner: AgentKey) {
    let Some(ball) = prop(POSE_BALL) else {
        return;
    };
    let item = sl_proto::TaskInventoryItem {
        asset_id: Some(AssetKey::from(POSE_ANIMATION)),
        asset_type: AssetType::Animation,
        inv_type: InventoryType::Animation,
        ..script_item(ball.local_id, 0x9, POSE, owner)
    };
    add_to(
        fixture,
        POSE_BALL,
        item,
        Some(sl_test_assets::anim::chest_twist_animation_asset()),
    );
}

/// Puts [`NOTECARD`] in the notecard reader's contents.
fn give_the_notecard(fixture: &mut RegionFixture, owner: AgentKey) {
    let Some(reader) = prop(NOTECARD_READER) else {
        return;
    };
    let item = sl_proto::TaskInventoryItem {
        asset_type: AssetType::Notecard,
        inv_type: InventoryType::Notecard,
        ..script_item(reader.local_id, 0x9, NOTECARD, owner)
    };
    let body = sl_notecard::Notecard {
        source_version: sl_notecard::NotecardVersion::V2,
        items: Vec::new(),
        // The file ends in a newline, as a text file does; the notecard does
        // not, or a reader would find an empty last line.
        text: NOTECARD_TEXT.trim_end_matches('\n').to_owned(),
    }
    .encode();
    add_to(fixture, NOTECARD_READER, item, Some(body));
}

/// Puts [`SPARK`] in the rezzer's contents, and the scripted prim it stands
/// for among the scene's held objects.
fn give_the_spark(fixture: &mut RegionFixture, owner: AgentKey) {
    let Some(rezzer) = prop(REZZER) else {
        return;
    };
    let item = sl_proto::TaskInventoryItem {
        asset_type: AssetType::Object,
        inv_type: InventoryType::Object,
        ..script_item(rezzer.local_id, 0x9, SPARK, owner)
    };
    let held_item = item.item_id;
    add_to(fixture, REZZER, item, None);
    let spark = named_box(
        SPARK_LOCAL_ID,
        SPARK,
        owner,
        Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        cube(0.2),
    );
    let script = script_item(SPARK_LOCAL_ID, 0x7, "spark.lsl", owner);
    let contents = running_contents(fixture, vec![(script, SPARK_SOURCE)]);
    fixture.held_objects.push(HeldObject {
        item: held_item,
        prims: vec![(spark, Some(contents))],
    });
}

/// The avatar the named scene `scripts-avatars` brings: [`visitor_on`] a grid
/// imitating Second Life, the grid a fake grid imitates by default.
#[must_use]
pub fn visitor() -> ScriptedAvatar {
    visitor_on(crate::ImitatedGrid::SecondLife)
}

/// The scene's visitor on a grid imitating `grid`: dressed and granting
/// whatever it is asked, it walks past the sensor and back, then works along
/// the row — touches the touch prim, picks Green from the dialog, greets the
/// listener and tells it to spin, pays the vendor, has the rezzer rez a spark,
/// has the notecard read, drops the physical box, pings the linkset — and ends
/// seated on the pose ball, posing. Each step waits for the line its script
/// answers with, so the avatar never runs ahead of the scene.
///
/// On a grid that keeps no balances (OpenSim's stock money module) it does not
/// pay: that grid drops a payment to an object without a word and raises no
/// `money` event, so there is nothing to wait for.
#[must_use]
pub fn visitor_on(grid: crate::ImitatedGrid) -> ScriptedAvatar {
    let named = |name: &str| Target::Named(name.to_owned());
    let heard = |line: &str| AvatarAction::AwaitChat(line.to_owned());
    let sensor_x = prop(SENSOR).map_or(114.0, |sensor| sensor.x);
    let mut avatar = ScriptedAvatar::new("Scripted", "Visitor")
        .dressed()
        .answering(AnswerPolicy::agreeable())
        .after_ticks(
            10,
            AvatarAction::Walk {
                to: Vector {
                    x: sensor_x + 2.0,
                    y: 130.0,
                    z: ground(),
                },
            },
        )
        .after_ticks(0, heard("came near"))
        .after_ticks(
            1,
            AvatarAction::Walk {
                to: Vector {
                    x: 128.0,
                    y: 130.0,
                    z: ground(),
                },
            },
        )
        .after_ticks(0, heard("went away"))
        .after_ticks(1, AvatarAction::Touch(named(TOUCH)))
        .after_ticks(0, heard("touched face"))
        .after_ticks(1, AvatarAction::Touch(named(DIALOG)))
        .after_ticks(
            0,
            AvatarAction::AnswerDialog(DialogChoice::Label("Green".to_owned())),
        )
        .after_ticks(0, heard("chose Green"))
        .after_ticks(
            1,
            AvatarAction::Say {
                channel: 0,
                message: "hello".to_owned(),
            },
        )
        .after_ticks(0, heard("Hello, Scripted Visitor"))
        .after_ticks(
            1,
            AvatarAction::Say {
                channel: LISTENER_CHANNEL,
                message: "spin".to_owned(),
            },
        )
        .after_ticks(0, heard("Spinning for"));
    if grid.upload_funds().is_some() {
        avatar = avatar
            .after_ticks(
                1,
                AvatarAction::Pay {
                    target: named(VENDOR),
                    amount: None,
                },
            )
            .after_ticks(0, heard("Thank you"));
    }
    avatar
        .after_ticks(1, AvatarAction::Touch(named(REZZER)))
        .after_ticks(0, heard("Spark 1 lit"))
        .after_ticks(1, AvatarAction::Touch(named(NOTECARD_READER)))
        .after_ticks(0, heard("End of Greetings"))
        .after_ticks(1, AvatarAction::Touch(named(PHYSICAL)))
        .after_ticks(0, heard("Bumped Plinth"))
        .after_ticks(1, AvatarAction::Touch(named(LINKSET)))
        .after_ticks(0, heard("Link 3 answered"))
        .after_ticks(1, AvatarAction::Sit(named(POSE_BALL)))
        .after_ticks(0, heard("Posing"))
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// Every prop stands in the scene under its name and key, clicked as
    /// stated, with its one script running; the plinth and the linkset's
    /// children are there too, the children linked to the root.
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
                    .map(|object| ClickAction::from_code(object.click_action)),
                Some(prop.click),
                "{}",
                prop.name
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
        for child in LINKSET_CHILDREN {
            let object = key(child).and_then(|key| {
                fixture
                    .world
                    .objects
                    .iter()
                    .find(|object| object.full_id == key)
                    .cloned()
            });
            assert_eq!(
                object.map(|object| object.parent_id),
                prop(LINKSET).map(|root| RegionLocalObjectId(root.local_id)),
                "{child}"
            );
        }
        assert!(
            fixture
                .world
                .objects
                .get(RegionLocalObjectId(PLINTH_LOCAL_ID))
                .is_some(),
            "the plinth is not in the scene"
        );
    }

    /// Every script compiles but the broken one, as both grids compile it.
    #[test]
    fn every_script_compiles_but_the_broken_one() {
        let sources = PROPS
            .iter()
            .map(|prop| (prop.name, prop.source))
            .chain([("spark", SPARK_SOURCE), ("child", LINKSET_CHILD_SOURCE)]);
        for grid in [
            crate::ImitatedGrid::SecondLife,
            crate::ImitatedGrid::OpenSim,
        ] {
            let compiler = grid.script_compile_policy().compiler;
            for (name, source) in sources.clone() {
                let outcome = crate::compile_source(
                    compiler,
                    sl_proto::ScriptTarget::Mono.to_wire(),
                    source.as_bytes(),
                    AssetKey::from(uuid::Uuid::nil()),
                );
                assert_eq!(
                    outcome.compiled,
                    name != BROKEN,
                    "{name} on {grid:?}: {:?}",
                    outcome.errors
                );
            }
        }
    }

    /// The rezzer holds the spark, the notecard reader its notecard and the
    /// pose ball its pose, each with what stands behind it: the spark a held
    /// scripted prim, the other two an asset.
    #[test]
    fn the_props_hold_what_their_scripts_use() {
        let fixture = region();
        let holds = |name: &str, item: &str| {
            prop(name)
                .and_then(|prop| {
                    fixture
                        .world
                        .objects
                        .contents(RegionLocalObjectId(prop.local_id))
                })
                .and_then(|contents| {
                    contents
                        .items
                        .iter()
                        .find(|held| held.name == item)
                        .cloned()
                })
        };
        let spark = holds(REZZER, SPARK);
        assert_eq!(
            fixture.held_objects.first().map(|held| held.item),
            spark.map(|spark| spark.item_id)
        );
        for (name, item) in [(NOTECARD_READER, NOTECARD), (POSE_BALL, POSE)] {
            let asset = holds(name, item).and_then(|held| held.asset_id);
            assert!(
                asset.is_some_and(|asset| fixture.assets.contains(asset)),
                "{name}'s {item} has nothing behind it"
            );
        }
    }
}
