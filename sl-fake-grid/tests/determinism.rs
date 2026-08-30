//! Two grids built from the same seed and the same content mint the same
//! identifiers — the property that makes an offline scenario's records
//! comparable run to run.

#[cfg(test)]
mod test {
    use pretty_assertions::{assert_eq, assert_ne};
    use sl_fake_grid::{AccountConfig, FakeGridBuilder, RegionConfig};
    use sl_types::key::AgentKey;

    type TestError = Box<dyn core::error::Error>;

    /// Build one seeded grid and collect its visible minted identifiers.
    async fn minted(seed: u64) -> Result<(AgentKey, uuid::Uuid), TestError> {
        let grid = FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .region(RegionConfig::default())
            .deterministic(seed)
            .http_port(0)
            .start()
            .await?;
        let agent = grid
            .account_agent_id("Test", "User")
            .ok_or("the account was not registered")?;
        let region = grid
            .region_names()
            .first()
            .and_then(|name| grid.region_id(name))
            .ok_or("the region was not registered")?;
        grid.shutdown();
        Ok((agent, region))
    }

    /// The same seed yields the same identifiers; a different seed does not.
    #[tokio::test]
    async fn the_same_seed_mints_the_same_identifiers() -> Result<(), TestError> {
        let first = minted(7).await?;
        let second = minted(7).await?;
        assert_eq!(first, second, "one seed, one identifier stream");
        let third = minted(8).await?;
        assert_ne!(first, third, "a different seed must not repeat the stream");
        Ok(())
    }
}
