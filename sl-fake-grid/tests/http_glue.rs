//! HTTP-glue tests: drive the fake grid's endpoints with a bare HTTP
//! client and the client-direction `sl-wire` codecs — no `sl-client-tokio`
//! involved.

#[cfg(test)]
mod test {
    use std::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_fake_grid::{AccountConfig, FakeGrid, FakeGridBuilder, RegionConfig};
    use sl_wire::{
        LoginRequest, LoginResponse, StartLocation, build_event_queue_request, build_login_request,
        build_login_request_llsd, build_seed_request, parse_event_queue_response,
        parse_login_response, parse_login_response_llsd, parse_seed_response,
    };

    /// A boxed error for terse test signatures.
    type TestError = Box<dyn std::error::Error>;

    /// Starts a one-account grid on ephemeral ports.
    async fn start_grid() -> Result<FakeGrid, TestError> {
        Ok(FakeGridBuilder::new()
            .account(AccountConfig::new("Test", "User", "password"))
            .region(RegionConfig::default())
            .event_queue_hold(Duration::from_millis(200))
            .start()
            .await?)
    }

    /// A login request for the stock account with the given password.
    fn login_request(password: &str) -> LoginRequest {
        LoginRequest::new(
            "Test",
            "User",
            password,
            StartLocation::Last,
            "sl-fake-grid-test",
            "0.0",
        )
    }

    /// Posts a login body and returns the response text.
    async fn post_login(
        grid: &FakeGrid,
        content_type: &str,
        body: String,
    ) -> Result<String, TestError> {
        let response = reqwest::Client::new()
            .post(grid.login_uri())
            .header("Content-Type", content_type)
            .body(body)
            .send()
            .await?;
        assert_eq!(response.status().as_u16(), 200);
        Ok(response.text().await?)
    }

    #[tokio::test]
    async fn xml_rpc_login_succeeds_and_bad_password_fails() -> Result<(), TestError> {
        let grid = start_grid().await?;

        let text = post_login(
            &grid,
            "text/xml",
            build_login_request(&login_request("password")),
        )
        .await?;
        let LoginResponse::Success(success) = parse_login_response(&text)? else {
            return Err("expected a successful login".into());
        };
        assert_eq!(
            Some(success.agent_id),
            grid.account_agent_id("Test", "User")
        );
        assert_eq!(success.first_name.as_deref(), Some("Test"));
        assert!(!success.inventory_skeleton.is_empty());
        assert!(success.inventory_root.is_some());
        assert_eq!(success.sim_ip, std::net::Ipv4Addr::LOCALHOST);
        // The stock scenario speaks WebRTC voice; the login says so.
        assert_eq!(
            success
                .voice_config
                .as_ref()
                .map(|voice| voice.voice_server_type.as_str()),
            Some("webrtc")
        );

        let text = post_login(
            &grid,
            "text/xml",
            build_login_request(&login_request("wrong")),
        )
        .await?;
        let LoginResponse::Failure(failure) = parse_login_response(&text)? else {
            return Err("expected a failed login".into());
        };
        assert_eq!(failure.reason, "key");
        Ok(())
    }

    #[tokio::test]
    async fn llsd_login_works_on_the_same_url() -> Result<(), TestError> {
        let grid = start_grid().await?;
        let text = post_login(
            &grid,
            "application/llsd+xml",
            build_login_request_llsd(&login_request("password")),
        )
        .await?;
        let LoginResponse::Success(success) = parse_login_response_llsd(&text)? else {
            return Err("expected a successful LLSD login".into());
        };
        assert_eq!(success.last_name.as_deref(), Some("User"));
        Ok(())
    }

    #[tokio::test]
    async fn seed_grant_is_idempotent() -> Result<(), TestError> {
        let grid = start_grid().await?;
        let text = post_login(
            &grid,
            "text/xml",
            build_login_request(&login_request("password")),
        )
        .await?;
        let LoginResponse::Success(success) = parse_login_response(&text)? else {
            return Err("expected a successful login".into());
        };

        let http = reqwest::Client::new();
        let seed_body = build_seed_request(&["EventQueueGet", "SimulatorFeatures"]);
        let mut replies = Vec::new();
        for _attempt in 0_u8..3 {
            let reply = http
                .post(success.seed_capability.clone())
                .header("Content-Type", "application/llsd+xml")
                .body(seed_body.clone())
                .send()
                .await?;
            assert_eq!(reply.status().as_u16(), 200);
            replies.push(reply.text().await?);
        }
        let first = replies.first().cloned().unwrap_or_default();
        assert!(replies.iter().all(|reply| *reply == first));
        let granted = parse_seed_response(&first)?;
        assert!(granted.contains_key("EventQueueGet"));
        assert!(granted.contains_key("SimulatorFeatures"));
        Ok(())
    }

    #[tokio::test]
    async fn event_queue_delivers_batches_and_times_out_as_502() -> Result<(), TestError> {
        let grid = start_grid().await?;
        let mut logins = grid.logins();
        let text = post_login(
            &grid,
            "text/xml",
            build_login_request(&login_request("password")),
        )
        .await?;
        let LoginResponse::Success(success) = parse_login_response(&text)? else {
            return Err("expected a successful login".into());
        };
        let notice = logins.recv().await?;
        let agent = grid.agent(&notice).await.ok_or("no live session")?;

        let http = reqwest::Client::new();
        let seed_reply = http
            .post(success.seed_capability.clone())
            .header("Content-Type", "application/llsd+xml")
            .body(build_seed_request(&["EventQueueGet"]))
            .send()
            .await?
            .text()
            .await?;
        let granted = parse_seed_response(&seed_reply)?;
        let eq_url = granted
            .get("EventQueueGet")
            .cloned()
            .ok_or("EventQueueGet not granted")?;

        // Empty queue: the held poll expires as the 502 re-poll answer.
        let empty = http
            .post(&eq_url)
            .header("Content-Type", "application/llsd+xml")
            .body(build_event_queue_request(None, false))
            .send()
            .await?;
        assert_eq!(empty.status().as_u16(), 502);

        // Enqueue grid-side, then the poll answers the batch.
        agent
            .with_sim(|sim| {
                sim.enqueue_caps_event(
                    "FakeGridTestEvent",
                    sl_proto::Llsd::String("ping".to_owned()),
                );
            })
            .await;
        let batch = http
            .post(&eq_url)
            .header("Content-Type", "application/llsd+xml")
            .body(build_event_queue_request(None, false))
            .send()
            .await?;
        assert_eq!(batch.status().as_u16(), 200);
        let parsed = parse_event_queue_response(&batch.text().await?)?;
        assert_eq!(parsed.events.len(), 1);

        // A held poll is woken by a fresh enqueue instead of timing out.
        let woken = tokio::spawn({
            let http = http.clone();
            let eq_url = eq_url.clone();
            let body = build_event_queue_request(Some(parsed.id), false);
            async move {
                http.post(&eq_url)
                    .header("Content-Type", "application/llsd+xml")
                    .body(body)
                    .send()
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        agent
            .with_sim(|sim| {
                sim.enqueue_caps_event(
                    "FakeGridTestEvent",
                    sl_proto::Llsd::String("pong".to_owned()),
                );
            })
            .await;
        let woken = woken.await??;
        assert_eq!(woken.status().as_u16(), 200);

        // A done poll tears the queue down; later polls answer 404.
        let done = http
            .post(&eq_url)
            .header("Content-Type", "application/llsd+xml")
            .body(build_event_queue_request(None, true))
            .send()
            .await?;
        assert_eq!(done.status().as_u16(), 200);
        let after = http
            .post(&eq_url)
            .header("Content-Type", "application/llsd+xml")
            .body(build_event_queue_request(None, false))
            .send()
            .await?;
        assert_eq!(after.status().as_u16(), 404);
        Ok(())
    }
}
