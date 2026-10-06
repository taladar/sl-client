//! How each grid refuses a login, and what a second login of an avatar that is
//! already in world does to the first.
//!
//! Every other case starts from a login that succeeded. This one keeps that
//! session and, beside it, makes the attempts a grid declines
//! ([`Session::attempt_login`], one request each, never retried), recording the
//! whole answer — reason code, text, the localisation key and its arguments,
//! and the name of every field the response carried:
//!
//! 1. the avatar's own name with a **wrong password** (`wrong_password_*`);
//! 2. a **name the grid has never heard of** (`unknown_account_*`), and
//!    whether the two answers can be told apart;
//! 3. the avatar's own name and password **while it is in world**
//!    (`second_login_*`): admitted or refused, and what became of the first
//!    session (`first_session_after`) — still there, kicked, or dropped.
//!
//! On a grid that asks for a one-time code the third attempt is sent bare
//! first, so the **challenge** itself is recorded (`mfa_challenge_*`), then
//! with only the `mfa_hash` the avatar's own login was answered with
//! (`mfa_hash_reused`: whether that alone gets past the challenge), and with a
//! fresh code only if it does not.
//!
//! On aditi every attempt in the avatar's name waits out the login cooldown,
//! and one wrong password per run is all the case ever sends, so it takes
//! about ten minutes there. `1av`, `[both, fake]`.
//!
//! The measured answers are in the book's *Grid Behaviour* part,
//! `book/src/gridspec/login.md` § Refusals.

use std::time::Duration;

use sl_client_tokio::{Event, LoginFailure, LoginRejectKind, MfaChallenge};

use crate::context::{LoginAnswer, LoginAttempt, Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq};

/// The overall budget: up to four logins behind aditi's 120 s cooldown.
const CASE_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// How long the first session is watched for a kick after the second login
/// was answered.
const FIRST_SESSION_WATCH: Duration = Duration::from_secs(20);

/// The password sent with the avatar's name in the wrong-password attempt,
/// and with [`UNKNOWN_ACCOUNT`].
const WRONG_PASSWORD: &str = "sl-conformance login-refusals: not the password";

/// A first and last name no grid has an account for.
const UNKNOWN_ACCOUNT: (&str, &str) = ("SlConformanceNobodyQx7Zk", "Resident");

/// Where the answers below are written down.
const SOURCE: &str = "book/src/gridspec/login.md (login-refusals, 2026-10-06)";

/// The text of the refusal for a wrong password — and, word for word, for an
/// unknown account.
const BAD_CREDENTIALS_MESSAGE: Measured<&str> = Measured {
    second_life: "Sorry! We couldn't log you in.\n\nPlease check to make sure you entered the \
        right\n\n    * Username (like bobsmith12 or steller.sunshine)\n\n    * Password\n\n    \
        * Second Factor Token (if enabled)\n\nAlso, please make sure your Caps Lock key is off. \
        If you feel this is an error, please contact support@secondlife.com.",
    opensim: "Could not authenticate your avatar. Please check your username and password, and \
        check the grid if problems persist.",
    source: SOURCE,
};

/// The localisation key sent with it; empty where the grid sends none.
const BAD_CREDENTIALS_MESSAGE_ID: Measured<&str> = Measured {
    second_life: "LoginFailedAuthenticationFailed",
    opensim: "",
    source: SOURCE,
};

/// Every field that refusal carries.
const BAD_CREDENTIALS_FIELDS: Measured<&str> = Measured {
    second_life: "Linden_Error_Code,login,message,message_args,message_id,reason",
    opensim: "login,message,reason",
    source: SOURCE,
};

/// Whether a login of an avatar that is in world gets in.
const SECOND_LOGIN_ADMITTED: Measured<bool> = Measured {
    second_life: true,
    opensim: false,
    source: SOURCE,
};

/// The text the second login is refused with, where it is.
const SECOND_LOGIN_REFUSAL: Measured<Option<&str>> = Measured {
    second_life: None,
    opensim: Some(
        "You appear to be already logged in. Please wait a a minute or two and retry. If this \
         takes longer than a few minutes please contact the grid owner. ",
    ),
    source: SOURCE,
};

/// What the second login does to the session the avatar already had: both
/// grids kick it, each in its own words.
const FIRST_SESSION_FATE: Measured<&str> = Measured {
    second_life: "kicked: The system has logged you out because you are attempting to log in \
        from another location.",
    opensim: "kicked: New login detected",
    source: SOURCE,
};

/// What the case observed, as `(metric, value)` pairs — collected rather than
/// written straight into the context so a run that fails half way still
/// records what it had seen.
type Notes = Vec<(String, String)>;

/// Add one observation.
fn note(notes: &mut Notes, key: &str, value: impl Into<String>) {
    notes.push((key.to_owned(), value.into()));
}

/// Record a refusal under `prefix`.
fn note_refusal(notes: &mut Notes, prefix: &str, kind: LoginRejectKind, failure: &LoginFailure) {
    note(notes, &format!("{prefix}_answer"), "refused");
    note(notes, &format!("{prefix}_kind"), format!("{kind:?}"));
    note(notes, &format!("{prefix}_reason"), failure.reason.clone());
    note(notes, &format!("{prefix}_message"), failure.message.clone());
    note(
        notes,
        &format!("{prefix}_message_id"),
        failure.message_id.clone().unwrap_or_default(),
    );
    note(
        notes,
        &format!("{prefix}_message_args"),
        failure
            .message_args
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(","),
    );
    // The incident id differs on every response; its length is its shape.
    note(
        notes,
        &format!("{prefix}_error_code_length"),
        failure
            .error_code
            .as_ref()
            .map_or_else(|| "absent".to_owned(), |code| code.len().to_string()),
    );
    note(
        notes,
        &format!("{prefix}_fields"),
        failure.response_fields.join(","),
    );
}

/// Hold a wrong-password or unknown-account refusal to the measured answer.
fn check_bad_credentials(
    grid: Grid,
    what: &str,
    failure: Option<&LoginFailure>,
) -> Result<(), TestFailure> {
    let failure = failure.ok_or_else(|| {
        TestFailure::Assertion(format!("the {what} login was challenged, not refused"))
    })?;
    check_eq(
        &format!("the {what} reason"),
        &failure.reason.as_str(),
        &"key",
    )?;
    BAD_CREDENTIALS_MESSAGE.check(&format!("the {what} text"), grid, &failure.message.as_str())?;
    BAD_CREDENTIALS_MESSAGE_ID.check(
        &format!("the {what} message id"),
        grid,
        &failure.message_id.as_deref().unwrap_or_default(),
    )?;
    BAD_CREDENTIALS_FIELDS.check(
        &format!("the {what} fields"),
        grid,
        &failure.response_fields.join(",").as_str(),
    )
}

/// Record a challenge under `prefix`. The hash itself is a credential, so only
/// its presence and length are kept.
fn note_challenge(notes: &mut Notes, prefix: &str, challenge: &MfaChallenge) {
    note(
        notes,
        &format!("{prefix}_message"),
        challenge.message.clone(),
    );
    note(
        notes,
        &format!("{prefix}_hash_length"),
        challenge
            .mfa_hash
            .as_ref()
            .map_or_else(|| "absent".to_owned(), |hash| hash.len().to_string()),
    );
    note(
        notes,
        &format!("{prefix}_fields"),
        challenge.response_fields.join(","),
    );
}

/// Make an attempt that must not get in, and record how it was answered. An
/// attempt that *is* admitted is logged out again and fails the case.
async fn refused_attempt(
    session: &Session,
    notes: &mut Notes,
    prefix: &str,
    attempt: &LoginAttempt,
) -> Result<Option<LoginFailure>, TestFailure> {
    match session.attempt_login(attempt).await? {
        LoginAnswer::Refused { kind, failure } => {
            note_refusal(notes, prefix, kind, &failure);
            Ok(Some(failure))
        }
        LoginAnswer::Challenged(challenge) => {
            // A grid that asks for a code before it has checked the password.
            note(notes, &format!("{prefix}_answer"), "challenged");
            note_challenge(notes, &format!("{prefix}_challenge"), &challenge);
            Ok(None)
        }
        LoginAnswer::Admitted(admitted) => {
            note(notes, &format!("{prefix}_answer"), "admitted");
            admitted.logout().await?;
            Err(TestFailure::Assertion(format!(
                "the {prefix} login was admitted"
            )))
        }
    }
}

/// Log the avatar in a second time while `session` is in world, answering a
/// challenge the way a viewer would, and return the final answer.
async fn second_login(session: &Session, notes: &mut Notes) -> Result<LoginAnswer, TestFailure> {
    let remembered = session
        .login_success()
        .and_then(|success| success.mfa_hash.clone())
        .filter(|hash| !hash.is_empty());
    note(
        notes,
        "success_mfa_hash_length",
        remembered
            .as_ref()
            .map_or_else(|| "absent".to_owned(), |hash| hash.len().to_string()),
    );
    let mut answer = session.attempt_login(&LoginAttempt::default()).await?;
    let LoginAnswer::Challenged(first) = &answer else {
        note(notes, "mfa_challenged", "false");
        return Ok(answer);
    };
    note(notes, "mfa_challenged", "true");
    note_challenge(notes, "mfa_challenge", first);
    note(
        notes,
        "mfa_challenge_hash_is_the_remembered_one",
        (first.mfa_hash.is_some() && first.mfa_hash == remembered).to_string(),
    );
    let mut challenge = first.clone();
    if let Some(hash) = remembered {
        // "Remember this device": the hash a login was answered with, sent
        // back with no code.
        answer = session
            .attempt_login(&LoginAttempt {
                mfa: Some((String::new(), Some(hash))),
                answers_challenge: true,
                ..LoginAttempt::default()
            })
            .await?;
        match &answer {
            LoginAnswer::Challenged(again) => {
                note(notes, "mfa_hash_reused", "false");
                challenge = again.clone();
            }
            LoginAnswer::Admitted(_) | LoginAnswer::Refused { .. } => {
                note(notes, "mfa_hash_reused", "true");
                return Ok(answer);
            }
        }
    }
    let token = session.acquire_mfa_token()?;
    session
        .attempt_login(&LoginAttempt {
            mfa: Some((token, challenge.mfa_hash)),
            answers_challenge: true,
            ..LoginAttempt::default()
        })
        .await
}

/// Watch the first session for what the second login did to it.
async fn first_session_fate(session: &mut Session) -> String {
    let watched = session
        .wait_for(FIRST_SESSION_WATCH, |event| match event {
            Event::Kicked(kick) => Some(format!("kicked: {}", kick.reason)),
            Event::Disconnected(reason) => Some(format!("disconnected: {reason:?}")),
            _other => None,
        })
        .await;
    match watched {
        Ok(fate) => fate,
        Err(TestFailure::Timeout(_)) => "still in world".to_owned(),
        Err(other) => format!("closed: {other}"),
    }
}

/// The attempts, in order, against the logged-in `session`.
async fn probe(grid: Grid, session: &mut Session, notes: &mut Notes) -> Result<(), TestFailure> {
    session.wait_for_region(REGION_TIMEOUT).await?;

    let wrong_password = refused_attempt(
        session,
        notes,
        "wrong_password",
        &LoginAttempt {
            password: Some(WRONG_PASSWORD.to_owned()),
            ..LoginAttempt::default()
        },
    )
    .await?;
    let unknown_account = refused_attempt(
        session,
        notes,
        "unknown_account",
        &LoginAttempt {
            name: Some((UNKNOWN_ACCOUNT.0.to_owned(), UNKNOWN_ACCOUNT.1.to_owned())),
            password: Some(WRONG_PASSWORD.to_owned()),
            ..LoginAttempt::default()
        },
    )
    .await?;
    // But for the incident id, which is never the same twice.
    let without_code = |failure: &Option<LoginFailure>| {
        failure.clone().map(|failure| LoginFailure {
            error_code: None,
            ..failure
        })
    };
    let indistinguishable = without_code(&wrong_password) == without_code(&unknown_account);
    note(
        notes,
        "unknown_account_same_as_wrong_password",
        indistinguishable.to_string(),
    );
    check_bad_credentials(grid, "wrong-password", wrong_password.as_ref())?;
    check_bad_credentials(grid, "unknown-account", unknown_account.as_ref())?;
    check(
        indistinguishable,
        "an unknown account is answered differently from a wrong password",
    )?;

    let second = second_login(session, notes).await?;
    let (admitted, refusal) = match second {
        LoginAnswer::Admitted(mut admitted) => {
            note(notes, "second_login_answer", "admitted");
            admitted.wait_for_region(REGION_TIMEOUT).await?;
            (Some(admitted), None)
        }
        LoginAnswer::Refused { kind, failure } => {
            note_refusal(notes, "second_login", kind, &failure);
            (None, Some((kind, failure)))
        }
        LoginAnswer::Challenged(challenge) => {
            return Err(TestFailure::Assertion(format!(
                "the second login was challenged again after its code: {}",
                challenge.message
            )));
        }
    };
    let fate = first_session_fate(session).await;
    let first_gone = fate != "still in world";
    note(notes, "first_session_after", fate.clone());
    let was_admitted = admitted.is_some();
    if let Some(admitted) = admitted {
        admitted.logout().await?;
    }
    if first_gone {
        // Leave the runner a live session to log out — and, after a refusal,
        // learn whether the refused attempt cleared the way for the next one.
        session.disconnect().await?;
        session.relogin().await?;
        session.wait_for_region(REGION_TIMEOUT).await?;
        note(notes, "relogin_after_second_login", "admitted");
    }
    SECOND_LOGIN_ADMITTED.check(
        "whether a second login of an avatar in world gets in",
        grid,
        &was_admitted,
    )?;
    SECOND_LOGIN_REFUSAL.check(
        "the text the second login is refused with",
        grid,
        &refusal
            .as_ref()
            .map(|(_kind, failure)| failure.message.as_str()),
    )?;
    if let Some((kind, failure)) = &refusal {
        check_eq(
            "the refusal's reason",
            &failure.reason.as_str(),
            &"presence",
        )?;
        // The client offers a retry on this classification, and the retry is
        // what gets in.
        check_eq(
            "how the client classifies the refusal",
            kind,
            &LoginRejectKind::AlreadyLoggedIn,
        )?;
    }
    FIRST_SESSION_FATE.check(
        "what the second login does to the first session",
        grid,
        &fate.as_str(),
    )
}

/// Records how a grid refuses a login and what a second login does to the
/// first.
#[derive(Debug)]
pub struct LoginRefusals;

impl GridTest for LoginRefusals {
    fn name(&self) -> &'static str {
        "login-refusals"
    }

    fn description(&self) -> &'static str {
        "Record a wrong password, an unknown account and a second login of one avatar"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn timeout(&self) -> Duration {
        CASE_TIMEOUT
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut notes = Notes::new();
            let grid = ctx.grid();
            let outcome = probe(grid, ctx.primary(), &mut notes).await;
            let metrics = ctx.metrics();
            for (key, value) in notes {
                metrics.set(&key, value);
            }
            outcome
        })
    }
}
