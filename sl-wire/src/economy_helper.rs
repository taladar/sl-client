//! The economy helper URIs: the XML-RPC endpoints a viewer reaches through
//! the grid's helper URI (`get_grid_info` `economy`, or the viewer's own grid
//! entry) for the buy-L$ and buy-land flows.
//!
//! Two scripts, four methods (Firestorm `llcurrencyuimanager.cpp` and
//! `llfloaterbuyland.cpp`):
//!
//! - `<helper>currency.php`: `getCurrencyQuote` (estimate the real-money cost
//!   of a L$ amount, returns a `confirm` token) and `buyCurrency` (commit
//!   with that token);
//! - `<helper>landtool.php`: `preflightBuyLandPrep` (membership / land-use
//!   upgrade requirements and a currency estimate for a parcel purchase,
//!   returns a `confirm` token) and `buyLandPrep` (commit).
//!
//! Every response is a struct with `success`; a failure carries
//! `errorMessage` and `errorURI` ([`HelperOutcome`]). Both directions are
//! modelled: a client builds requests and parses responses, a grid parses
//! requests and builds responses.

use std::collections::HashMap;

use sl_llsd::{Llsd, LlsdError};
use uuid::Uuid;

use crate::xmlrpc::{
    XmlRpcCall, XmlRpcError, build_method_call, build_method_response, parse_method_call,
    parse_method_response,
};

/// The currency helper script, relative to the helper URI.
pub const CURRENCY_HELPER_PATH: &str = "currency.php";
/// The land-tool helper script, relative to the helper URI.
pub const LAND_TOOL_HELPER_PATH: &str = "landtool.php";
/// The currency-quote method on [`CURRENCY_HELPER_PATH`].
pub const GET_CURRENCY_QUOTE_METHOD: &str = "getCurrencyQuote";
/// The buy-currency method on [`CURRENCY_HELPER_PATH`].
pub const BUY_CURRENCY_METHOD: &str = "buyCurrency";
/// The preflight method on [`LAND_TOOL_HELPER_PATH`].
pub const PREFLIGHT_BUY_LAND_PREP_METHOD: &str = "preflightBuyLandPrep";
/// The commit method on [`LAND_TOOL_HELPER_PATH`].
pub const BUY_LAND_PREP_METHOD: &str = "buyLandPrep";

/// The viewer version members every currency request carries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewerVersionInfo {
    /// `viewerChannel`.
    pub channel: String,
    /// `viewerMajorVersion`.
    pub major: i32,
    /// `viewerMinorVersion`.
    pub minor: i32,
    /// `viewerPatchVersion`.
    pub patch: i32,
    /// `viewerBuildVersion` — a string on the wire (GitHub build numbers
    /// overflow XML-RPC's 32-bit integer).
    pub build: String,
}

/// A failed helper call: `success = false` with its message and URI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HelperFailure {
    /// `errorMessage`, shown to the user.
    pub error_message: String,
    /// `errorURI`, a page the viewer offers to open (may be empty).
    pub error_uri: String,
}

/// The outcome of a helper call: the method's payload, or a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HelperOutcome<T> {
    /// `success = true` with the method's payload.
    Ok(T),
    /// `success = false`.
    Failed(HelperFailure),
}

/// A `getCurrencyQuote` request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CurrencyQuoteRequest {
    /// `agentId`.
    pub agent_id: Uuid,
    /// `secureSessionId`.
    pub secure_session_id: Uuid,
    /// `language`, e.g. `en`.
    pub language: String,
    /// `currencyBuy`, the L$ amount wanted.
    pub currency_buy: i32,
    /// The viewer version members.
    pub viewer: ViewerVersionInfo,
}

/// A successful `getCurrencyQuote` payload.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CurrencyQuote {
    /// `currency.currencyBuy` — the amount the site will actually sell (it
    /// may round the request).
    pub currency_buy: i32,
    /// `currency.estimatedCost` in US cents (the older server form).
    pub estimated_cost: Option<i32>,
    /// `currency.estimatedLocalCost`, a localised price string (the newer
    /// form).
    pub estimated_local_cost: Option<String>,
    /// `confirm`, the token to pass back to `buyCurrency`.
    pub confirm: String,
}

/// A `buyCurrency` request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuyCurrencyRequest {
    /// `agentId`.
    pub agent_id: Uuid,
    /// `secureSessionId`.
    pub secure_session_id: Uuid,
    /// `language`.
    pub language: String,
    /// `currencyBuy`.
    pub currency_buy: i32,
    /// `confirm`, the token from the quote.
    pub confirm: String,
    /// `estimatedCost` echoed from the quote, if it gave one.
    pub estimated_cost: Option<i32>,
    /// `estimatedLocalCost` echoed from the quote, if it gave one.
    pub estimated_local_cost: Option<String>,
    /// `password`, when the site demanded one.
    pub password: Option<String>,
    /// The viewer version members.
    pub viewer: ViewerVersionInfo,
}

/// A `preflightBuyLandPrep` or `buyLandPrep` request (the commit adds the
/// confirm token, the estimate, and the chosen membership level).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LandPrepRequest {
    /// `agentId`.
    pub agent_id: Uuid,
    /// `secureSessionId`.
    pub secure_session_id: Uuid,
    /// `language`.
    pub language: String,
    /// `billableArea`, the parcel's billable square metres (0 for a group buy).
    pub billable_area: i32,
    /// `currencyBuy`, the L$ the user also wants to buy for the purchase.
    pub currency_buy: i32,
    /// `levelId`, the membership level chosen (commit only).
    pub level_id: Option<String>,
    /// `estimatedCost` echoed from the preflight (commit only).
    pub estimated_cost: Option<i32>,
    /// `estimatedLocalCost` echoed from the preflight (commit only).
    pub estimated_local_cost: Option<String>,
    /// `confirm`, the preflight's token (commit only).
    pub confirm: Option<String>,
    /// `password`, when the site demanded one (commit only).
    pub password: Option<String>,
}

/// One membership level the site offers (`membership.levels[]`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MembershipLevel {
    /// `id`.
    pub id: String,
    /// `description`.
    pub description: String,
}

/// The `membership` block of a preflight response.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MembershipRequirement {
    /// `upgrade`: whether the purchase needs a membership upgrade.
    pub upgrade: bool,
    /// `action`: the human-readable upgrade action.
    pub action: String,
    /// `levels`: the levels on offer.
    pub levels: Vec<MembershipLevel>,
}

/// The `landUse` block of a preflight response.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LandUseRequirement {
    /// `upgrade`: whether the purchase needs a land-use fee upgrade.
    pub upgrade: bool,
    /// `action`: the human-readable upgrade action.
    pub action: String,
}

/// A successful `preflightBuyLandPrep` payload.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LandPrep {
    /// The `membership` block.
    pub membership: MembershipRequirement,
    /// The `landUse` block.
    pub land_use: LandUseRequirement,
    /// `currency.estimatedCost` in US cents.
    pub estimated_cost: Option<i32>,
    /// `currency.estimatedLocalCost`.
    pub estimated_local_cost: Option<String>,
    /// `confirm`, the token to pass back to `buyLandPrep`.
    pub confirm: String,
}

/// Map-building helper: a fresh struct.
fn map() -> HashMap<String, Llsd> {
    HashMap::new()
}

/// Inserts a string member.
fn put_str(map: &mut HashMap<String, Llsd>, key: &str, value: &str) {
    map.insert(key.to_owned(), Llsd::String(value.to_owned()));
}

/// Inserts an integer member.
fn put_int(map: &mut HashMap<String, Llsd>, key: &str, value: i32) {
    map.insert(key.to_owned(), Llsd::Integer(value));
}

/// Inserts a boolean member.
fn put_bool(map: &mut HashMap<String, Llsd>, key: &str, value: bool) {
    map.insert(key.to_owned(), Llsd::Boolean(value));
}

/// Inserts the viewer version members.
fn put_viewer(map: &mut HashMap<String, Llsd>, viewer: &ViewerVersionInfo) {
    put_str(map, "viewerChannel", &viewer.channel);
    put_int(map, "viewerMajorVersion", viewer.major);
    put_int(map, "viewerMinorVersion", viewer.minor);
    put_int(map, "viewerPatchVersion", viewer.patch);
    put_str(map, "viewerBuildVersion", &viewer.build);
}

/// Reads the viewer version members (all optional — an older viewer omits
/// them).
fn get_viewer(params: &Llsd) -> ViewerVersionInfo {
    ViewerVersionInfo {
        channel: opt_str(params, "viewerChannel").unwrap_or_default(),
        major: opt_int(params, "viewerMajorVersion").unwrap_or_default(),
        minor: opt_int(params, "viewerMinorVersion").unwrap_or_default(),
        patch: opt_int(params, "viewerPatchVersion").unwrap_or_default(),
        build: opt_str(params, "viewerBuildVersion").unwrap_or_default(),
    }
}

/// An optional string member (an integer-typed value is rendered as text).
fn opt_str(params: &Llsd, key: &str) -> Option<String> {
    match params.get(key)? {
        Llsd::String(text) => Some(text.clone()),
        Llsd::Integer(value) => Some(value.to_string()),
        other => other.as_str().map(str::to_owned),
    }
}

/// An optional integer member (a numeric string is accepted too).
fn opt_int(params: &Llsd, key: &str) -> Option<i32> {
    match params.get(key)? {
        Llsd::Integer(value) => Some(*value),
        Llsd::String(text) => text.trim().parse().ok(),
        other => other.as_i32(),
    }
}

/// A required string member.
fn req_str(params: &Llsd, key: &str, field: &'static str) -> Result<String, XmlRpcError> {
    opt_str(params, key).ok_or(XmlRpcError::Llsd(LlsdError::MissingField { field }))
}

/// A required integer member.
fn req_int(params: &Llsd, key: &str, field: &'static str) -> Result<i32, XmlRpcError> {
    opt_int(params, key).ok_or(XmlRpcError::Llsd(LlsdError::MissingField { field }))
}

/// A required UUID member (string-encoded on the wire).
fn req_uuid(params: &Llsd, key: &str, field: &'static str) -> Result<Uuid, XmlRpcError> {
    let text = req_str(params, key, field)?;
    Uuid::parse_str(text.trim())
        .map_err(|_error| XmlRpcError::Llsd(LlsdError::MalformedField { field, value: text }))
}

/// A boolean member (`0`/`1` or `true`/`false` text tolerated); absent = false.
fn opt_bool(params: &Llsd, key: &str) -> bool {
    match params.get(key) {
        Some(Llsd::Boolean(value)) => *value,
        Some(Llsd::Integer(value)) => *value != 0,
        Some(Llsd::String(text)) => matches!(text.trim(), "1" | "true"),
        _ => false,
    }
}

/// Unwraps a parsed call, checking its method name and taking the single
/// struct parameter.
fn call_params(call: XmlRpcCall, expected: &str) -> Result<Llsd, XmlRpcError> {
    if call.method != expected {
        return Err(XmlRpcError::UnexpectedMethod {
            method: call.method,
        });
    }
    call.params
        .into_iter()
        .next()
        .filter(|param| matches!(param, Llsd::Map(_)))
        .ok_or(XmlRpcError::Llsd(LlsdError::MissingField {
            field: "params[0]",
        }))
}

/// Parses a response document into its struct payload, mapping `success`
/// into [`HelperOutcome`] and handing the struct to `decode` on success.
fn response_outcome<T>(
    xml: &str,
    decode: impl FnOnce(&Llsd) -> Result<T, XmlRpcError>,
) -> Result<HelperOutcome<T>, XmlRpcError> {
    let response = parse_method_response(xml)?;
    let Some(payload) = response.first_param().filter(|p| matches!(p, Llsd::Map(_))) else {
        return Err(XmlRpcError::Llsd(LlsdError::MissingField {
            field: "params[0]",
        }));
    };
    if !opt_bool(payload, "success") {
        return Ok(HelperOutcome::Failed(HelperFailure {
            error_message: opt_str(payload, "errorMessage").unwrap_or_default(),
            error_uri: opt_str(payload, "errorURI").unwrap_or_default(),
        }));
    }
    decode(payload).map(HelperOutcome::Ok)
}

/// Builds the struct of a response: the failure shape, or `success = true`
/// plus the members `fill` adds.
fn build_outcome<T>(
    outcome: &HelperOutcome<T>,
    fill: impl FnOnce(&T, &mut HashMap<String, Llsd>),
) -> String {
    let mut out = map();
    match outcome {
        HelperOutcome::Ok(payload) => {
            put_bool(&mut out, "success", true);
            fill(payload, &mut out);
        }
        HelperOutcome::Failed(failure) => {
            put_bool(&mut out, "success", false);
            put_str(&mut out, "errorMessage", &failure.error_message);
            put_str(&mut out, "errorURI", &failure.error_uri);
        }
    }
    build_method_response(&[Llsd::Map(out)])
}

/// Inserts the `currency` sub-struct shared by quotes and land preflights.
fn put_currency(
    out: &mut HashMap<String, Llsd>,
    currency_buy: Option<i32>,
    estimated_cost: Option<i32>,
    estimated_local_cost: Option<&str>,
) {
    let mut currency = map();
    if let Some(amount) = currency_buy {
        put_int(&mut currency, "currencyBuy", amount);
    }
    if let Some(cents) = estimated_cost {
        put_int(&mut currency, "estimatedCost", cents);
    }
    if let Some(local) = estimated_local_cost {
        put_str(&mut currency, "estimatedLocalCost", local);
    }
    out.insert("currency".to_owned(), Llsd::Map(currency));
}

/// Reads the `currency` sub-struct's estimate members.
fn get_estimates(payload: &Llsd) -> (Option<i32>, Option<String>) {
    let currency = payload.get("currency").unwrap_or(&Llsd::Undef);
    (
        opt_int(currency, "estimatedCost"),
        opt_str(currency, "estimatedLocalCost"),
    )
}

// ---- getCurrencyQuote ----------------------------------------------------

/// Builds a `getCurrencyQuote` call.
#[must_use]
pub fn build_currency_quote_request(request: &CurrencyQuoteRequest) -> String {
    let mut params = map();
    put_str(&mut params, "agentId", &request.agent_id.to_string());
    put_str(
        &mut params,
        "secureSessionId",
        &request.secure_session_id.to_string(),
    );
    put_str(&mut params, "language", &request.language);
    put_int(&mut params, "currencyBuy", request.currency_buy);
    put_viewer(&mut params, &request.viewer);
    build_method_call(GET_CURRENCY_QUOTE_METHOD, &[Llsd::Map(params)])
}

/// Parses a `getCurrencyQuote` call.
///
/// # Errors
///
/// Returns an [`XmlRpcError`] for a malformed document, a different method
/// name, or a missing/invalid required member.
pub fn parse_currency_quote_request(xml: &str) -> Result<CurrencyQuoteRequest, XmlRpcError> {
    let params = call_params(parse_method_call(xml)?, GET_CURRENCY_QUOTE_METHOD)?;
    Ok(CurrencyQuoteRequest {
        agent_id: req_uuid(&params, "agentId", "agentId")?,
        secure_session_id: req_uuid(&params, "secureSessionId", "secureSessionId")?,
        language: opt_str(&params, "language").unwrap_or_default(),
        currency_buy: req_int(&params, "currencyBuy", "currencyBuy")?,
        viewer: get_viewer(&params),
    })
}

/// Builds a `getCurrencyQuote` response.
#[must_use]
pub fn build_currency_quote_response(outcome: &HelperOutcome<CurrencyQuote>) -> String {
    build_outcome(outcome, |quote, out| {
        put_currency(
            out,
            Some(quote.currency_buy),
            quote.estimated_cost,
            quote.estimated_local_cost.as_deref(),
        );
        put_str(out, "confirm", &quote.confirm);
    })
}

/// Parses a `getCurrencyQuote` response.
///
/// # Errors
///
/// Returns an [`XmlRpcError`] for a malformed document or a response without
/// a struct payload.
pub fn parse_currency_quote_response(
    xml: &str,
) -> Result<HelperOutcome<CurrencyQuote>, XmlRpcError> {
    response_outcome(xml, |payload| {
        let (estimated_cost, estimated_local_cost) = get_estimates(payload);
        Ok(CurrencyQuote {
            currency_buy: payload
                .get("currency")
                .and_then(|currency| opt_int(currency, "currencyBuy"))
                .unwrap_or_default(),
            estimated_cost,
            estimated_local_cost,
            confirm: opt_str(payload, "confirm").unwrap_or_default(),
        })
    })
}

// ---- buyCurrency ---------------------------------------------------------

/// Builds a `buyCurrency` call.
#[must_use]
pub fn build_buy_currency_request(request: &BuyCurrencyRequest) -> String {
    let mut params = map();
    put_str(&mut params, "agentId", &request.agent_id.to_string());
    put_str(
        &mut params,
        "secureSessionId",
        &request.secure_session_id.to_string(),
    );
    put_str(&mut params, "language", &request.language);
    put_int(&mut params, "currencyBuy", request.currency_buy);
    put_str(&mut params, "confirm", &request.confirm);
    put_viewer(&mut params, &request.viewer);
    if let Some(cents) = request.estimated_cost {
        put_int(&mut params, "estimatedCost", cents);
    }
    if let Some(local) = &request.estimated_local_cost {
        put_str(&mut params, "estimatedLocalCost", local);
    }
    if let Some(password) = &request.password {
        put_str(&mut params, "password", password);
    }
    build_method_call(BUY_CURRENCY_METHOD, &[Llsd::Map(params)])
}

/// Parses a `buyCurrency` call.
///
/// # Errors
///
/// Returns an [`XmlRpcError`] for a malformed document, a different method
/// name, or a missing/invalid required member.
pub fn parse_buy_currency_request(xml: &str) -> Result<BuyCurrencyRequest, XmlRpcError> {
    let params = call_params(parse_method_call(xml)?, BUY_CURRENCY_METHOD)?;
    Ok(BuyCurrencyRequest {
        agent_id: req_uuid(&params, "agentId", "agentId")?,
        secure_session_id: req_uuid(&params, "secureSessionId", "secureSessionId")?,
        language: opt_str(&params, "language").unwrap_or_default(),
        currency_buy: req_int(&params, "currencyBuy", "currencyBuy")?,
        confirm: req_str(&params, "confirm", "confirm")?,
        estimated_cost: opt_int(&params, "estimatedCost"),
        estimated_local_cost: opt_str(&params, "estimatedLocalCost"),
        password: opt_str(&params, "password"),
        viewer: get_viewer(&params),
    })
}

/// Builds a `buyCurrency` response (success carries no payload).
#[must_use]
pub fn build_buy_currency_response(outcome: &HelperOutcome<()>) -> String {
    build_outcome(outcome, |(), _out| {})
}

/// Parses a `buyCurrency` response.
///
/// # Errors
///
/// Returns an [`XmlRpcError`] for a malformed document or a response without
/// a struct payload.
pub fn parse_buy_currency_response(xml: &str) -> Result<HelperOutcome<()>, XmlRpcError> {
    response_outcome(xml, |_payload| Ok(()))
}

// ---- preflightBuyLandPrep / buyLandPrep ----------------------------------

/// Inserts the members shared by both land-prep calls.
fn put_land_prep(params: &mut HashMap<String, Llsd>, request: &LandPrepRequest) {
    put_str(params, "agentId", &request.agent_id.to_string());
    put_str(
        params,
        "secureSessionId",
        &request.secure_session_id.to_string(),
    );
    put_str(params, "language", &request.language);
    put_int(params, "billableArea", request.billable_area);
    put_int(params, "currencyBuy", request.currency_buy);
    if let Some(level) = &request.level_id {
        put_str(params, "levelId", level);
    }
    if let Some(cents) = request.estimated_cost {
        put_int(params, "estimatedCost", cents);
    }
    if let Some(local) = &request.estimated_local_cost {
        put_str(params, "estimatedLocalCost", local);
    }
    if let Some(confirm) = &request.confirm {
        put_str(params, "confirm", confirm);
    }
    if let Some(password) = &request.password {
        put_str(params, "password", password);
    }
}

/// Reads the members shared by both land-prep calls.
fn get_land_prep(params: &Llsd) -> Result<LandPrepRequest, XmlRpcError> {
    Ok(LandPrepRequest {
        agent_id: req_uuid(params, "agentId", "agentId")?,
        secure_session_id: req_uuid(params, "secureSessionId", "secureSessionId")?,
        language: opt_str(params, "language").unwrap_or_default(),
        billable_area: req_int(params, "billableArea", "billableArea")?,
        currency_buy: opt_int(params, "currencyBuy").unwrap_or_default(),
        level_id: opt_str(params, "levelId"),
        estimated_cost: opt_int(params, "estimatedCost"),
        estimated_local_cost: opt_str(params, "estimatedLocalCost"),
        confirm: opt_str(params, "confirm"),
        password: opt_str(params, "password"),
    })
}

/// Builds a `preflightBuyLandPrep` call.
#[must_use]
pub fn build_preflight_land_prep_request(request: &LandPrepRequest) -> String {
    let mut params = map();
    put_land_prep(&mut params, request);
    build_method_call(PREFLIGHT_BUY_LAND_PREP_METHOD, &[Llsd::Map(params)])
}

/// Parses a `preflightBuyLandPrep` call.
///
/// # Errors
///
/// Returns an [`XmlRpcError`] for a malformed document, a different method
/// name, or a missing/invalid required member.
pub fn parse_preflight_land_prep_request(xml: &str) -> Result<LandPrepRequest, XmlRpcError> {
    let params = call_params(parse_method_call(xml)?, PREFLIGHT_BUY_LAND_PREP_METHOD)?;
    get_land_prep(&params)
}

/// Builds a `preflightBuyLandPrep` response.
#[must_use]
pub fn build_preflight_land_prep_response(outcome: &HelperOutcome<LandPrep>) -> String {
    build_outcome(outcome, |prep, out| {
        let mut membership = map();
        put_bool(&mut membership, "upgrade", prep.membership.upgrade);
        put_str(&mut membership, "action", &prep.membership.action);
        membership.insert(
            "levels".to_owned(),
            Llsd::Array(
                prep.membership
                    .levels
                    .iter()
                    .map(|level| {
                        let mut entry = map();
                        put_str(&mut entry, "id", &level.id);
                        put_str(&mut entry, "description", &level.description);
                        Llsd::Map(entry)
                    })
                    .collect(),
            ),
        );
        out.insert("membership".to_owned(), Llsd::Map(membership));
        let mut land_use = map();
        put_bool(&mut land_use, "upgrade", prep.land_use.upgrade);
        put_str(&mut land_use, "action", &prep.land_use.action);
        out.insert("landUse".to_owned(), Llsd::Map(land_use));
        put_currency(
            out,
            None,
            prep.estimated_cost,
            prep.estimated_local_cost.as_deref(),
        );
        put_str(out, "confirm", &prep.confirm);
    })
}

/// Parses a `preflightBuyLandPrep` response.
///
/// # Errors
///
/// Returns an [`XmlRpcError`] for a malformed document or a response without
/// a struct payload.
pub fn parse_preflight_land_prep_response(
    xml: &str,
) -> Result<HelperOutcome<LandPrep>, XmlRpcError> {
    response_outcome(xml, |payload| {
        let membership = payload.get("membership").unwrap_or(&Llsd::Undef);
        let land_use = payload.get("landUse").unwrap_or(&Llsd::Undef);
        let (estimated_cost, estimated_local_cost) = get_estimates(payload);
        Ok(LandPrep {
            membership: MembershipRequirement {
                upgrade: opt_bool(membership, "upgrade"),
                action: opt_str(membership, "action").unwrap_or_default(),
                levels: membership
                    .get("levels")
                    .and_then(Llsd::as_array)
                    .unwrap_or_default()
                    .iter()
                    .map(|level| MembershipLevel {
                        id: opt_str(level, "id").unwrap_or_default(),
                        description: opt_str(level, "description").unwrap_or_default(),
                    })
                    .collect(),
            },
            land_use: LandUseRequirement {
                upgrade: opt_bool(land_use, "upgrade"),
                action: opt_str(land_use, "action").unwrap_or_default(),
            },
            estimated_cost,
            estimated_local_cost,
            confirm: opt_str(payload, "confirm").unwrap_or_default(),
        })
    })
}

/// Builds a `buyLandPrep` call.
#[must_use]
pub fn build_buy_land_prep_request(request: &LandPrepRequest) -> String {
    let mut params = map();
    put_land_prep(&mut params, request);
    build_method_call(BUY_LAND_PREP_METHOD, &[Llsd::Map(params)])
}

/// Parses a `buyLandPrep` call.
///
/// # Errors
///
/// Returns an [`XmlRpcError`] for a malformed document, a different method
/// name, or a missing/invalid required member.
pub fn parse_buy_land_prep_request(xml: &str) -> Result<LandPrepRequest, XmlRpcError> {
    let params = call_params(parse_method_call(xml)?, BUY_LAND_PREP_METHOD)?;
    get_land_prep(&params)
}

/// Builds a `buyLandPrep` response (success carries no payload).
#[must_use]
pub fn build_buy_land_prep_response(outcome: &HelperOutcome<()>) -> String {
    build_outcome(outcome, |(), _out| {})
}

/// Parses a `buyLandPrep` response.
///
/// # Errors
///
/// Returns an [`XmlRpcError`] for a malformed document or a response without
/// a struct payload.
pub fn parse_buy_land_prep_response(xml: &str) -> Result<HelperOutcome<()>, XmlRpcError> {
    response_outcome(xml, |_payload| Ok(()))
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{
        BuyCurrencyRequest, CurrencyQuote, CurrencyQuoteRequest, HelperFailure, HelperOutcome,
        LandPrep, LandPrepRequest, LandUseRequirement, MembershipLevel, MembershipRequirement,
        ViewerVersionInfo, build_buy_currency_request, build_buy_currency_response,
        build_buy_land_prep_request, build_buy_land_prep_response, build_currency_quote_request,
        build_currency_quote_response, build_preflight_land_prep_request,
        build_preflight_land_prep_response, parse_buy_currency_request,
        parse_buy_currency_response, parse_buy_land_prep_request, parse_buy_land_prep_response,
        parse_currency_quote_request, parse_currency_quote_response,
        parse_preflight_land_prep_request, parse_preflight_land_prep_response,
    };
    use crate::xmlrpc::XmlRpcError;

    fn agent() -> Uuid {
        Uuid::from_u128(0xA0A0_0000_0000_0000_0000_0000_0000_0001)
    }

    fn session() -> Uuid {
        Uuid::from_u128(0x5E55_0000_0000_0000_0000_0000_0000_0002)
    }

    fn viewer() -> ViewerVersionInfo {
        ViewerVersionInfo {
            channel: "Firestorm-Release".to_owned(),
            major: 7,
            minor: 1,
            patch: 9,
            build: "74745".to_owned(),
        }
    }

    #[test]
    fn currency_quote_round_trips_both_directions() -> Result<(), XmlRpcError> {
        let request = CurrencyQuoteRequest {
            agent_id: agent(),
            secure_session_id: session(),
            language: "en".to_owned(),
            currency_buy: 1000,
            viewer: viewer(),
        };
        let xml = build_currency_quote_request(&request);
        assert_eq!(parse_currency_quote_request(&xml)?, request);

        let quote = HelperOutcome::Ok(CurrencyQuote {
            currency_buy: 1000,
            estimated_cost: Some(400),
            estimated_local_cost: Some("US$ 4.00".to_owned()),
            confirm: "tok".to_owned(),
        });
        let xml = build_currency_quote_response(&quote);
        assert_eq!(parse_currency_quote_response(&xml)?, quote);

        let failed: HelperOutcome<CurrencyQuote> = HelperOutcome::Failed(HelperFailure {
            error_message: "closed".to_owned(),
            error_uri: "http://x/".to_owned(),
        });
        let xml = build_currency_quote_response(&failed);
        assert_eq!(parse_currency_quote_response(&xml)?, failed);
        Ok(())
    }

    #[test]
    fn buy_currency_round_trips() -> Result<(), XmlRpcError> {
        let request = BuyCurrencyRequest {
            agent_id: agent(),
            secure_session_id: session(),
            language: "de".to_owned(),
            currency_buy: 250,
            confirm: "tok".to_owned(),
            estimated_cost: None,
            estimated_local_cost: Some("US$ 1.00".to_owned()),
            password: Some("hunter2".to_owned()),
            viewer: viewer(),
        };
        let xml = build_buy_currency_request(&request);
        assert_eq!(parse_buy_currency_request(&xml)?, request);
        let ok = HelperOutcome::Ok(());
        assert_eq!(
            parse_buy_currency_response(&build_buy_currency_response(&ok))?,
            ok
        );
        Ok(())
    }

    #[test]
    fn land_prep_round_trips() -> Result<(), XmlRpcError> {
        let preflight = LandPrepRequest {
            agent_id: agent(),
            secure_session_id: session(),
            language: "en".to_owned(),
            billable_area: 512,
            currency_buy: 0,
            level_id: None,
            estimated_cost: None,
            estimated_local_cost: None,
            confirm: None,
            password: None,
        };
        let xml = build_preflight_land_prep_request(&preflight);
        assert_eq!(parse_preflight_land_prep_request(&xml)?, preflight);

        let prep = HelperOutcome::Ok(LandPrep {
            membership: MembershipRequirement {
                upgrade: true,
                action: "upgrade to premium".to_owned(),
                levels: vec![MembershipLevel {
                    id: "premium".to_owned(),
                    description: "Premium".to_owned(),
                }],
            },
            land_use: LandUseRequirement {
                upgrade: false,
                action: String::new(),
            },
            estimated_cost: Some(995),
            estimated_local_cost: None,
            confirm: "land-tok".to_owned(),
        });
        let xml = build_preflight_land_prep_response(&prep);
        assert_eq!(parse_preflight_land_prep_response(&xml)?, prep);

        let commit = LandPrepRequest {
            level_id: Some("premium".to_owned()),
            estimated_cost: Some(995),
            confirm: Some("land-tok".to_owned()),
            ..preflight
        };
        let xml = build_buy_land_prep_request(&commit);
        assert_eq!(parse_buy_land_prep_request(&xml)?, commit);
        let ok = HelperOutcome::Ok(());
        assert_eq!(
            parse_buy_land_prep_response(&build_buy_land_prep_response(&ok))?,
            ok
        );
        Ok(())
    }

    /// The literal shape Firestorm's `LLXMLRPCTransaction` emits (integers as
    /// `<int>`, everything else strings).
    const FIRESTORM_QUOTE: &str = r#"<?xml version="1.0"?>
<methodCall><methodName>getCurrencyQuote</methodName><params><param><value><struct>
<member><name>agentId</name><value><string>a0a00000-0000-0000-0000-000000000001</string></value></member>
<member><name>secureSessionId</name><value><string>5e550000-0000-0000-0000-000000000002</string></value></member>
<member><name>language</name><value><string>en</string></value></member>
<member><name>currencyBuy</name><value><int>2000</int></value></member>
<member><name>viewerChannel</name><value><string>Firestorm-Release</string></value></member>
<member><name>viewerMajorVersion</name><value><int>7</int></value></member>
<member><name>viewerMinorVersion</name><value><int>1</int></value></member>
<member><name>viewerPatchVersion</name><value><int>9</int></value></member>
<member><name>viewerBuildVersion</name><value><string>74745</string></value></member>
</struct></value></param></params></methodCall>"#;

    #[test]
    fn firestorm_literal_quote_parses() -> Result<(), XmlRpcError> {
        let request = parse_currency_quote_request(FIRESTORM_QUOTE)?;
        assert_eq!(request.agent_id, agent());
        assert_eq!(request.secure_session_id, session());
        assert_eq!(request.currency_buy, 2000);
        assert_eq!(request.viewer, viewer());
        Ok(())
    }

    #[test]
    fn wrong_method_and_missing_members_are_rejected() {
        assert!(matches!(
            parse_buy_currency_request(FIRESTORM_QUOTE),
            Err(XmlRpcError::UnexpectedMethod { method }) if method == "getCurrencyQuote"
        ));
        let no_agent = FIRESTORM_QUOTE.replace("agentId", "agentX");
        assert!(matches!(
            parse_currency_quote_request(&no_agent),
            Err(XmlRpcError::Llsd(_))
        ));
    }
}
