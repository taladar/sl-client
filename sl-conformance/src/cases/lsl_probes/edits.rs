//! The drives that change a probe's prims from a viewer, as a resident would
//! with the build floater and the contents tab: an edit (`drive=edit`), a
//! change to a prim's contents (`drive=contents`), and a sale of prim A to the
//! other avatar (`drive=buy`).
//!
//! Each is sent as the reference viewer sends it, and noted; what the grid
//! made of it is the probe's to say. The face edits go as an `ObjectImage` of
//! every face carrying the faces the prim has now with one thing changed, so
//! that an edit changes that one thing and nothing else.

use std::collections::HashSet;
use std::time::Duration;

use sl_client_tokio::{
    AssetType, AssetUpdateLocation, Command, Event, GroupKey, InventoryFolderKey, InventoryItem,
    InventoryType, LightData, LindenAmount, MediaEntry, NewInventoryItem, Object, ObjectBuyItem,
    ObjectFlagSettings, ObjectKey, ObjectTransform, Permissions, RestoreItem, Rotation, SaleType,
    ScriptTarget, ScriptUploadLocation, TaskInventoryItem, TaskInventoryKey, TextureFace,
    TextureKey, UpdatableAssetType, Uuid, Vector, WearableType, decode_texture_entry, prim_flags,
};
use sl_lsl_runtime::probe::{By, ContentsOp, Edit};

use super::{
    Avatars, COMPILE_TIMEOUT, STEP_TIMEOUT, Scene, Transcript, Until, Watch, after, collect,
    drain_other, in_step, listing, new_script, quote, scoped_for,
};
use crate::cases::task_inventory::{drop_item, notecard_bytes};
use crate::context::{Session, TestFailure};
use crate::probe_corpus::Probe;

/// The faces of the cube every probe prim is rezzed as.
const CUBE_FACES: usize = 6;

/// How long a sale waits between the owner putting prim A up for sale and the
/// other avatar buying it, so that the grid has the sale before the purchase.
const SALE_SETTLE: Duration = Duration::from_secs(2);

/// How long a drop or a purchase by the other avatar keeps the probe's lines
/// before the other avatar's own events are read: what the grid answers the
/// other avatar comes within it.
const OTHER_SETTLE: Duration = Duration::from_secs(3);

/// The body a `save-script` saves: a script that does nothing.
const IDLE_SCRIPT: &str = "default\n{\n    state_entry()\n    {\n    }\n}\n";

/// The text of a notecard a `contents` drop makes.
const DROPPED_TEXT: &str = "dropped by the probe runner";

/// The prim the probe labelled `label`, as the region last sent it.
fn labelled(
    scene: &Scene<'_>,
    probe: &Probe,
    transcript: &Transcript,
    label: &str,
) -> Result<Object, TestFailure> {
    let prim = scene
        .prims
        .get(label)
        .ok_or_else(|| TestFailure::State(format!("{}: no prim {label}", probe.id)))?;
    Ok(transcript.latest.get(&prim.full_id).unwrap_or(prim).clone())
}

/// An edit as a note says it.
fn edit_note(edit: &Edit) -> String {
    let triple = |[x, y, z]: [f32; 3]| format!("<{x}, {y}, {z}>");
    match edit {
        Edit::Color([red, green, blue]) => format!("what=color value={red},{green},{blue}"),
        Edit::Alpha(alpha) => format!("what=alpha value={alpha}"),
        Edit::Texture(id) => format!("what=texture value={id}"),
        Edit::Repeats(repeats) => format!("what=repeats value={repeats}"),
        Edit::Glow(glow) => format!("what=glow value={glow}"),
        Edit::Fullbright(on) => format!("what=fullbright value={}", u8::from(*on)),
        Edit::Scale(size) => format!("what=scale value={}", triple(*size)),
        Edit::Move(offset) => format!("what=move value={}", triple(*offset)),
        Edit::Turn(degrees) => format!("what=turn value={degrees}"),
        Edit::Cut(percent) => format!("what=cut value={percent}"),
        Edit::Light(on) => format!("what=light value={}", u8::from(*on)),
        Edit::Name(name) => format!("what=name value={}", quote(name)),
        Edit::Description(text) => format!("what=description value={}", quote(text)),
        Edit::Phantom(on) => format!("what=phantom value={}", u8::from(*on)),
        Edit::Media(url) => format!("what=media value={}", quote(url)),
    }
}

/// Sends `prim` again with every face changed by `change`, as the build
/// floater's texture tab does: an `ObjectImage` of the faces it has now, the
/// legacy media URL kept.
async fn retexture(
    session: &Session,
    prim: &Object,
    change: impl Fn(&mut TextureFace),
) -> Result<(), TestFailure> {
    let mut entry = decode_texture_entry(&prim.texture_entry, CUBE_FACES);
    for face in &mut entry.faces {
        change(face);
    }
    session
        .send(Command::SetObjectImage {
            local_id: scoped_for(session, prim),
            media_url: prim.media_url.as_ref().map(ToString::to_string),
            texture_entry: entry,
        })
        .await
}

/// Changes the prim the probe labelled `label` as the build floater does, as
/// the owner, and notes it.
pub(super) async fn edit_prim(
    session: &Session,
    probe: &Probe,
    scene: &Scene<'_>,
    transcript: &mut Transcript,
    (edit, label): (&Edit, &str),
) -> Result<(), TestFailure> {
    let prim = labelled(scene, probe, transcript, label)?;
    transcript
        .body
        .push(format!("# edit prim={label} {}", edit_note(edit)));
    let local_id = scoped_for(session, &prim);
    let transform = |transform: ObjectTransform| Command::UpdateObject {
        local_id,
        transform,
    };
    let command = match edit {
        Edit::Color([red, green, blue]) => {
            let rgb = [*red, *green, *blue];
            return retexture(session, &prim, |face| {
                face.color = [rgb[0], rgb[1], rgb[2], face.color[3]];
            })
            .await;
        }
        Edit::Alpha(alpha) => {
            return retexture(session, &prim, |face| face.color[3] = *alpha).await;
        }
        Edit::Texture(id) => {
            let texture = TextureKey::from(Uuid::parse_str(id).map_err(|error| {
                TestFailure::Assertion(format!("{}: texture {id}: {error}", probe.id))
            })?);
            return retexture(session, &prim, |face| face.texture_id = texture).await;
        }
        Edit::Repeats(repeats) => {
            return retexture(session, &prim, |face| {
                face.scale_s = *repeats;
                face.scale_t = *repeats;
            })
            .await;
        }
        Edit::Glow(glow) => return retexture(session, &prim, |face| face.glow = *glow).await,
        Edit::Fullbright(on) => {
            /// The full-bright bit of a face's bump/shiny/full-bright byte.
            const FULLBRIGHT: u8 = 0x20;
            return retexture(session, &prim, |face| {
                if *on {
                    face.bump_shiny_fullbright |= FULLBRIGHT;
                } else {
                    face.bump_shiny_fullbright &= !FULLBRIGHT;
                }
            })
            .await;
        }
        Edit::Scale([x, y, z]) => transform(ObjectTransform {
            scale: Some(Vector {
                x: *x,
                y: *y,
                z: *z,
            }),
            ..ObjectTransform::default()
        }),
        Edit::Move([x, y, z]) => {
            let at = &prim.motion.position;
            transform(ObjectTransform {
                position: Some(Vector {
                    x: at.x + x,
                    y: at.y + y,
                    z: at.z + z,
                }),
                ..ObjectTransform::default()
            })
        }
        Edit::Turn(degrees) => {
            let half = degrees.to_radians() / 2.0;
            transform(ObjectTransform {
                rotation: Some(Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: half.sin(),
                    s: half.cos(),
                }),
                ..ObjectTransform::default()
            })
        }
        Edit::Cut(percent) => {
            let mut shape = prim.shape;
            // The path cut's start, quantized as the wire carries it: a
            // fiftieth of a thousandth.
            shape.path_begin = u16::from(*percent).saturating_mul(500);
            Command::SetObjectShape { local_id, shape }
        }
        Edit::Light(on) => {
            let mut params = prim.extra.clone();
            params.light = on.then_some(LightData {
                color: [255, 255, 255, 255],
                radius: 10.0,
                cutoff: 0.0,
                falloff: 0.75,
            });
            Command::SetObjectExtraParams { local_id, params }
        }
        Edit::Name(name) => Command::SetObjectName {
            local_id,
            name: name.clone(),
        },
        Edit::Description(description) => Command::SetObjectDescription {
            local_id,
            description: description.clone(),
        },
        Edit::Phantom(on) => Command::SetObjectFlags {
            local_id,
            flags: ObjectFlagSettings {
                use_physics: prim.update_flags & prim_flags::USE_PHYSICS != 0,
                is_temporary: prim.update_flags & prim_flags::TEMPORARY_ON_REZ != 0,
                is_phantom: *on,
                casts_shadows: false,
            },
        },
        Edit::Media(url) => {
            let faces = if url.is_empty() {
                vec![None; CUBE_FACES]
            } else {
                let parsed = url::Url::parse(url).map_err(|error| {
                    TestFailure::Assertion(format!("{}: media URL {url}: {error}", probe.id))
                })?;
                let mut faces = vec![None; CUBE_FACES];
                if let Some(first) = faces.first_mut() {
                    *first = Some(MediaEntry {
                        current_url: Some(parsed.clone()),
                        home_url: Some(parsed),
                        ..MediaEntry::default()
                    });
                }
                faces
            };
            Command::SetObjectMedia {
                object_id: prim.full_id,
                faces,
            }
        }
    };
    session.send(command).await
}

/// The item of `prim`'s contents called `name`, if its listing holds one.
async fn named_item(
    session: &mut Session,
    prim: &Object,
    name: &str,
    watch: &mut Watch<'_>,
) -> Result<Option<TaskInventoryItem>, TestFailure> {
    session
        .send(Command::FetchTaskInventory {
            target: scoped_for(session, prim),
            held_serial: None,
        })
        .await?;
    let items = listing(session, prim, watch).await?;
    Ok(items.into_iter().find(|item| item.name == name))
}

/// Makes a notecard called `name` in the folder `folder` of `session`'s
/// inventory and saves `text` into it, as a viewer does, handing every other
/// event to `keep`; returns the item as it stands once saved.
pub(super) async fn saved_notecard(
    session: &mut Session,
    (name, text): (&str, &str),
    folder: InventoryFolderKey,
    mut keep: impl FnMut(&Event),
) -> Result<InventoryItem, TestFailure> {
    session
        .send(Command::CreateInventoryItem(NewInventoryItem {
            folder_id: folder,
            transaction_id: Uuid::nil(),
            next_owner_mask: Permissions::ALL.bits(),
            asset_type: AssetType::Notecard,
            inv_type: InventoryType::Notecard,
            wearable_type: WearableType::Shape,
            name: name.to_owned(),
            description: "sl-conformance lsl-probes".to_owned(),
        }))
        .await?;
    let mut item = session
        .wait_for(STEP_TIMEOUT, |event| match event {
            // By its type, not its name: OpenSim keeps a name that is not
            // ASCII as a hex dump of its bytes, and the item is listed so.
            Event::InventoryItemCreated { item, .. }
                if i32::from(item.inv_type) == InventoryType::Notecard.to_code() =>
            {
                Some(item.clone())
            }
            other => {
                keep(other);
                None
            }
        })
        .await
        .map_err(|failure| in_step(failure, &format!("creating notecard {name:?}")))?;
    session
        .send(Command::UpdateInventoryAsset {
            location: AssetUpdateLocation::AgentInventory {
                item_id: item.item_id,
            },
            asset_type: UpdatableAssetType::Notecard,
            data: notecard_bytes(text),
        })
        .await?;
    let saved = session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::AssetUploaded { new_asset, .. } => Some(Ok(*new_asset)),
            Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
            other => {
                keep(other);
                None
            }
        })
        .await
        .map_err(|failure| in_step(failure, &format!("saving notecard {name:?}")))?
        .map_err(|reason| {
            TestFailure::Assertion(format!("saving notecard {name:?} failed: {reason}"))
        })?;
    // The drop's checksum covers the asset the item holds now.
    item.asset_id = saved;
    Ok(item)
}

/// Changes the contents of the prim the probe labelled `label` as a viewer
/// does — the owner, or for a drop the avatar `by` names — and notes it.
pub(super) async fn change_contents(
    avatars: &mut Avatars<'_>,
    by: By,
    probe: &Probe,
    scene: &Scene<'_>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    (op, label): (&ContentsOp, &str),
) -> Result<(), TestFailure> {
    let prim = labelled(scene, probe, transcript, label)?;
    if let (By::Other, ContentsOp::Drop { name }) = (by, op) {
        return drop_by_other(avatars, probe, keys, transcript, (&prim, label), name).await;
    }
    let session = &mut *avatars.session;
    let mut watch = Watch {
        id: &probe.id,
        prims: keys,
        transcript,
    };
    match op {
        ContentsOp::Drop { name } => {
            watch.transcript.body.push(format!(
                "# contents prim={label} op=drop name={}",
                quote(name)
            ));
            let item = saved_notecard(session, (name, DROPPED_TEXT), scene.trash, |event| {
                let _kept = watch.transcript.take(watch.id, watch.prims, event);
            })
            .await?;
            session
                .send(Command::UpdateTaskInventory {
                    target: scoped_for(session, &prim),
                    key: TaskInventoryKey::Item,
                    item: Box::new(drop_item(&item, prim.full_id)?),
                })
                .await
        }
        ContentsOp::NewScript { name } => {
            watch.transcript.body.push(format!(
                "# contents prim={label} op=new-script name={}",
                quote(name)
            ));
            let _item = new_script(session, &prim, name, &mut watch).await?;
            Ok(())
        }
        ContentsOp::Rename { name, to } => {
            let Some(item) = named_item(session, &prim, name, &mut watch).await? else {
                watch.transcript.body.push(format!(
                    "# contents prim={label} op=rename name={} found=0",
                    quote(name)
                ));
                return Ok(());
            };
            watch.transcript.body.push(format!(
                "# contents prim={label} op=rename name={} to={}",
                quote(name),
                quote(to)
            ));
            let renamed = RestoreItem::from_task_item(&item, Some(to), None, Uuid::new_v4())
                .map_err(|error| {
                    TestFailure::Assertion(format!("the item does not encode: {error}"))
                })?;
            session
                .send(Command::UpdateTaskInventory {
                    target: scoped_for(session, &prim),
                    key: TaskInventoryKey::Item,
                    item: Box::new(renamed),
                })
                .await
        }
        ContentsOp::Remove { name } => {
            let Some(item) = named_item(session, &prim, name, &mut watch).await? else {
                watch.transcript.body.push(format!(
                    "# contents prim={label} op=remove name={} found=0",
                    quote(name)
                ));
                return Ok(());
            };
            watch.transcript.body.push(format!(
                "# contents prim={label} op=remove name={}",
                quote(name)
            ));
            session
                .send(Command::RemoveTaskInventory {
                    target: scoped_for(session, &prim),
                    item_id: item.item_id,
                })
                .await
        }
        ContentsOp::SaveScript { name } => {
            let Some(item) = named_item(session, &prim, name, &mut watch).await? else {
                watch.transcript.body.push(format!(
                    "# contents prim={label} op=save-script name={} found=0",
                    quote(name)
                ));
                return Ok(());
            };
            session
                .send(Command::UploadScript {
                    location: ScriptUploadLocation::TaskInventory {
                        task_id: prim.full_id,
                        item_id: item.item_id,
                        running: false,
                        experience: None,
                    },
                    target: ScriptTarget::Mono,
                    source: format!("{}{IDLE_SCRIPT}", scene.engine.source_prefix()).into_bytes(),
                })
                .await?;
            let compiled = session
                .wait_for(COMPILE_TIMEOUT, |event| match event {
                    Event::ScriptUploaded { compiled, .. } => Some(Some(*compiled)),
                    Event::AssetUploadFailed { .. } => Some(None),
                    other => {
                        let _kept = watch.transcript.take(watch.id, watch.prims, other);
                        None
                    }
                })
                .await
                .map_err(|failure| in_step(failure, &format!("saving script {name:?}")))?;
            let outcome = match compiled {
                Some(true) => "compiled=1",
                Some(false) => "compiled=0",
                None => "refused",
            };
            watch.transcript.body.push(format!(
                "# contents prim={label} op=save-script name={} {outcome}",
                quote(name)
            ));
            Ok(())
        }
    }
}

/// The other avatar makes a notecard called `name` and drops it into `prim`,
/// which it does not own; whatever the grid answers it is noted.
async fn drop_by_other(
    avatars: &mut Avatars<'_>,
    probe: &Probe,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
    (prim, label): (&Object, &str),
    name: &str,
) -> Result<(), TestFailure> {
    let folder = avatars.other_trash.ok_or_else(|| {
        TestFailure::State(format!(
            "{}: the other avatar's inventory was not read",
            probe.id
        ))
    })?;
    transcript.body.push(format!(
        "# contents by=other prim={label} op=drop name={}",
        quote(name)
    ));
    let other = &mut *avatars.other;
    let item = saved_notecard(other, (name, DROPPED_TEXT), folder, |event| {
        transcript.take_other(keys, event);
    })
    .await?;
    other
        .send(Command::UpdateTaskInventory {
            target: scoped_for(other, prim),
            key: TaskInventoryKey::Item,
            item: Box::new(drop_item(&item, prim.full_id)?),
        })
        .await?;
    collect(
        avatars.session,
        probe,
        keys,
        transcript,
        after(OTHER_SETTLE),
        Until::Deadline,
    )
    .await?;
    drain_other(avatars.other, keys, transcript).await
}

/// The owner puts prim A up for sale, the original for L$ 0, and the other
/// avatar buys it, as the reference viewer's buy dialog does.
pub(super) async fn buy(
    avatars: &mut Avatars<'_>,
    probe: &Probe,
    scene: &Scene<'_>,
    keys: &HashSet<ObjectKey>,
    transcript: &mut Transcript,
) -> Result<(), TestFailure> {
    let prim = labelled(scene, probe, transcript, "A")?;
    transcript
        .body
        .push("# sale prim=A type=original price=0".to_owned());
    avatars
        .session
        .send(Command::SetObjectForSale {
            local_id: scoped_for(avatars.session, &prim),
            sale_type: SaleType::Original,
            sale_price: Some(LindenAmount(0)),
        })
        .await?;
    collect(
        avatars.session,
        probe,
        keys,
        transcript,
        after(SALE_SETTLE),
        Until::Deadline,
    )
    .await?;
    transcript.body.push("# buy by=other prim=A".to_owned());
    avatars
        .other
        .send(Command::BuyObject {
            group_id: GroupKey::from(Uuid::nil()),
            category_id: Uuid::nil(),
            objects: vec![ObjectBuyItem {
                local_id: prim.local_id,
                sale_type: SaleType::Original,
                sale_price: LindenAmount(0),
            }],
        })
        .await?;
    collect(
        avatars.session,
        probe,
        keys,
        transcript,
        after(OTHER_SETTLE),
        Until::Deadline,
    )
    .await?;
    drain_other(avatars.other, keys, transcript).await
}
