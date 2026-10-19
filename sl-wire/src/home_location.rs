//! The **`HomeLocation`** capability: "Set Home to Here" over HTTP
//! (`LLAgent::setStartPosition`), in place of the UDP
//! `SetStartLocationRequest` it falls back to.
//!
//! The reference POSTs
//!
//! ```text
//! { HomeLocation: { LocationId: <slot>, LocationPos: { X, Y, Z },
//!                   LocationLookAt: { X, Y, Z } } }
//! ```
//!
//! — the slot as an integer, both vectors as maps of reals — and reads back
//!
//! ```text
//! { success: <true>, HomeLocation: { LocationPos: { X, Y, Z } } }
//! ```
//!
//! taking the position the grid stored (`setStartPositionSuccess`). The
//! reference reads `success` as a string that begins `true`, so a grid may
//! send it as a boolean or as the string; OpenSim sends the string. A refusal
//! is `success` false (and nothing else): the grid says why in an alert sent
//! beside the answer, not in it.

use std::collections::HashMap;

use sl_types::lsl::Vector;
use sl_types::map::RegionCoordinates;

use crate::error::WireError;
use crate::llsd::Llsd;

/// What a `HomeLocation` POST asks: store `position` / `look_at` in the start
/// location slot `location_id` (`1` for home).
#[derive(Debug, Clone, PartialEq)]
pub struct HomeLocationRequest {
    /// The slot (`LocationId`): the reference viewer's `EStartLocation`
    /// ordinal, `1` for home.
    pub location_id: u32,
    /// The region-local position to store (`LocationPos`).
    pub position: RegionCoordinates,
    /// The direction to face on arrival (`LocationLookAt`).
    pub look_at: Vector,
}

/// A `HomeLocation` answer.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HomeLocationAnswer {
    /// Whether the location was stored (`success`).
    pub success: bool,
    /// The position the grid stored (`HomeLocation.LocationPos`), where the
    /// answer names one.
    pub position: Option<RegionCoordinates>,
}

/// How a `HomeLocation` answer spells its `success`: the reference reads
/// either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeLocationSuccessForm {
    /// An LLSD boolean.
    Boolean,
    /// The string `true` / `false` (OpenSim's form).
    String,
}

/// A vector as the `{ X, Y, Z }` map of reals both directions use.
fn vector_map(x: f32, y: f32, z: f32) -> Llsd {
    Llsd::Map(HashMap::from([
        ("X".to_owned(), Llsd::Real(f64::from(x))),
        ("Y".to_owned(), Llsd::Real(f64::from(y))),
        ("Z".to_owned(), Llsd::Real(f64::from(z))),
    ]))
}

/// Reads a `{ X, Y, Z }` map whose components are reals or integers.
fn read_vector(map: Option<&Llsd>, field: &'static str) -> Result<Option<Vector>, WireError> {
    let Some(map) = map else {
        return Ok(None);
    };
    let component = |key: &str| {
        map.get(key).and_then(Llsd::as_f32).ok_or_else(|| {
            WireError::from(sl_llsd::LlsdError::MalformedField {
                field,
                value: format!("no number {key}"),
            })
        })
    };
    Ok(Some(Vector {
        x: component("X")?,
        y: component("Y")?,
        z: component("Z")?,
    }))
}

/// Builds the body of a `HomeLocation` POST, as the reference sends it.
#[must_use]
pub fn build_home_location_request(request: &HomeLocationRequest) -> String {
    let home = Llsd::Map(HashMap::from([
        (
            "LocationId".to_owned(),
            Llsd::Integer(i32::try_from(request.location_id).unwrap_or(i32::MAX)),
        ),
        (
            "LocationPos".to_owned(),
            vector_map(
                request.position.x(),
                request.position.y(),
                request.position.z(),
            ),
        ),
        (
            "LocationLookAt".to_owned(),
            vector_map(request.look_at.x, request.look_at.y, request.look_at.z),
        ),
    ]));
    Llsd::Map(HashMap::from([("HomeLocation".to_owned(), home)])).to_llsd_xml()
}

/// Reads the body of a `HomeLocation` POST — the server side of
/// [`build_home_location_request`]. `None` for a body that is not a request:
/// no `HomeLocation` map, or one missing its position or look-at, which both
/// grids refuse rather than reject.
///
/// # Errors
///
/// Returns [`WireError`] if a vector is present but not a map of three
/// numbers.
pub fn parse_home_location_request(body: &Llsd) -> Result<Option<HomeLocationRequest>, WireError> {
    let Some(home) = body
        .get("HomeLocation")
        .filter(|home| home.as_map().is_some())
    else {
        return Ok(None);
    };
    let position = read_vector(home.get("LocationPos"), "LocationPos")?;
    let look_at = read_vector(home.get("LocationLookAt"), "LocationLookAt")?;
    let location_id = home
        .get("LocationId")
        .and_then(Llsd::as_i32)
        .and_then(|id| u32::try_from(id).ok())
        .unwrap_or(1);
    Ok(position
        .zip(look_at)
        .map(|(position, look_at)| HomeLocationRequest {
            location_id,
            position: RegionCoordinates::new(position.x, position.y, position.z),
            look_at,
        }))
}

/// Builds a `HomeLocation` answer (server side), spelling `success` in `form`.
#[must_use]
pub fn build_home_location_answer(
    answer: &HomeLocationAnswer,
    form: HomeLocationSuccessForm,
) -> String {
    let success = match form {
        HomeLocationSuccessForm::Boolean => Llsd::Boolean(answer.success),
        HomeLocationSuccessForm::String => Llsd::String(answer.success.to_string()),
    };
    let mut map = HashMap::from([("success".to_owned(), success)]);
    if let Some(position) = answer.position {
        let _previous = map.insert(
            "HomeLocation".to_owned(),
            Llsd::Map(HashMap::from([(
                "LocationPos".to_owned(),
                vector_map(position.x(), position.y(), position.z()),
            )])),
        );
    }
    Llsd::Map(map).to_llsd_xml()
}

/// Reads a `HomeLocation` answer as the reference does: `success` true as a
/// boolean or as a string beginning `true`, anything else (or nothing) false.
///
/// # Errors
///
/// Returns [`WireError`] if the stored position is present but not a map of
/// three numbers.
pub fn parse_home_location_answer(body: &Llsd) -> Result<HomeLocationAnswer, WireError> {
    let success = match body.get("success") {
        Some(Llsd::String(text)) => text.starts_with("true"),
        Some(other) => other.as_bool().unwrap_or(false),
        None => false,
    };
    let position = read_vector(
        body.get("HomeLocation")
            .and_then(|home| home.get("LocationPos")),
        "LocationPos",
    )?
    .map(|position| RegionCoordinates::new(position.x, position.y, position.z));
    Ok(HomeLocationAnswer { success, position })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_types::lsl::Vector;
    use sl_types::map::RegionCoordinates;

    use super::{
        HomeLocationAnswer, HomeLocationRequest, HomeLocationSuccessForm,
        build_home_location_answer, build_home_location_request, parse_home_location_answer,
        parse_home_location_request,
    };
    use crate::llsd::{Llsd, parse_llsd_xml};

    /// A request survives the trip through its body.
    #[test]
    fn a_request_round_trips() -> Result<(), String> {
        let request = HomeLocationRequest {
            location_id: 1,
            position: RegionCoordinates::new(52.5, 62.25, 33.0),
            look_at: Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
        };
        let body = parse_llsd_xml(&build_home_location_request(&request))
            .map_err(|error| format!("{error:?}"))?;
        let parsed = parse_home_location_request(&body).map_err(|error| format!("{error:?}"))?;
        assert_eq!(parsed, Some(request));
        Ok(())
    }

    /// A body without the map, or without its look-at, is no request.
    #[test]
    fn an_incomplete_request_is_none() -> Result<(), String> {
        let empty = parse_llsd_xml("<llsd><map></map></llsd>").map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            parse_home_location_request(&empty).map_err(|e| format!("{e:?}"))?,
            None
        );
        let no_look = parse_llsd_xml(
            "<llsd><map><key>HomeLocation</key><map><key>LocationPos</key><map>\
             <key>X</key><real>1</real><key>Y</key><real>2</real><key>Z</key><real>3</real>\
             </map></map></map></llsd>",
        )
        .map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            parse_home_location_request(&no_look).map_err(|e| format!("{e:?}"))?,
            None
        );
        Ok(())
    }

    /// An answer survives the trip in either spelling of `success`.
    #[test]
    fn an_answer_round_trips_in_either_form() -> Result<(), String> {
        for form in [
            HomeLocationSuccessForm::Boolean,
            HomeLocationSuccessForm::String,
        ] {
            for answer in [
                HomeLocationAnswer {
                    success: true,
                    position: Some(RegionCoordinates::new(52.5, 62.25, 33.0)),
                },
                HomeLocationAnswer {
                    success: false,
                    position: None,
                },
            ] {
                let body = parse_llsd_xml(&build_home_location_answer(&answer, form))
                    .map_err(|error| format!("{error:?}"))?;
                assert_eq!(
                    parse_home_location_answer(&body).map_err(|error| format!("{error:?}"))?,
                    answer
                );
            }
        }
        Ok(())
    }

    /// Integer components are read as the reference reads them, and a
    /// `success` that is no boolean and no `true` string is a refusal.
    #[test]
    fn an_answer_is_read_leniently() -> Result<(), String> {
        let body = parse_llsd_xml(
            "<llsd><map><key>success</key><string>true enough</string><key>HomeLocation</key><map>\
             <key>LocationPos</key><map><key>X</key><integer>52</integer><key>Y</key>\
             <integer>62</integer><key>Z</key><real>33.5</real></map></map></map></llsd>",
        )
        .map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            parse_home_location_answer(&body).map_err(|e| format!("{e:?}"))?,
            HomeLocationAnswer {
                success: true,
                position: Some(RegionCoordinates::new(52.0, 62.0, 33.5)),
            }
        );
        assert_eq!(
            parse_home_location_answer(&Llsd::Undef).map_err(|e| format!("{e:?}"))?,
            HomeLocationAnswer {
                success: false,
                position: None,
            }
        );
        Ok(())
    }
}
