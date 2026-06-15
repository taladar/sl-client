#![doc = include_str!("../README.md")]

use thiserror::Error;

use tracing::instrument;

/// Error type for sl_wire
#[derive(Debug, Error)]
pub enum Error {
    /// This variant is just here as a placeholder to show the syntax
    #[error("error type not implemented yet")]
    NotImplementedError,
}

/// Just a function to illustrate use of instrument and Error
#[expect(
    dead_code,
    reason = "This example function is not called by anything since this is a library but we need to be clippy clean for the initial commit to pass pre-commit hooks"
)]
#[instrument]
async fn example_fun() -> Result<(), Error> {
    tracing::debug!("{:#?}", "Hello, World!");
    Ok(())
}

#[cfg(test)]
mod test {
    //use super::*;
    //use pretty_assertions::{assert_eq, assert_ne};
}
