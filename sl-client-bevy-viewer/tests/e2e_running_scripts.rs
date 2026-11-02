//! End-to-end test for the cards a *running* script puts to the avatar,
//! answered on the viewer and heard back by the script, against the fake
//! grid's script engine ([[server-fake-grid-script-engine-wiring]]).
//!
//! `e2e_script_dialog` and `e2e_script_permissions` hold the cards to what
//! the live grids send, put by the test itself. Here a script in the region
//! puts them: told on channel 7 by the avatar, it calls `llDialog` or
//! `llRequestExperiencePermissions`, and says what came of it on channel 9,
//! where the test listens. So the whole round trip is held — the engine's
//! `Host` putting the card, the viewer's reply, the grid turning the reply
//! into a line the script's `listen` hears (a dialog) or into the
//! `experience_permissions` event (the experience card), and the agent's
//! acceptance recorded for the experience.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_fake_grid::chat::{Line, ListenFilter};
    use sl_fake_grid::experiences::stranger_experience;
    use sl_fake_grid::world::{TaskInventory, box_prim, region_wide_parcel};
    use sl_fake_grid::{ImitatedGrid, RegionConfig, RegionFixture};
    use sl_proto::{
        AgentKey, AssetKey, ChatChannel, ChatSource, ChatType, ExperienceKey, InventoryKey,
        ObjectKey, OwnerKey, ParcelKey, RegionLocalObjectId, RegionLocalParcelId,
        TaskInventoryItem, Uuid, Vector,
    };

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long the viewer is given to show what the script put, and the
    /// script to say what came of the answer.
    const WAIT: Duration = Duration::from_secs(60);

    /// The channel the script is told on.
    const ASK: ChatChannel = ChatChannel(7);

    /// The channel the script says what came of it on.
    const TOLD: ChatChannel = ChatChannel(9);

    /// The scripted box's region-local id.
    const BOX: RegionLocalObjectId = RegionLocalObjectId(0x7E0);

    /// Where the box stands: beside the arrival point.
    const BOX_AT: Vector = Vector {
        x: 130.0,
        y: 128.0,
        z: 25.5,
    };

    /// A script that puts a three-button dialog on channel 8 to whoever says
    /// `dialog` on channel 7, and says on channel 9 what the dialog answered.
    const DIALOG_SOURCE: &str = "default
{
    state_entry()
    {
        llListen(7, \"\", NULL_KEY, \"dialog\");
        llListen(8, \"\", NULL_KEY, \"\");
    }

    listen(integer channel, string name, key id, string message)
    {
        if (channel == 7)
        {
            llDialog(id, \"Pick a colour\", [\"Red\", \"Green\", \"Blue\"], 8);
        }
        else
        {
            llSay(9, \"Picked: \" + message);
        }
    }
}
";

    /// A script of an experience that asks whoever says `ask` on channel 7
    /// for the experience's permissions, and says on channel 9 what came of
    /// it.
    const EXPERIENCE_SOURCE: &str = "default
{
    state_entry()
    {
        llListen(7, \"\", NULL_KEY, \"ask\");
    }

    listen(integer channel, string name, key id, string message)
    {
        llRequestExperiencePermissions(id, \"\");
    }

    experience_permissions(key agent)
    {
        llSay(9, \"granted\");
    }

    experience_permissions_denied(key agent, integer reason)
    {
        llSay(9, \"denied \" + (string)reason);
    }
}
";

    /// The box's key.
    fn box_key() -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(0x7E05))
    }

    /// The prim the test listens as: beside the box, since a listen never
    /// hears its own object.
    const EAR: RegionLocalObjectId = RegionLocalObjectId(0x7E1);

    /// The listening prim's key.
    fn ear_key() -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(0x7E15))
    }

    /// A region of one public parcel, the box, its one script `source`
    /// running under `experience`, and the prim the test listens as.
    fn region(source: &str, experience: Option<ExperienceKey>) -> Result<RegionConfig, TestError> {
        let owner = AgentKey::from(Uuid::from_u128(0x7E00));
        let asset = AssetKey::from(Uuid::from_u128(0x7E06));
        let mut fixture = RegionFixture::new();
        fixture.world.add_parcel(
            region_wide_parcel(RegionLocalParcelId(1), OwnerKey::Agent(owner), "Scripts"),
            ParcelKey::from(Uuid::from_u128(0x7E0A)),
            0.0,
        );
        fixture.world.objects.push(box_prim(
            BOX,
            box_key(),
            owner,
            BOX_AT,
            Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        ));
        fixture.world.objects.push(box_prim(
            EAR,
            ear_key(),
            owner,
            Vector {
                x: BOX_AT.x + 2.0,
                ..BOX_AT
            },
            Vector {
                x: 0.5,
                y: 0.5,
                z: 0.5,
            },
        ));
        let item = TaskInventoryItem {
            item_id: InventoryKey::from(Uuid::from_u128(0x7E07)),
            parent_task: box_key(),
            asset_id: Some(asset),
            name: "Script".to_owned(),
            ..sl_fake_grid::scenario::stock_script_item()
        };
        let mut contents = TaskInventory::stated(1, vec![item.clone()]);
        let _running = contents.running.insert(item.item_id);
        if let Some(experience) = experience {
            let _previous = contents.experiences.insert(item.item_id, experience);
        }
        fixture
            .world
            .add_task_inventory(BOX, contents)
            .map_err(|_contents| "the box is not in the region")?;
        let _previous = fixture.assets.insert(asset, source.as_bytes().to_vec());
        Ok(fixture.into_region(RegionConfig::default()))
    }

    /// What the script says on [`TOLD`], as the test hears it.
    type Told = tokio::sync::mpsc::UnboundedReceiver<Line>;

    /// Listen on [`TOLD`] as the listening prim, in viewer `label`'s
    /// region.
    async fn listen(stage: &Stage, label: &str) -> Result<Told, BodyError> {
        let agent = stage.agent(label).await?;
        let (_id, told) = agent
            .with_world(|world, _sim| world.listens.listen(ear_key(), ListenFilter::channel(TOLD)))
            .await;
        Ok(told)
    }

    /// Viewer `label`'s avatar says `message` on [`ASK`] beside the box.
    async fn tell(stage: &Stage, label: &str, message: &str) -> Result<(), BodyError> {
        let agent_id = stage.agent_id(label)?;
        let name = stage.account_name(label)?.to_owned();
        let agent = stage.agent(label).await?;
        agent
            .say(Line {
                source: ChatSource::Agent(agent_id),
                from_name: name,
                owner_id: agent_id.uuid(),
                chat_type: ChatType::Normal,
                channel: ASK,
                position: Vector {
                    x: BOX_AT.x - 2.0,
                    ..BOX_AT
                },
                message: message.to_owned(),
            })
            .await;
        Ok(())
    }

    /// The next line the script says on [`TOLD`].
    async fn told(told: &mut Told) -> Result<String, BodyError> {
        let line = tokio::time::timeout(WAIT, told.recv())
            .await
            .map_err(|_elapsed| "the script said nothing")?
            .ok_or("the listen was dropped")?;
        Ok(line.message)
    }

    /// **A running script's dialog on each flavour**: told `dialog`, the
    /// script puts its card; the viewer's *Green* is the line its `listen`
    /// hears on the dialog's channel.
    #[test]
    fn a_running_script_hears_its_dialog_answered() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "running_dialog_second_life"),
            (ImitatedGrid::OpenSim, "running_dialog_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .region(region(DIALOG_SOURCE, None)?)
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async |stage: &Stage| {
                    let alpha = &stage.viewer("Alpha")?;
                    let mut said = listen(stage, "Alpha").await?;
                    tell(stage, "Alpha", "dialog").await?;
                    let _shown = alpha
                        .expect_notification()
                        .timeout(WAIT)
                        .to_show("ScriptDialog")
                        .await?;
                    let _green = alpha
                        .ui()
                        .test_id("script-dialog-button:Green")
                        .click()
                        .await?;
                    assert_eq!(
                        told(&mut said).await?,
                        "Picked: Green",
                        "{flavour:?}: the script's listen"
                    );
                    Ok(())
                })?;
        }
        Ok(())
    }

    /// **A running script's experience card on the Second Life flavour**:
    /// told `ask`, a script of an experience the agent has not answered puts
    /// the experience card; its *Yes* raises `experience_permissions` in the
    /// script and puts the experience on the agent's allowed list.
    #[test]
    fn a_running_script_is_granted_its_experience_on_the_card() -> Result<(), TestError> {
        let experience = stranger_experience();
        StageBuilder::new("running_experience_second_life")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .region(region(EXPERIENCE_SOURCE, Some(experience))?)
            .configure_grid(|grid| grid.imitates(ImitatedGrid::SecondLife))
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let mut said = listen(stage, "Alpha").await?;
                tell(stage, "Alpha", "ask").await?;
                let _yes = alpha
                    .ui()
                    .test_id("experience-permission-action:Yes")
                    .timeout(WAIT)
                    .click()
                    .await?;
                assert_eq!(told(&mut said).await?, "granted", "the script's event");
                let agent = stage.agent("Alpha").await?;
                let (allowed, _blocked) = agent
                    .with_sim(|sim| sim.experiences().agent_permissions())
                    .await;
                assert!(
                    allowed.contains(&experience),
                    "the experience was not allowed: {allowed:?}"
                );
                Ok(())
            })?;
        Ok(())
    }
}
