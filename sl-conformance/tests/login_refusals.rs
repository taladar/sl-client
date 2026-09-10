//! The login refusals: every reason a grid can decline a correctly-addressed
//! login, and what the client makes of each.
//!
//! [`tests/offline.rs`](offline) runs the registered conformance cases, and
//! every one of them starts from a login that **succeeded** — a
//! [`sl_conformance::context::TestContext`] is assembled out of live sessions,
//! so a case cannot be the one that asserts a login was refused. This file is
//! the other half: no registry, no context, one
//! [`sl_fake_grid::FakeGridBuilder`] built per case with exactly the gate under
//! test set, driving [`sl_client_tokio::Client::connect`] straight at it.
//!
//! The grid has served all of this since it was written and nothing has ever
//! asked for it. What is asserted is not that the grid refuses — `sl-wire`'s
//! own tests pin [`sl_wire::LoginServer`]'s decisions — but that the refusal
//! survives the XML-RPC round trip and reaches a **client** as a reason it can
//! act on: a `"tos"` the viewer can put a dialog in front of, a `"presence"` it
//! may retry, an MFA challenge it can answer.
//!
//! One grid per test, on its own ephemeral port, because each needs a
//! differently-built one and a gate is a property of the grid rather than of
//! the request.

#[cfg(test)]
mod test {
    use core::time::Duration;

    use pretty_assertions::assert_eq;
    use sl_client_tokio::{
        Client, Error, LoginParams, LoginRejectKind, LoginRequest, StartLocation,
    };
    use sl_fake_grid::{AccountConfig, FakeGrid, FakeGridBuilder, RegionConfig};
    use sl_wire::{LoginGates, LoginRedirect, MfaPolicy};

    /// A boxed error for terse test signatures.
    type TestError = Box<dyn std::error::Error>;

    /// The account every grid here registers.
    const FIRST_NAME: &str = "Refused";
    /// The account's last name.
    const LAST_NAME: &str = "Tester";
    /// The account's password.
    const PASSWORD: &str = "conformance";

    /// The viewer channel these logins report.
    const CHANNEL: &str = "sl-conformance-login-refusals";

    /// Starts a one-region grid whose single account is gated by `gates` and
    /// carries `mfa`.
    async fn grid_with(gates: LoginGates, mfa: Option<MfaPolicy>) -> Result<FakeGrid, TestError> {
        let account = AccountConfig {
            mfa,
            ..AccountConfig::new(FIRST_NAME, LAST_NAME, PASSWORD)
        };
        Ok(FakeGridBuilder::new()
            .account(account)
            .region(RegionConfig::default())
            .gates(gates)
            .start()
            .await?)
    }

    /// The login request the tests start from: the right name and password, and
    /// **neither** of the two acceptances set.
    ///
    /// [`LoginRequest::new`] leaves `agree_to_tos` and `read_critical` `true`,
    /// which is right for a driver logging into a grid it has already agreed
    /// with — and means the stock request sails through both gates. A viewer
    /// that has to *show* the ToS sends the first attempt without them, which is
    /// the attempt modelled here.
    fn unaccepting_request() -> LoginRequest {
        LoginRequest {
            agree_to_tos: false,
            read_critical: false,
            ..LoginRequest::new(
                FIRST_NAME,
                LAST_NAME,
                PASSWORD,
                StartLocation::Last,
                CHANNEL,
                "0.0",
            )
        }
    }

    /// Performs one login against `grid` with `request`.
    async fn attempt(grid: &FakeGrid, request: LoginRequest) -> Result<Client, Error> {
        Client::connect(LoginParams {
            login_uri: grid.login_uri(),
            request,
        })
        .await
    }

    /// The rejection `request` was refused with, or an error naming what
    /// happened instead.
    async fn refusal(grid: &FakeGrid, request: LoginRequest) -> Result<Error, TestError> {
        match attempt(grid, request).await {
            Ok(_client) => Err("the login succeeded; nothing refused it".into()),
            Err(error) => Ok(error),
        }
    }

    /// A wrong password is `"key"`, and the client classifies it as credentials
    /// that cannot be retried.
    #[tokio::test]
    async fn a_wrong_password_is_refused_as_bad_credentials() -> Result<(), TestError> {
        let grid = grid_with(LoginGates::default(), None).await?;
        let request = LoginRequest::new(
            FIRST_NAME,
            LAST_NAME,
            "not the password",
            StartLocation::Last,
            CHANNEL,
            "0.0",
        );
        let Error::LoginRejected { kind, reason, .. } = refusal(&grid, request).await? else {
            return Err("a wrong password is a rejection, not any other failure".into());
        };
        assert_eq!(kind, LoginRejectKind::BadCredentials);
        assert_eq!(reason, "key");
        Ok(())
    }

    /// An account the grid has never heard of is refused **exactly** as a wrong
    /// password is, so the endpoint does not tell a caller which names exist.
    #[tokio::test]
    async fn an_unknown_account_is_indistinguishable_from_a_wrong_password() -> Result<(), TestError>
    {
        let grid = grid_with(LoginGates::default(), None).await?;
        let unknown = LoginRequest::new(
            "Nobody",
            "Here",
            PASSWORD,
            StartLocation::Last,
            CHANNEL,
            "0.0",
        );
        let wrong_password = LoginRequest::new(
            FIRST_NAME,
            LAST_NAME,
            "not the password",
            StartLocation::Last,
            CHANNEL,
            "0.0",
        );
        let (Error::LoginRejected { kind, reason, .. }, Error::LoginRejected { kind: other, .. }) = (
            refusal(&grid, unknown).await?,
            refusal(&grid, wrong_password).await?,
        ) else {
            return Err("both attempts are rejections".into());
        };
        assert_eq!(kind, LoginRejectKind::BadCredentials);
        assert_eq!(reason, "key");
        assert_eq!(kind, other, "the two answers must not be told apart");
        Ok(())
    }

    /// A pending terms-of-service acceptance rejects with the ToS **text**, and
    /// the same login re-sent with `agree_to_tos` goes through.
    ///
    /// The text matters: it is what the viewer's ToS dialog displays, so a grid
    /// that refused without it would leave nothing to agree to.
    #[tokio::test]
    async fn a_pending_terms_of_service_is_refused_with_its_text() -> Result<(), TestError> {
        const TOS: &str = "<p>The terms of this grid.</p>";
        let grid = grid_with(
            LoginGates {
                tos_message: Some(TOS.to_owned()),
                ..LoginGates::default()
            },
            None,
        )
        .await?;
        let Error::LoginRejected {
            kind,
            reason,
            message,
        } = refusal(&grid, unaccepting_request()).await?
        else {
            return Err("a pending ToS is a rejection".into());
        };
        assert_eq!(kind, LoginRejectKind::Tos);
        assert_eq!(reason, "tos");
        assert_eq!(message, TOS, "the refusal carries the text to agree to");

        let accepted = LoginRequest {
            agree_to_tos: true,
            ..unaccepting_request()
        };
        attempt(&grid, accepted).await?;
        Ok(())
    }

    /// A critical message is the same shape one reason over: refused until the
    /// request acknowledges it, and the message is the notice to display.
    #[tokio::test]
    async fn a_critical_message_is_refused_until_it_is_acknowledged() -> Result<(), TestError> {
        const NOTICE: &str = "This grid has something to tell you.";
        let grid = grid_with(
            LoginGates {
                critical_message: Some(NOTICE.to_owned()),
                ..LoginGates::default()
            },
            None,
        )
        .await?;
        let Error::LoginRejected {
            kind,
            reason,
            message,
        } = refusal(&grid, unaccepting_request()).await?
        else {
            return Err("a pending critical message is a rejection".into());
        };
        assert_eq!(kind, LoginRejectKind::CriticalMessage);
        assert_eq!(reason, "critical");
        assert_eq!(message, NOTICE);

        let acknowledged = LoginRequest {
            read_critical: true,
            ..unaccepting_request()
        };
        attempt(&grid, acknowledged).await?;
        Ok(())
    }

    /// An account the grid believes is already online is refused as
    /// `"presence"` — and the client classifies it as the *retryable* one,
    /// which it does from the message rather than the reason code.
    #[tokio::test]
    async fn an_already_logged_in_account_is_refused_as_retryable() -> Result<(), TestError> {
        let grid = grid_with(
            LoginGates {
                already_logged_in: true,
                ..LoginGates::default()
            },
            None,
        )
        .await?;
        let Error::LoginRejected {
            kind,
            reason,
            message,
        } = refusal(&grid, unaccepting_request()).await?
        else {
            return Err("an already-logged-in account is a rejection".into());
        };
        assert_eq!(kind, LoginRejectKind::AlreadyLoggedIn);
        assert_eq!(reason, "presence");
        assert!(
            message.to_lowercase().contains("already logged in"),
            "the classifier reads the message, so the message has to say it: {message}"
        );
        Ok(())
    }

    /// An account with a multi-factor policy raises a challenge rather than a
    /// rejection, and the one-time code the challenge asks for answers it.
    #[tokio::test]
    async fn an_mfa_account_challenges_before_it_admits() -> Result<(), TestError> {
        const TOKEN: &str = "123456";
        const HASH: &str = "remember-this-device";
        const PROMPT: &str = "Enter the code from your authenticator.";
        let grid = grid_with(
            LoginGates::default(),
            Some(MfaPolicy {
                expected_token: TOKEN.to_owned(),
                mfa_hash: HASH.to_owned(),
                challenge_message: PROMPT.to_owned(),
            }),
        )
        .await?;
        let Error::MfaChallenge(challenge) = refusal(&grid, unaccepting_request()).await? else {
            return Err("an MFA account challenges rather than rejecting".into());
        };
        assert_eq!(challenge.message, PROMPT);
        assert_eq!(
            challenge.mfa_hash.as_deref(),
            Some(HASH),
            "the challenge hands out the hash a later login echoes to skip it"
        );

        // The code answers this login...
        attempt(
            &grid,
            unaccepting_request().with_mfa(TOKEN, challenge.mfa_hash.clone()),
        )
        .await?;
        // ...and the remembered hash alone answers the next one, with no code:
        // "remember this device", which is the whole point of the hash.
        attempt(
            &grid,
            unaccepting_request().with_mfa("", challenge.mfa_hash),
        )
        .await?;
        Ok(())
    }

    /// A wrong password behind an MFA policy is still a wrong password: the
    /// challenge must not be raised for a login that could never succeed, or it
    /// would tell an attacker the password was right.
    #[tokio::test]
    async fn a_wrong_password_is_refused_before_the_mfa_challenge() -> Result<(), TestError> {
        let grid = grid_with(
            LoginGates::default(),
            Some(MfaPolicy {
                expected_token: "123456".to_owned(),
                mfa_hash: "remember-this-device".to_owned(),
                challenge_message: "Enter the code.".to_owned(),
            }),
        )
        .await?;
        let request = LoginRequest::new(
            FIRST_NAME,
            LAST_NAME,
            "not the password",
            StartLocation::Last,
            CHANNEL,
            "0.0",
        );
        let Error::LoginRejected { kind, .. } = refusal(&grid, request).await? else {
            return Err("a wrong password is a rejection whatever else is configured".into());
        };
        assert_eq!(kind, LoginRejectKind::BadCredentials);
        Ok(())
    }

    /// A redirect (`login = "indeterminate"`) sends the same login to another
    /// endpoint, and the client re-POSTs it there without being asked twice for
    /// anything.
    ///
    /// Two grids: the first knows the account but redirects every login, the
    /// second is where the session actually comes from.
    #[tokio::test]
    async fn a_redirect_is_followed_to_the_grid_that_answers_it() -> Result<(), TestError> {
        let authoritative = grid_with(LoginGates::default(), None).await?;
        let front = grid_with(
            LoginGates {
                redirect: Some(LoginRedirect {
                    next_url: authoritative.login_uri(),
                    next_method: "login_to_simulator".to_owned(),
                    message: Some("Handing you over.".to_owned()),
                    next_options: Vec::new(),
                }),
                ..LoginGates::default()
            },
            None,
        )
        .await?;
        let mut logins = authoritative.logins();
        let client = attempt(&front, unaccepting_request()).await?;
        // The session is the second grid's: it is the one that saw the login.
        let notice = tokio::time::timeout(Duration::from_secs(10), logins.recv()).await??;
        assert_eq!(notice.first_name, FIRST_NAME);
        assert_eq!(client.agent_id(), Some(notice.agent_id));
        Ok(())
    }

    /// A grid that redirects to itself is a loop, and the client gives up on it
    /// rather than following it forever.
    ///
    /// A grid does not know its own login URI until it has started, and its
    /// gates are fixed before that — so the port is reserved first and handed to
    /// the builder, which is the only way to write a redirect that points back
    /// at the grid serving it.
    #[tokio::test]
    async fn a_redirect_loop_is_abandoned() -> Result<(), TestError> {
        let port = {
            let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let port = probe.local_addr()?.port();
            drop(probe);
            port
        };
        let own_uri: url::Url = format!("http://127.0.0.1:{port}/").parse()?;
        let grid = FakeGridBuilder::new()
            .account(AccountConfig::new(FIRST_NAME, LAST_NAME, PASSWORD))
            .region(RegionConfig::default())
            .gates(LoginGates {
                redirect: Some(LoginRedirect {
                    next_url: own_uri,
                    next_method: "login_to_simulator".to_owned(),
                    message: None,
                    next_options: Vec::new(),
                }),
                ..LoginGates::default()
            })
            .http_port(port)
            .start()
            .await?;
        let error = refusal(&grid, unaccepting_request()).await?;
        assert!(
            matches!(error, Error::TooManyLoginRedirects { .. }),
            "a redirect loop ends in the hop bound, not in {error}"
        );
        Ok(())
    }

    /// The **stale presence** the conformance runner retries past: the first
    /// login is refused as already-logged-in, that refusal evicts the ghost, and
    /// the retry gets in.
    ///
    /// This is the one case that goes through
    /// [`sl_conformance::context::login`] rather than
    /// [`sl_client_tokio::Client::connect`], because the thing under test is the
    /// runner's own retry branch — the only production code in this workspace
    /// that reacts to a [`LoginRejectKind::AlreadyLoggedIn`], and until now the
    /// only one nothing exercised. It is keyed on OpenSim (Second Life may read
    /// rapid repeats as suspicious), so the grid is addressed as
    /// [`sl_conformance::Grid::Opensim`] — which it is by every means the
    /// function has of telling: the avatar names the URI, and the URI is this
    /// grid.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stale_presence_is_refused_once_and_the_runner_retries_past_it()
    -> Result<(), TestError> {
        let grid = FakeGridBuilder::new()
            .account(AccountConfig::new(FIRST_NAME, LAST_NAME, PASSWORD))
            .region(RegionConfig::default())
            .stale_presence()
            .start()
            .await?;

        // The first attempt meets the ghost and is refused...
        let Error::LoginRejected { kind, .. } = refusal(&grid, unaccepting_request()).await? else {
            return Err("a stale presence is a rejection".into());
        };
        assert_eq!(kind, LoginRejectKind::AlreadyLoggedIn);
        // ...and evicts it, so the next one goes through.
        attempt(&grid, unaccepting_request()).await?;

        // Now the same thing through the runner, which does both halves itself.
        let grid = FakeGridBuilder::new()
            .account(AccountConfig::new(FIRST_NAME, LAST_NAME, PASSWORD))
            .region(RegionConfig::default())
            .stale_presence()
            .start()
            .await?;
        let credentials = sl_repl::Credentials::from_toml_str(&format!(
            "default_avatar = \"primary\"\n\n[avatars.primary]\nfirst = \"{FIRST_NAME}\"\n\
             last = \"{LAST_NAME}\"\npassword = \"{PASSWORD}\"\nlogin_uri = \"{}\"\n",
            grid.login_uri()
        ))?;
        let avatar = credentials.select(Some("primary"))?;
        let session = sl_conformance::context::login(
            sl_conformance::Grid::Opensim,
            avatar,
            CHANNEL,
            "0.0",
            "last",
            &std::env::temp_dir(),
            false,
            None,
        )
        .await?;
        session.logout().await?;
        Ok(())
    }
}
