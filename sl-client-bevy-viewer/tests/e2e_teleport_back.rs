//! Live check for [[viewer-teleport-back-gets-no-answer]]: world-map teleports
//! back and forth between two distant Second Life regions, staying a minute in
//! each, must all arrive.
//!
//! Before the fix, the third teleport (back to the region left first) got no
//! answer at this pace, and asking again did not help. It needs a live grid
//! (`SL_E2E_GRID=aditi`) and is skipped otherwise. The regions are aditi's
//! Mauve and Ahern; the stay is 60 s because 30 s never stalled.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use serde_json::json;
    use sl_automation_proto::{Locator, Probe, Role};
    use sl_e2e::{BodyError, Need, Stage, StageBuilder};
    use sl_viewer_driver::Viewer;

    /// The viewer binary cargo built for this test.
    const VIEWER: &str = env!("CARGO_BIN_EXE_sl-client-bevy-viewer");
    /// How long the world map may take to list a searched region.
    const SEARCH: Duration = Duration::from_secs(60);
    /// How long one teleport may take to arrive.
    const TELEPORT: Duration = Duration::from_secs(120);
    /// How long the agent stays in each region, the pace that stalled.
    const STAY: Duration = Duration::from_secs(60);
    /// Teleports made, alternating between the two regions.
    const HOPS: u32 = 8;

    /// Teleports `alpha` to `region` through the world map's search, and
    /// waits for the arrival.
    async fn teleport_by_map(alpha: &Viewer, region: &str) -> Result<(), BodyError> {
        let _opened = alpha
            .menu_path(&["menu-bar-world", "menu-bar-world-map"])
            .await?;
        let map = alpha.ui().window("worldmap");
        let _typed = map
            .test_id("worldmap:search")
            .role(Role::Textbox)
            .fill(region)
            .await?;
        map.test_id("worldmap:search")
            .role(Role::Textbox)
            .press("Enter")
            .await?;
        let _picked = map
            .get(Locator::role(Role::ListItem).named(region))
            .timeout(SEARCH)
            .click()
            .await?;
        let _asked = map
            .test_id("worldmap-button:teleport-selected")
            .click()
            .await?;
        let _arrived = alpha
            .expect_state(Probe::Agent)
            .at("/region/name")
            .timeout(TELEPORT)
            .to_equal(json!(region))
            .await?;
        let _closed = alpha
            .menu_path(&["menu-bar-world", "menu-bar-world-map"])
            .await?;
        Ok(())
    }

    #[test]
    fn teleports_back_and_forth_a_minute_apart_all_arrive()
    -> Result<(), Box<dyn core::error::Error>> {
        StageBuilder::new("teleport_back")
            .viewer_binary(VIEWER)
            .viewer("Alpha")
            .needs(Need::LiveGrid(
                "Second Life's answer to a teleport back to a region left a minute ago",
            ))
            .run(async |stage: &Stage| {
                let alpha = &stage.viewer("Alpha")?;
                let here = alpha.agent().await?;
                let (first, second) = if format!("{:?}", here.region).contains("Mauve") {
                    ("Ahern", "Mauve")
                } else {
                    ("Mauve", "Ahern")
                };
                for hop in 1..=HOPS {
                    let to = if hop % 2 == 1 { first } else { second };
                    teleport_by_map(alpha, to).await?;
                    tokio::time::sleep(STAY).await;
                }
                Ok(())
            })?;
        Ok(())
    }
}
