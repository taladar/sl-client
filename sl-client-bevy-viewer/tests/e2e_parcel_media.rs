//! End-to-end tests for **legacy whole-parcel media** (`viewer-video-playback`)
//! on the fake grid, through the automation driver alone: the grid states the
//! parcel's media in a `ParcelMediaUpdate` and drives it with the
//! `ParcelMediaCommandMessage`s a script's `llParcelMediaCommandList` sends,
//! and the viewer has to know the media, refuse a script's play while scripted
//! media is off, play it on the faces wearing its texture once it is allowed,
//! list it in the Nearby Media window, and stop it again.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use pretty_assertions::assert_eq;
    use serde_json::json;
    use sl_automation_proto::{Locator, Probe, Role};
    use sl_e2e::{BodyError, Need, Stage, StageBuilder};
    use sl_proto::{ParcelMediaCommand, ParcelMediaUpdateInfo, TextureKey};
    use sl_viewer_driver::Viewer;

    /// A failed stage, or a test that could not set one up.
    type TestError = Box<dyn core::error::Error>;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");

    /// How long something the viewer has to take in may take.
    const WAIT: Duration = Duration::from_secs(60);

    /// How long a refused play is given to (wrongly) start something.
    const REFUSED: Duration = Duration::from_secs(3);

    /// The media URL: a closed local port, so the video engine builds its
    /// pipeline and fetches nothing.
    const MEDIA_URL: &str = "http://127.0.0.1:9/feature.mp4";

    /// `PARCEL_MEDIA_COMMAND_STOP`'s flag bit.
    const STOP_BIT: u32 = 1 << 0;

    /// `PARCEL_MEDIA_COMMAND_PLAY`'s flag bit.
    const PLAY_BIT: u32 = 1 << 2;

    /// The setting that lets a script start the parcel's media.
    const SCRIPTED_MEDIA: &str = "PermAllowScriptedMedia";

    /// A one-viewer stage with the video engine on.
    fn stage(name: &str) -> StageBuilder {
        StageBuilder::new(name)
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .video_media()
            .needs(Need::GridControl)
    }

    /// The parcel's media: a video replacing the default plywood texture, which
    /// every prim of the stock scene wears.
    fn media() -> Result<ParcelMediaUpdateInfo, url::ParseError> {
        Ok(ParcelMediaUpdateInfo {
            media_url: Some(url::Url::parse(MEDIA_URL)?),
            media_id: Some(TextureKey::from(sl_proto::DEFAULT_PRIM_TEXTURE)),
            media_auto_scale: true,
            media_type: "video/mp4".to_owned(),
            media_desc: String::new(),
            media_width: None,
            media_height: None,
            media_loop: true,
        })
    }

    /// Have the grid send viewer `label` the parcel's media.
    async fn state_media(stage: &Stage, label: &str) -> Result<(), BodyError> {
        let media = media()?;
        let agent = stage.agent(label).await?;
        let now = agent.now();
        agent
            .with_sim(|sim| sim.send_parcel_media_update(&media, now))
            .await
            .map_err(|error| format!("sending the parcel media: {error}"))?;
        Ok(())
    }

    /// Have the grid send viewer `label` a script's media command.
    async fn command(
        stage: &Stage,
        label: &str,
        flags: u32,
        command: ParcelMediaCommand,
    ) -> Result<(), BodyError> {
        let agent = stage.agent(label).await?;
        let now = agent.now();
        agent
            .with_sim(|sim| sim.send_parcel_media_command(flags, command, 0.0, now))
            .await
            .map_err(|error| format!("sending the media command: {error}"))?;
        Ok(())
    }

    /// Wait until the parcel's media is on at least one face, and say how many.
    async fn painted(viewer: &Viewer) -> Result<usize, BodyError> {
        let deadline = tokio::time::Instant::now()
            .checked_add(WAIT)
            .ok_or("the deadline overflows")?;
        loop {
            let media = viewer.parcel_media().await?;
            if media.engine.is_some() && media.faces > 0 {
                return Ok(media.faces);
            }
            if tokio::time::Instant::now() > deadline {
                return Err(format!("the parcel media never reached a face: {media:?}").into());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// **A script plays the parcel's media only when allowed, on every face
    /// wearing its texture, and stops it**: the viewer knows the media the grid
    /// stated; a script's play changes nothing while scripted media is off (as
    /// it is by default); with it on, the same play starts the video engine and
    /// the plywood faces wear the media, and the Nearby Media window lists it as
    /// playing; the script's stop takes it off every face.
    #[test]
    fn a_script_plays_the_parcel_media_once_allowed_and_stops_it() -> Result<(), TestError> {
        stage("parcel_media_script").run(async |stage: &Stage| {
            let alpha = &stage.viewer("Alpha")?;
            state_media(stage, "Alpha").await?;
            let _known = alpha
                .expect_state(Probe::ParcelMedia)
                .at("/url")
                .timeout(WAIT)
                .to_equal(json!(MEDIA_URL))
                .await?;

            // Refused: scripted media is off by default.
            command(stage, "Alpha", PLAY_BIT, ParcelMediaCommand::Play).await?;
            tokio::time::sleep(REFUSED).await;
            let refused = alpha.parcel_media().await?;
            assert_eq!(refused.engine, None, "a script started the media unasked");
            assert_eq!(refused.faces, 0);

            // Allowed: the same play starts it on the plywood.
            let _allowed = alpha.set_setting(SCRIPTED_MEDIA, json!(true)).await?;
            command(stage, "Alpha", PLAY_BIT, ParcelMediaCommand::Play).await?;
            let _faces = painted(alpha).await?;
            let _video = alpha
                .expect_state(Probe::ParcelMedia)
                .at("/engine")
                .to_equal(json!("video"))
                .await?;

            // Listed in the Nearby Media window, under the stream's place.
            let _opened = alpha.ui().button("Nearby media").click().await?;
            let window = alpha.ui().window("nearby-media");
            let _row = alpha
                .expect(
                    &window
                        .get(Locator::role(Role::Text).named("Parcel Streaming Media (playing)")),
                )
                .timeout(WAIT)
                .to_be_visible()
                .await?;

            // Stopped: off every face.
            command(stage, "Alpha", STOP_BIT, ParcelMediaCommand::Stop).await?;
            let _stopped = alpha
                .expect_state(Probe::ParcelMedia)
                .at("/faces")
                .timeout(WAIT)
                .to_equal(json!(0))
                .await?;
            let _no_engine = alpha
                .expect_state(Probe::ParcelMedia)
                .at("/engine")
                .to_be_absent()
                .await?;
            Ok(())
        })?;
        Ok(())
    }
}
