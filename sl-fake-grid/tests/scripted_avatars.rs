//! Scripted avatars: the grid logs in a second real client of its own and
//! drives it through the scripted-avatar scene — it walks to the pose ball and
//! sits on it and the ball poses it, pays the vendor its
//! price, grants the permission prim what it asks, and answers the menu's
//! dialog with a named button — each script saying what it saw, the avatar's
//! transcript saying what it answered, and a viewer standing beside it seeing it
//! seated and posing. Two runs of one seed write the same transcript, and an
//! avatar that answers nothing leaves the question it is asked open.

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_client_tokio::{Client, Command, Event, LoginParams, LoginRequest, StartLocation};
    use sl_fake_grid::fixtures::scripted_avatar::{
        MENU, PERMISSION_PRIM, POSE_BALL, VENDOR, VENDOR_PRICE, position, region,
    };
    use sl_fake_grid::scripted_avatars::Entry;
    use sl_fake_grid::{
        AccountConfig, AnswerPolicy, AvatarAction, DialogChoice, FakeGrid, FakeGridBuilder,
        HeartbeatConfig, ImitatedGrid, Progress, RegionConfig, ScriptedAvatar, Target,
    };
    use sl_types::lsl::{ScriptPermissions, Vector};
    use tokio::sync::mpsc;

    type TestError = Box<dyn core::error::Error>;

    /// How long any single wait in these tests may take.
    const WAIT: Duration = Duration::from_secs(60);

    /// The stock region's name.
    const REGION: &str = "Fake Region";

    /// The seed both runs of the determinism test share.
    const SEED: u64 = 0x5C_A1AB;

    /// The prop called `name`, as a step names it.
    fn named(name: &str) -> Target {
        Target::Named(name.to_owned())
    }

    /// Where the walk goes: a metre in front of the pose ball.
    fn beside_the_pose_ball() -> Result<Vector, TestError> {
        let ball = position(POSE_BALL).ok_or("the scene has no pose ball")?;
        Ok(Vector {
            x: ball.x,
            y: ball.y - 1.0,
            z: ball.z,
        })
    }

    /// The whole conversation: walk to the pose ball, sit, hear it pose; pay
    /// the vendor its price, hear its thanks; touch the permission prim, hear
    /// its grant; touch the menu, answer "Blue", hear it chosen.
    fn conversationalist() -> Result<ScriptedAvatar, TestError> {
        Ok(ScriptedAvatar::new("Scripted", "Sitter")
            .answering(AnswerPolicy::agreeable())
            .after_ticks(
                1,
                AvatarAction::Walk {
                    to: beside_the_pose_ball()?,
                },
            )
            .after_ticks(1, AvatarAction::Sit(named(POSE_BALL)))
            .after_ticks(0, AvatarAction::AwaitChat("Posing".to_owned()))
            .after_ticks(
                1,
                AvatarAction::Pay {
                    target: named(VENDOR),
                    amount: None,
                },
            )
            .after_ticks(0, AvatarAction::AwaitChat("Thank you".to_owned()))
            .after_ticks(1, AvatarAction::Touch(named(PERMISSION_PRIM)))
            .after_ticks(0, AvatarAction::AwaitChat("Granted".to_owned()))
            .after_ticks(1, AvatarAction::Touch(named(MENU)))
            .after_ticks(
                0,
                AvatarAction::AnswerDialog(DialogChoice::Label("Blue".to_owned())),
            )
            .after_ticks(0, AvatarAction::AwaitChat("chose".to_owned())))
    }

    /// A grid imitating `imitated`, seeded, its one region the scripted-avatar
    /// scene under a heartbeat stepped by hand, holding `avatar` and an
    /// account for an observing viewer.
    async fn grid(imitated: ImitatedGrid, avatar: ScriptedAvatar) -> Result<FakeGrid, TestError> {
        let mut config = region().into_region(RegionConfig {
            heartbeat: HeartbeatConfig::stepped(Duration::from_millis(100)),
            ..RegionConfig::default()
        });
        config.scripted_avatars.push(avatar);
        Ok(FakeGridBuilder::new()
            .deterministic(SEED)
            .imitates(imitated)
            .account(AccountConfig::new("Test", "User", "password"))
            .region(config)
            .start()
            .await?)
    }

    /// Ticks the region until the grid's one scripted avatar has run its
    /// script, giving its client a moment between ticks to act on the last.
    async fn play(grid: &FakeGrid) -> Result<Vec<Entry>, TestError> {
        let avatar = grid
            .scripted_avatars()
            .first()
            .ok_or("the grid logged in no avatar")?
            .clone();
        let arrived = tokio::time::timeout(WAIT, avatar.arrived()).await?;
        if arrived != Progress::Running {
            return Err(format!("the avatar did not arrive: {arrived:?}").into());
        }
        tokio::time::timeout(WAIT, async {
            loop {
                match avatar.progress() {
                    Progress::Finished => return Ok(()),
                    Progress::Stopped(reason) => return Err(reason),
                    _other => {}
                }
                grid.advance_one_tick(REGION)
                    .await
                    .map_err(|error| error.to_string())?;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_elapsed| {
            format!(
                "the avatar never finished its script; it got as far as {:#?}",
                avatar.transcript()
            )
        })??;
        Ok(avatar.transcript())
    }

    /// Every line of a transcript, as it reads.
    fn lines(transcript: &[Entry]) -> Vec<String> {
        transcript.iter().map(ToString::to_string).collect()
    }

    /// What the conversation writes, word for word.
    fn conversation() -> Result<Vec<String>, TestError> {
        let to = beside_the_pose_ball()?;
        Ok(vec![
            "arrived".to_owned(),
            format!("walked to <{}, {}, {}>", to.x, to.y, to.z),
            "sat on Pose ball".to_owned(),
            "heard Pose ball: Posing Scripted Sitter".to_owned(),
            format!("paid Vendor L${VENDOR_PRICE}"),
            format!("heard Vendor: Thank you Scripted Sitter for L${VENDOR_PRICE}"),
            "touched Permission prim".to_owned(),
            "Permission prim asked for permissions 0x404".to_owned(),
            "granted Permission prim permissions 0x404".to_owned(),
            "heard Permission prim: Granted 1028".to_owned(),
            "touched Menu".to_owned(),
            "Menu showed a dialog \"Pick a colour\" [\"Red\", \"Green\", \"Blue\"]".to_owned(),
            "answered Menu with \"Blue\"".to_owned(),
            "heard Menu: Scripted Sitter chose Blue".to_owned(),
        ])
    }

    /// **The whole conversation on Second Life.** The avatar walks to the pose
    /// ball and is seated, and the ball poses it — a sitter's animation is
    /// granted without a question, so none is asked; it pays the vendor what its pay window offers and is thanked
    /// by name and amount; the permission prim's question is granted by
    /// policy; and the menu's dialog is answered "Blue", which its script
    /// says it heard. A viewer logged in beside it sees it seated on the ball
    /// and playing the ball's pose.
    #[tokio::test]
    async fn a_scripted_avatar_answers_every_prop_on_second_life() -> Result<(), TestError> {
        let grid = grid(ImitatedGrid::SecondLife, conversationalist()?).await?;
        let observer = Client::connect(LoginParams {
            login_uri: grid.login_uri(),
            request: LoginRequest::new(
                "Test",
                "User",
                "password",
                StartLocation::Last,
                "sl-fake-grid-scripted-avatars",
                "0.0",
            ),
        })
        .await?;
        let (event_tx, mut events) = mpsc::channel::<Event>(256);
        let (_command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(async move {
            let _ended = observer.run(event_tx, diag_tx, command_rx).await;
        });
        // What the observer saw of the scripted avatar: the animations it was
        // last said to play, and the object its body was last parented to.
        let (seen_tx, mut seen) = mpsc::unbounded_channel();
        let watch = tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let noted = match event {
                    Event::AvatarAnimation {
                        avatar_id,
                        animations,
                        ..
                    } => Some((
                        Some(avatar_id),
                        animations.iter().map(|playing| playing.anim_id).collect(),
                        None,
                    )),
                    Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                        if object.pcode == sl_proto::pcode::AVATAR =>
                    {
                        Some((
                            Some(sl_types::key::AgentKey::from(object.full_id.uuid())),
                            Vec::new(),
                            Some(object.parent_id),
                        ))
                    }
                    _other => None,
                };
                if let Some(noted) = noted
                    && seen_tx.send(noted).is_err()
                {
                    return;
                }
            }
        });

        let transcript = play(&grid).await?;
        assert_eq!(lines(&transcript), conversation()?);

        let sitter = grid
            .scripted_avatar("Scripted Sitter")
            .and_then(sl_fake_grid::ScriptedAvatarHandle::agent_id)
            .ok_or("the scripted avatar has no agent id")?;
        let pose = sl_fake_grid::fixtures::scripted_avatar::POSE_ANIMATION;
        let ball = grid
            .sessions_in(REGION)
            .await
            .into_iter()
            .find(|agent| agent.agent_id() == sitter)
            .ok_or("the scripted avatar has no session")?
            .with_world(|world, _sim| {
                sl_fake_grid::fixtures::scripted_avatar::key(POSE_BALL)
                    .and_then(|key| world.local_id_of(key))
            })
            .await
            .ok_or("the pose ball is gone")?;
        let (mut posed, mut seated) = (false, false);
        let mut lists = Vec::new();
        tokio::time::timeout(WAIT, async {
            while !(posed && seated) {
                let Some((avatar, animations, parent)) = seen.recv().await else {
                    break;
                };
                lists.push(format!("{avatar:?} {animations:?} {parent:?}"));
                if avatar != Some(sitter) {
                    continue;
                }
                if !animations.is_empty() {
                    posed = animations.contains(&pose);
                }
                if let Some(parent) = parent {
                    seated = parent == ball;
                }
            }
        })
        .await
        .map_err(|_elapsed| format!("the viewer saw it posed {posed}, seated {seated}; sitter {sitter:?} pose {pose:?} {lists:#?}"))?;
        assert!(posed && seated, "posed {posed}, seated {seated}");

        let avatar = grid
            .scripted_avatar("Scripted Sitter")
            .ok_or("no scripted avatar")?;
        let ended = tokio::time::timeout(WAIT, avatar.log_out()).await?;
        assert!(matches!(ended, Progress::Stopped(_)), "{ended:?}");
        grid.shutdown();
        run.abort();
        watch.abort();
        Ok(())
    }

    /// **The pose ball and the menu on OpenSim**: seated and posing with no
    /// question asked, and the dialog answered by its first button by policy.
    #[tokio::test]
    async fn a_scripted_avatar_sits_and_answers_on_opensim() -> Result<(), TestError> {
        let avatar = ScriptedAvatar::new("Scripted", "Sitter")
            .answering(AnswerPolicy::agreeable())
            .after_ticks(1, AvatarAction::Sit(named(POSE_BALL)))
            .after_ticks(0, AvatarAction::AwaitChat("Posing".to_owned()))
            .after_ticks(1, AvatarAction::Touch(named(MENU)))
            .after_ticks(0, AvatarAction::AwaitChat("chose".to_owned()));
        let grid = grid(ImitatedGrid::OpenSim, avatar).await?;
        let transcript = play(&grid).await?;
        assert_eq!(
            lines(&transcript),
            [
                "arrived",
                "sat on Pose ball",
                "heard Pose ball: Posing Scripted Sitter",
                "touched Menu",
                "Menu showed a dialog \"Pick a colour\" [\"Red\", \"Green\", \"Blue\"]",
                "answered Menu with \"Red\"",
                "heard Menu: Scripted Sitter chose Red",
            ]
        );
        grid.shutdown();
        Ok(())
    }

    /// **A sit plays `sit` until the seat's script replaces it**, on both
    /// flavours, as a viewer beside the sitter is told it: seated on the pose
    /// ball, the sitter plays the built-in `sit` alone; the ball's
    /// `llStopAnimation("sit")` leaves it playing nothing and its
    /// `llStartAnimation` the pose; and standing up brings `stand` back under
    /// the pose, which the stand does not stop (`avatar/sit-animation`,
    /// 2026-11-12: OpenSim states each of these, and Second Life the start,
    /// the stop and the pose kept past the stand).
    #[tokio::test]
    async fn a_sitter_plays_sit_until_its_seat_poses_it() -> Result<(), TestError> {
        /// The built-in `sit`.
        const SIT: uuid::Uuid = uuid::uuid!("1a5fe8ac-a804-8a5d-7cbd-56bd83184568");
        let pose = sl_fake_grid::fixtures::scripted_avatar::POSE_ANIMATION;
        let stand = sl_fake_grid::animations::STAND;
        for imitated in [ImitatedGrid::SecondLife, ImitatedGrid::OpenSim] {
            let avatar = ScriptedAvatar::new("Scripted", "Sitter")
                .answering(AnswerPolicy::agreeable())
                .after_ticks(
                    1,
                    AvatarAction::Walk {
                        to: beside_the_pose_ball()?,
                    },
                )
                .after_ticks(1, AvatarAction::Sit(named(POSE_BALL)))
                .after_ticks(0, AvatarAction::AwaitChat("Posing".to_owned()))
                .after_ticks(5, AvatarAction::Stand);
            let grid = grid(imitated, avatar).await?;
            let observer = Client::connect(LoginParams {
                login_uri: grid.login_uri(),
                request: LoginRequest::new(
                    "Test",
                    "User",
                    "password",
                    StartLocation::Last,
                    "sl-fake-grid-scripted-avatars",
                    "0.0",
                ),
            })
            .await?;
            let (event_tx, mut events) = mpsc::channel::<Event>(256);
            let (_command_tx, command_rx) = mpsc::channel::<Command>(8);
            let (diag_tx, _diag_rx) = mpsc::channel(16);
            let run = tokio::spawn(async move {
                let _ended = observer.run(event_tx, diag_tx, command_rx).await;
            });
            // Every set the observer is told, read on as the grid ticks so
            // the client never waits on a full channel.
            let (sets_tx, mut sets) = mpsc::unbounded_channel();
            let watch = tokio::spawn(async move {
                while let Some(event) = events.recv().await {
                    if let Event::AvatarAnimation {
                        avatar_id,
                        animations,
                        ..
                    } = event
                        && sets_tx
                            .send((
                                avatar_id,
                                animations
                                    .iter()
                                    .map(|playing| playing.anim_id)
                                    .collect::<Vec<_>>(),
                            ))
                            .is_err()
                    {
                        return;
                    }
                }
            });

            let _transcript = play(&grid).await?;
            let sitter = grid
                .scripted_avatar("Scripted Sitter")
                .and_then(sl_fake_grid::ScriptedAvatarHandle::agent_id)
                .ok_or("the scripted avatar has no agent id")?;
            // The sitter's sets, from the first that holds `sit`, until it
            // stands again under its pose.
            let mut told: Vec<Vec<uuid::Uuid>> = Vec::new();
            tokio::time::timeout(WAIT, async {
                while told.last() != Some(&vec![stand, pose]) {
                    while let Ok((avatar, set)) = sets.try_recv() {
                        if avatar == sitter
                            && (!told.is_empty() || set.contains(&SIT))
                            && told.last() != Some(&set)
                        {
                            told.push(set);
                        }
                    }
                    grid.advance_one_tick(REGION)
                        .await
                        .map_err(|error| error.to_string())?;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Ok::<(), String>(())
            })
            .await
            .map_err(|_elapsed| format!("{imitated:?}: the sitter was told {told:#?}"))??;
            // A stand that puts the avatar above the ground (OpenSim's) drops
            // it first: its fall and landing under the pose are its posture's.
            let told: Vec<Vec<uuid::Uuid>> = told
                .into_iter()
                .filter(|set| {
                    !matches!(set.as_slice(), [first, second] if *second == pose && *first != stand)
                })
                .collect();
            assert_eq!(
                told,
                vec![vec![SIT], vec![], vec![pose], vec![stand, pose]],
                "{imitated:?}"
            );
            grid.shutdown();
            run.abort();
            watch.abort();
        }
        Ok(())
    }

    /// **The named scene, as a harness runs it**: its region ticking by itself,
    /// its sitter seated on the pose ball, and a viewer that logs in well after
    /// the pose ball has posed it, as `sl-crosscheck`'s does, told on arrival
    /// that it is posing.
    #[tokio::test]
    async fn the_named_scene_shows_a_late_viewer_its_sitter_posing() -> Result<(), TestError> {
        let scene = sl_fake_grid::fixtures::scenarios::scenario("scripted-avatar")
            .ok_or("no scripted-avatar scene")?;
        let grid = FakeGridBuilder::new()
            .deterministic(SEED)
            .account(AccountConfig::new("Test", "User", "password"))
            .region(scene.dress(RegionConfig::default()))
            .start()
            .await?;
        let sitter = grid
            .scripted_avatar("Scripted Sitter")
            .ok_or("the scene logged in no sitter")?
            .clone();
        let finished = tokio::time::timeout(WAIT, sitter.finished()).await?;
        assert_eq!(finished, Progress::Finished, "{:#?}", sitter.transcript());
        assert_eq!(
            lines(&sitter.transcript()),
            ["arrived", "sat on Pose ball"],
            "the sitter's script"
        );
        let agent = sitter.agent_id().ok_or("the sitter has no agent id")?;
        let pose = sl_fake_grid::fixtures::scripted_avatar::POSE_ANIMATION;
        // Twenty ticks on: the pose ball has long since posed its sitter, so
        // the viewer is told it in its arrival, not as it happens.
        let mut ticks = grid.ticks(REGION)?;
        let until = ticks.borrow_and_update().number.saturating_add(20);
        tokio::time::timeout(WAIT, ticks.wait_for(|tick| tick.number >= until)).await??;
        let viewer = Client::connect(LoginParams {
            login_uri: grid.login_uri(),
            request: LoginRequest::new(
                "Test",
                "User",
                "password",
                StartLocation::Last,
                "sl-fake-grid-scripted-avatars",
                "0.0",
            ),
        })
        .await?;
        let (event_tx, mut told) = mpsc::channel::<Event>(256);
        let (_command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(async move {
            let _ended = viewer.run(event_tx, diag_tx, command_rx).await;
        });
        let posed = tokio::time::timeout(WAIT, async {
            while let Some(event) = told.recv().await {
                if let Event::AvatarAnimation {
                    avatar_id,
                    animations,
                    ..
                } = event
                    && avatar_id == agent
                    && animations.iter().any(|playing| playing.anim_id == pose)
                {
                    return true;
                }
            }
            false
        })
        .await
        .map_err(|_elapsed| "the late viewer was never told the pose")?;
        assert!(posed, "the late viewer's client stopped");
        grid.shutdown();
        run.abort();
        Ok(())
    }

    /// The lower-body bake a viewer beside the scripted avatar `avatar` is
    /// shown, decoded, on a Second-Life-flavoured grid: what it is told of the
    /// avatar's appearance, fetched from the appearance service its login
    /// named.
    async fn lower_bake_seen_beside(avatar: ScriptedAvatar) -> Result<Vec<u8>, TestError> {
        let grid = grid(ImitatedGrid::SecondLife, avatar).await?;
        let scripted = grid
            .scripted_avatars()
            .first()
            .ok_or("the grid logged in no avatar")?
            .clone();
        let arrived = tokio::time::timeout(WAIT, scripted.arrived()).await?;
        if arrived != Progress::Running && arrived != Progress::Finished {
            return Err(format!("the avatar did not arrive: {arrived:?}").into());
        }
        let agent = scripted.agent_id().ok_or("the avatar has no agent id")?;
        let viewer = Client::connect(LoginParams {
            login_uri: grid.login_uri(),
            request: LoginRequest::new(
                "Test",
                "User",
                "password",
                StartLocation::Last,
                "sl-fake-grid-scripted-avatars",
                "0.0",
            ),
        })
        .await?;
        let service = viewer
            .login_success()
            .and_then(|success| success.agent_appearance_service.clone())
            .ok_or("a central-baking login named no appearance service")?;
        let (event_tx, mut told) = mpsc::channel::<Event>(256);
        let (_command_tx, command_rx) = mpsc::channel::<Command>(8);
        let (diag_tx, _diag_rx) = mpsc::channel(16);
        let run = tokio::spawn(async move {
            let _ended = viewer.run(event_tx, diag_tx, command_rx).await;
        });
        let appearance = tokio::time::timeout(WAIT, async {
            while let Some(event) = told.recv().await {
                if let Event::AvatarAppearance(appearance) = event
                    && appearance.avatar_id == agent
                {
                    return Some(appearance);
                }
            }
            None
        })
        .await
        .map_err(|_elapsed| "the viewer was never told the avatar's appearance")?
        .ok_or("the viewer's client stopped")?;
        let lower = appearance
            .texture_entry
            .texture_id(sl_proto::avatar_texture::LOWER_BAKED)
            .ok_or("the appearance names no lower-body bake")?;
        let url = service.join(&format!("texture/{}/lower/{}", agent.uuid(), lower.uuid()))?;
        let bytes = reqwest::get(url).await?.error_for_status()?.bytes().await?;
        let decoded = sl_texture::decode_j2c(&bytes, sl_proto::j2c::DiscardLevel::FULL)
            .map_err(|error| format!("the lower-body bake did not decode: {error}"))?;
        grid.shutdown();
        run.abort();
        Ok(decoded.pixels.to_vec())
    }

    /// **A dressed scripted avatar is shown in its clothes.** The dressed
    /// account's pants and shoes name no texture, and the bake left such a
    /// layer off, so a viewer beside it saw the bare legs an undressed avatar
    /// has; the reference draws the layer with the slot's default, plain white
    /// fabric, and so does the grid's bake now, so the two avatars' lower-body
    /// bakes differ.
    #[tokio::test]
    async fn a_dressed_scripted_avatar_is_shown_in_its_clothes() -> Result<(), TestError> {
        let bare = lower_bake_seen_beside(ScriptedAvatar::new("Scripted", "Sitter")).await?;
        let dressed =
            lower_bake_seen_beside(ScriptedAvatar::new("Scripted", "Sitter").dressed()).await?;
        assert_eq!(
            bare.len(),
            dressed.len(),
            "the bakes are of different sizes"
        );
        assert!(
            bare != dressed,
            "the dressed avatar's lower body is baked bare"
        );
        Ok(())
    }

    /// **Two runs of one seed write one transcript.**
    #[tokio::test]
    async fn two_runs_of_one_seed_write_one_transcript() -> Result<(), TestError> {
        let mut transcripts = Vec::new();
        for _run in 0..2 {
            let grid = grid(ImitatedGrid::SecondLife, conversationalist()?).await?;
            transcripts.push(play(&grid).await?);
            grid.shutdown();
        }
        assert_eq!(transcripts.first(), transcripts.get(1));
        Ok(())
    }

    /// **An avatar that answers nothing leaves the question open**, and one
    /// that refuses is refused: the permission prim hears no grant from the
    /// first, and hears an empty one from the second.
    #[tokio::test]
    async fn an_unanswered_question_stays_open_and_a_refusal_is_heard() -> Result<(), TestError> {
        let avatar = ScriptedAvatar::new("Scripted", "Sitter")
            .after_ticks(1, AvatarAction::Touch(named(PERMISSION_PRIM)))
            .after_ticks(5, AvatarAction::Touch(named(PERMISSION_PRIM)))
            .after_ticks(
                0,
                AvatarAction::AnswerPermissions(ScriptPermissions::default()),
            )
            .after_ticks(0, AvatarAction::AwaitChat("Granted".to_owned()));
        let grid = grid(ImitatedGrid::SecondLife, avatar).await?;
        let transcript = play(&grid).await?;
        assert_eq!(
            lines(&transcript),
            [
                "arrived",
                "touched Permission prim",
                "Permission prim asked for permissions 0x404",
                "touched Permission prim",
                "Permission prim asked for permissions 0x404",
                "refused Permission prim its permissions",
                "heard Permission prim: Granted 0",
            ]
        );
        grid.shutdown();
        Ok(())
    }
}
