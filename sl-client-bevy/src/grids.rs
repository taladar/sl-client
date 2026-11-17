//! The grids a viewer logs in to: fetching a grid's `get_grid_info` document,
//! what a grid manager asks a login URI before it knows the grid's name.
//!
//! The document sits beside the login method, at `<login-uri>/get_grid_info`
//! (OpenSim serves it there; a Second Life login URI answers nothing useful,
//! which is why the viewer knows its own grids). The fetch is blocking and goes
//! through the process-wide proxy ([`crate::http_proxy`]), so a caller runs it
//! off the frame thread.

use std::time::Duration;

pub use sl_proto::GridInfo;
use sl_proto::parse_grid_info_xml;
use url::Url;

/// How long a grid gets to answer before the fetch gives up: a grid manager's
/// user is waiting on it, and a grid that is down should say so promptly.
const GRID_INFO_TIMEOUT: Duration = Duration::from_secs(15);

/// Why a grid's `get_grid_info` could not be read.
#[derive(Debug)]
#[non_exhaustive]
pub enum GridInfoError {
    /// The login URI has no `get_grid_info` beside it (it cannot be a base).
    Url(url::ParseError),
    /// The request did not complete: the grid is unreachable, refused the
    /// connection, or did not answer in time.
    Http(reqwest::Error),
    /// The grid answered with an error status.
    Status(reqwest::StatusCode),
    /// The grid answered something that is not a grid-info document.
    Parse(sl_proto::WireError),
}

impl core::fmt::Display for GridInfoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Url(error) => write!(f, "no grid info address beside the login URI: {error}"),
            Self::Http(error) => write!(f, "the grid did not answer: {error}"),
            Self::Status(status) => write!(f, "the grid answered {status}"),
            Self::Parse(error) => write!(f, "the grid's answer is not grid info: {error}"),
        }
    }
}

impl core::error::Error for GridInfoError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Url(error) => Some(error),
            Self::Http(error) => Some(error),
            Self::Status(_) => None,
            Self::Parse(error) => Some(error),
        }
    }
}

/// Where `login_uri`'s grid info lives: `get_grid_info` beside the login
/// method, as Firestorm's grid manager asks for it.
///
/// # Errors
///
/// [`GridInfoError::Url`] when no such address can be formed.
pub fn grid_info_url(login_uri: &Url) -> Result<Url, GridInfoError> {
    let mut base = login_uri.clone();
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }
    base.join("get_grid_info").map_err(GridInfoError::Url)
}

/// Fetch and parse `login_uri`'s grid info. Blocks for up to fifteen
/// seconds (`GRID_INFO_TIMEOUT`).
///
/// # Errors
///
/// A [`GridInfoError`] saying which step failed.
pub fn fetch_grid_info(login_uri: &Url) -> Result<GridInfo, GridInfoError> {
    let url = grid_info_url(login_uri)?;
    let http = crate::http_proxy::blocking_client_builder()
        .timeout(GRID_INFO_TIMEOUT)
        .build()
        .map_err(GridInfoError::Http)?;
    let response = http.get(url).send().map_err(GridInfoError::Http)?;
    let status = response.status();
    if !status.is_success() {
        return Err(GridInfoError::Status(status));
    }
    let body = response.text().map_err(GridInfoError::Http)?;
    parse_grid_info_xml(&body).map_err(GridInfoError::Parse)
}

#[cfg(test)]
mod tests {
    use super::grid_info_url;
    use pretty_assertions::assert_eq;

    /// The grid info sits beside the login method whether or not the login URI
    /// ends in a slash, and beside a script path as well.
    #[test]
    fn grid_info_sits_beside_the_login_method() -> Result<(), Box<dyn core::error::Error>> {
        for (login, expected) in [
            (
                "http://127.0.0.1:9000/",
                "http://127.0.0.1:9000/get_grid_info",
            ),
            (
                "http://127.0.0.1:9000",
                "http://127.0.0.1:9000/get_grid_info",
            ),
            (
                "http://grid.example.org:8002/login",
                "http://grid.example.org:8002/login/get_grid_info",
            ),
        ] {
            assert_eq!(grid_info_url(&login.parse()?)?.as_str(), expected);
        }
        Ok(())
    }
}
