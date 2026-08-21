//! The non-CAPS HTTP surfaces: `get_grid_info` (both forms), the world-map
//! tiles, and the economy helper scripts — driven with a bare HTTP client
//! and the client-direction `sl-wire` codecs.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_fake_grid::{
        AccountConfig, EconomyConfig, EconomyEvent, FakeGrid, FakeGridBuilder, GridIdentity,
        RegionConfig, STOCK_TILE_JPEG,
    };
    use sl_wire::{
        BuyCurrencyRequest, CURRENCY_HELPER_PATH, CurrencyQuoteRequest, GRID_INFO_PATH,
        HelperOutcome, LAND_TOOL_HELPER_PATH, LandPrepRequest, LoginRequest, LoginResponse,
        MapTileRef, StartLocation, ViewerVersionInfo, XmlRpcResponse, build_buy_currency_request,
        build_buy_land_prep_request, build_currency_quote_request, build_login_request,
        build_method_call, build_preflight_land_prep_request, parse_buy_currency_response,
        parse_buy_land_prep_response, parse_currency_quote_response, parse_grid_info_xml,
        parse_grid_info_xmlrpc_response, parse_login_response, parse_method_response,
        parse_preflight_land_prep_response,
    };
    use uuid::Uuid;

    /// A boxed error for terse test signatures.
    type TestError = Box<dyn std::error::Error>;

    /// Starts a one-account, one-region grid with a named identity.
    async fn start_grid(economy: EconomyConfig) -> Result<FakeGrid, TestError> {
        Ok(FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .region(RegionConfig::default())
            .grid_identity(GridIdentity {
                name: "Loopback & Co".to_owned(),
                nick: "loopback".to_owned(),
                ..GridIdentity::default()
            })
            .economy(economy)
            .start()
            .await?)
    }

    /// POSTs an XML-RPC body to a path under the login URI.
    async fn post_xml(grid: &FakeGrid, path: &str, body: String) -> Result<String, TestError> {
        let response = reqwest::Client::new()
            .post(grid.login_uri().join(path)?)
            .header("Content-Type", "text/xml")
            .body(body)
            .send()
            .await?;
        assert_eq!(response.status().as_u16(), 200);
        Ok(response.text().await?)
    }

    #[tokio::test]
    async fn grid_info_is_served_as_xml_and_xml_rpc() -> Result<(), TestError> {
        let grid = start_grid(EconomyConfig::default()).await?;

        let response = reqwest::get(grid.login_uri().join(GRID_INFO_PATH)?).await?;
        assert_eq!(response.status().as_u16(), 200);
        let info = parse_grid_info_xml(&response.text().await?)?;
        assert_eq!(info.login_uri(), Some(grid.login_uri()));
        assert_eq!(info.grid_name(), Some("Loopback & Co"));
        assert_eq!(info.grid_nick(), Some("loopback"));
        assert_eq!(info.platform(), Some("OpenSim"));
        assert_eq!(info.helper_uri(), Some(grid.login_uri()));
        assert_eq!(&info, grid.grid_info());

        let text = post_xml(&grid, "", build_method_call("get_grid_info", &[])).await?;
        let via_rpc = parse_grid_info_xmlrpc_response(&text)?;
        assert_eq!(via_rpc.grid_name(), Some("Loopback & Co"));
        assert_eq!(via_rpc.login_uri(), Some(grid.login_uri()));

        // The login still works on the same URL, and advertises the tile base.
        let text = post_xml(
            &grid,
            "",
            build_login_request(&LoginRequest::new(
                "Test",
                "User",
                "password",
                StartLocation::Last,
                "sl-fake-grid-test",
                "0.0",
            )),
        )
        .await?;
        let LoginResponse::Success(success) = parse_login_response(&text)? else {
            return Err("expected a successful login".into());
        };
        assert_eq!(success.map_server_url, Some(grid.login_uri()));
        assert_eq!(success.currency.as_deref(), Some("L$"));

        let response = reqwest::Client::new()
            .post(grid.login_uri().join(GRID_INFO_PATH)?)
            .send()
            .await?;
        assert_eq!(response.status().as_u16(), 405);
        Ok(())
    }

    #[tokio::test]
    async fn map_tiles_are_served_for_regions() -> Result<(), TestError> {
        let grid = FakeGridBuilder::new()
            .region(RegionConfig::default())
            .region(RegionConfig {
                name: "East".to_owned(),
                grid_x: 1001,
                ..RegionConfig::default()
            })
            .map_tile(
                MapTileRef::new(2, 1000, 1000).ok_or("bad zoom")?,
                &b"not really a jpeg"[..],
            )
            .start()
            .await?;
        let base = grid.login_uri();

        let tile = MapTileRef::new(1, 1001, 1000).ok_or("bad zoom")?;
        let response = reqwest::get(base.join(&tile.file_name())?).await?;
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("image/jpeg")
        );
        assert!(
            response
                .headers()
                .get("cache-control")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("max-age"))
        );
        assert_eq!(response.bytes().await?.as_ref(), STOCK_TILE_JPEG);

        let custom = MapTileRef::new(2, 1000, 1000).ok_or("bad zoom")?;
        let response = reqwest::get(base.join(&custom.file_name())?).await?;
        assert_eq!(response.bytes().await?.as_ref(), b"not really a jpeg");

        let absent = MapTileRef::new(1, 999, 999).ok_or("bad zoom")?;
        let response = reqwest::get(base.join(&absent.file_name())?).await?;
        assert_eq!(response.status().as_u16(), 404);

        let response = reqwest::Client::new()
            .head(base.join(&tile.file_name())?)
            .send()
            .await?;
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(
            response
                .headers()
                .get("content-length")
                .and_then(|v| v.to_str().ok()),
            Some(STOCK_TILE_JPEG.len().to_string().as_str())
        );
        Ok(())
    }

    fn viewer() -> ViewerVersionInfo {
        ViewerVersionInfo {
            channel: "sl-fake-grid-test".to_owned(),
            major: 1,
            minor: 0,
            patch: 0,
            build: "1".to_owned(),
        }
    }

    #[tokio::test]
    async fn currency_quote_then_buy_publishes_an_event() -> Result<(), TestError> {
        let grid = start_grid(EconomyConfig::default()).await?;
        let mut events = grid.economy_events();
        let agent = Uuid::from_u128(0xA1);

        let quote_request = CurrencyQuoteRequest {
            agent_id: agent,
            secure_session_id: Uuid::from_u128(0x5E),
            language: "en".to_owned(),
            currency_buy: 1000,
            viewer: viewer(),
        };
        let text = post_xml(
            &grid,
            CURRENCY_HELPER_PATH,
            build_currency_quote_request(&quote_request),
        )
        .await?;
        let HelperOutcome::Ok(quote) = parse_currency_quote_response(&text)? else {
            return Err("expected a quote".into());
        };
        assert_eq!(quote.currency_buy, 1000);
        assert_eq!(quote.estimated_cost, Some(250));
        assert_eq!(quote.estimated_local_cost.as_deref(), Some("US$ 2.50"));

        let mut buy = BuyCurrencyRequest {
            agent_id: agent,
            secure_session_id: Uuid::from_u128(0x5E),
            language: "en".to_owned(),
            currency_buy: 1000,
            confirm: "forged".to_owned(),
            estimated_cost: quote.estimated_cost,
            estimated_local_cost: quote.estimated_local_cost.clone(),
            password: None,
            viewer: viewer(),
        };
        let text = post_xml(
            &grid,
            CURRENCY_HELPER_PATH,
            build_buy_currency_request(&buy),
        )
        .await?;
        let HelperOutcome::Failed(failure) = parse_buy_currency_response(&text)? else {
            return Err("a forged confirm must fail".into());
        };
        assert!(failure.error_message.contains("confirmation"));

        buy.confirm = quote.confirm;
        let text = post_xml(
            &grid,
            CURRENCY_HELPER_PATH,
            build_buy_currency_request(&buy),
        )
        .await?;
        assert_eq!(parse_buy_currency_response(&text)?, HelperOutcome::Ok(()));
        assert_eq!(
            events.try_recv()?,
            EconomyEvent::CurrencyBought {
                agent_id: agent.into(),
                amount: 1000,
            }
        );

        // A method the script does not implement is an XML-RPC fault.
        let text = post_xml(
            &grid,
            CURRENCY_HELPER_PATH,
            build_method_call("preflightBuyLandPrep", &[]),
        )
        .await?;
        assert!(matches!(
            parse_method_response(&text)?,
            XmlRpcResponse::Fault { .. }
        ));
        Ok(())
    }

    #[tokio::test]
    async fn land_preflight_then_buy_and_a_down_site() -> Result<(), TestError> {
        let grid = start_grid(EconomyConfig {
            membership_upgrade: true,
            ..EconomyConfig::default()
        })
        .await?;
        let mut events = grid.economy_events();
        let agent = Uuid::from_u128(0xA2);
        let preflight = LandPrepRequest {
            agent_id: agent,
            secure_session_id: Uuid::from_u128(0x5E),
            language: "en".to_owned(),
            billable_area: 512,
            currency_buy: 400,
            level_id: None,
            estimated_cost: None,
            estimated_local_cost: None,
            confirm: None,
            password: None,
        };
        let text = post_xml(
            &grid,
            LAND_TOOL_HELPER_PATH,
            build_preflight_land_prep_request(&preflight),
        )
        .await?;
        let HelperOutcome::Ok(prep) = parse_preflight_land_prep_response(&text)? else {
            return Err("expected a preflight".into());
        };
        assert!(prep.membership.upgrade);
        assert_eq!(prep.membership.levels.len(), 1);
        assert!(!prep.land_use.upgrade);
        assert_eq!(prep.estimated_cost, Some(100));

        let commit = LandPrepRequest {
            level_id: prep.membership.levels.first().map(|level| level.id.clone()),
            estimated_cost: prep.estimated_cost,
            confirm: Some(prep.confirm),
            ..preflight
        };
        let text = post_xml(
            &grid,
            LAND_TOOL_HELPER_PATH,
            build_buy_land_prep_request(&commit),
        )
        .await?;
        assert_eq!(parse_buy_land_prep_response(&text)?, HelperOutcome::Ok(()));
        assert_eq!(
            events.try_recv()?,
            EconomyEvent::LandPrepared {
                agent_id: agent.into(),
                billable_area: 512,
                currency_buy: 400,
            }
        );

        let down = start_grid(EconomyConfig {
            site_valid: false,
            ..EconomyConfig::default()
        })
        .await?;
        let text = post_xml(
            &down,
            LAND_TOOL_HELPER_PATH,
            build_preflight_land_prep_request(&commit),
        )
        .await?;
        assert!(matches!(
            parse_preflight_land_prep_response(&text)?,
            HelperOutcome::Failed(_)
        ));
        Ok(())
    }
}
