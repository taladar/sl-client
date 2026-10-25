//! End-to-end test for the permission question a script puts to the avatar
//! (`llRequestPermissions`), answered on the viewer's permission card, against
//! each flavour of the fake grid ([[gridspec-lsl-avatar-control]]).
//!
//! Both live grids put the question alike (`book/src/gridspec/lsl.md`
//! § Animations, sitting, controls and camera): a `ScriptQuestion` naming the
//! object, its owner by legacy name and the mask asked, no experience — and
//! both take whatever mask the answer carries. Where they differ, the question
//! is not put at all (Second Life refuses camera control to an object the
//! avatar neither wears nor sits on, and answers a mask of 0 with nothing),
//! which leaves a viewer nothing to show. So each flavour is run through the
//! same steps: a question for three avatar-control permissions, granted on
//! the card's *Yes*, which the grid has from the viewer as that mask; and a
//! question for one, refused on its *No*, which the grid has as an empty
//! mask.
//!
//! The experience card has no live answer to be held to: a script in no
//! experience is refused `llRequestExperiencePermissions` at once on Second
//! Life (`experience_permissions_denied`, reason 5) with no question to the
//! viewer, OpenSim compiles no such script, and none of our avatars can save a
//! script into an experience ([[gridspec-lsl-experience]]). So the fake Second
//! Life flavour puts the question as the reference reads it — the ordinary
//! `ScriptQuestion` with its `Experience` block naming a catalogue experience
//! — and the card's *Yes* must both grant the mask and admit the experience.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_e2e::{BodyError, Stage, StageBuilder};
    use sl_fake_grid::ImitatedGrid;
    use sl_fake_grid::experiences::stranger_experience;
    use sl_proto::{
        ExperienceKey, InventoryKey, ObjectKey, ScriptPermissionRequest, ScriptPermissions, Uuid,
    };

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long the viewer is given to show what the grid sent, and the grid
    /// to hear the answer.
    const WAIT: Duration = Duration::from_secs(60);

    /// The notification a permission question raises.
    const QUESTION: &str = "ScriptQuestion";

    /// `PERMISSION_TAKE_CONTROLS | PERMISSION_TRIGGER_ANIMATION |
    /// PERMISSION_TRACK_CAMERA`, the mask the `avatar/permissions` probe asks.
    const AVATAR_CONTROL: i32 = 0x4 | 0x10 | 0x400;

    /// `PERMISSION_TRIGGER_ANIMATION`.
    const TRIGGER_ANIMATION: i32 = 0x10;

    /// The scripted object asking.
    fn task() -> ObjectKey {
        ObjectKey::from(Uuid::from_u128(0x9E53_0B1E))
    }

    /// Put to viewer `label` the question of the script `item` for `mask`,
    /// as both grids put it: the object's name, the owner's legacy name.
    async fn ask(
        stage: &Stage,
        label: &str,
        item: InventoryKey,
        mask: i32,
    ) -> Result<(), BodyError> {
        ask_under(stage, label, item, mask, None).await
    }

    /// [`ask`], under `experience` when it names one.
    async fn ask_under(
        stage: &Stage,
        label: &str,
        item: InventoryKey,
        mask: i32,
        experience: Option<ExperienceKey>,
    ) -> Result<(), BodyError> {
        let owner = stage.account_name(label)?.to_owned();
        let agent = stage.agent(label).await?;
        let now = agent.now();
        let question = ScriptPermissionRequest {
            task_id: task(),
            item_id: item,
            object_name: "Object".to_owned(),
            object_owner: owner,
            experience_id: experience,
            permissions: ScriptPermissions(mask),
        };
        agent
            .with_sim(|sim| sim.send_script_question(&question, now))
            .await
            .map_err(|error| format!("asking the question: {error}"))?;
        Ok(())
    }

    /// The answer the grid has from viewer `label` for the script `item`,
    /// once it has one.
    async fn answered(
        stage: &Stage,
        label: &str,
        item: InventoryKey,
    ) -> Result<ScriptPermissions, BodyError> {
        let agent = stage.agent(label).await?;
        let started = std::time::Instant::now();
        loop {
            if let Some(answer) = agent.with_sim(|sim| sim.script_grant(task(), item)).await {
                return Ok(answer);
            }
            if started.elapsed() >= WAIT {
                return Err("the grid never had an answer from the viewer".into());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// **The permission card on each flavour**: *Yes* grants the mask asked,
    /// *No* answers with an empty one.
    #[test]
    fn a_permission_question_is_granted_and_refused_on_the_card() -> Result<(), TestError> {
        for (flavour, name) in [
            (ImitatedGrid::SecondLife, "permissions_second_life"),
            (ImitatedGrid::OpenSim, "permissions_open_sim"),
        ] {
            StageBuilder::new(name)
                .viewer_binary(VIEWER)
                .viewer("Alpha")
                .configure_grid(move |grid| grid.imitates(flavour))
                .run(async |stage: &Stage| {
                    let alpha = &stage.viewer("Alpha")?;

                    let granted = InventoryKey::from(Uuid::from_u128(0x9E53_0001));
                    ask(stage, "Alpha", granted, AVATAR_CONTROL).await?;
                    let _shown = alpha
                        .expect_notification()
                        .timeout(WAIT)
                        .to_show(QUESTION)
                        .await?;
                    let _yes = alpha
                        .ui()
                        .test_id("script-permission-action:Yes")
                        .click()
                        .await?;
                    assert_eq!(
                        answered(stage, "Alpha", granted).await?,
                        ScriptPermissions(AVATAR_CONTROL),
                        "{flavour:?}: Yes grants the mask asked"
                    );

                    let refused = InventoryKey::from(Uuid::from_u128(0x9E53_0002));
                    ask(stage, "Alpha", refused, TRIGGER_ANIMATION).await?;
                    let _shown = alpha
                        .expect_notification()
                        .timeout(WAIT)
                        .to_show(QUESTION)
                        .await?;
                    let _no = alpha
                        .ui()
                        .test_id("script-permission-action:No")
                        .click()
                        .await?;
                    assert_eq!(
                        answered(stage, "Alpha", refused).await?,
                        ScriptPermissions(0),
                        "{flavour:?}: No answers with an empty mask"
                    );
                    Ok(())
                })?;
        }
        Ok(())
    }

    /// **The experience card on the Second Life flavour**: a question under
    /// an experience raises the experience card, not the permission card, and
    /// its *Yes* grants the mask asked and puts the experience on the agent's
    /// allowed list.
    #[test]
    fn an_experience_question_is_accepted_on_the_experience_card() -> Result<(), TestError> {
        StageBuilder::new("experience_permissions_second_life")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .configure_grid(|grid| grid.imitates(ImitatedGrid::SecondLife))
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let experience = stranger_experience();
                let item = InventoryKey::from(Uuid::from_u128(0x9E53_0003));
                ask_under(stage, "Alpha", item, AVATAR_CONTROL, Some(experience)).await?;
                let _yes = alpha
                    .ui()
                    .test_id("experience-permission-action:Yes")
                    .timeout(WAIT)
                    .click()
                    .await?;
                assert_eq!(
                    answered(stage, "Alpha", item).await?,
                    ScriptPermissions(AVATAR_CONTROL),
                    "Yes grants the mask asked"
                );
                let agent = stage.agent("Alpha").await?;
                let started = std::time::Instant::now();
                loop {
                    let (allowed, _blocked) = agent
                        .with_sim(|sim| sim.experiences().agent_permissions())
                        .await;
                    if allowed.contains(&experience) {
                        break;
                    }
                    if started.elapsed() >= WAIT {
                        return Err(format!(
                            "Yes never admitted the experience: allowed {allowed:?}"
                        )
                        .into());
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Ok(())
            })?;
        Ok(())
    }
}
