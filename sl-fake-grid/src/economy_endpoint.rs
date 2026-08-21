//! The economy helper endpoints: `POST /currency.php` and `POST /landtool.php`
//! (the helper URI is the login URI), dispatching the four XML-RPC methods to
//! the [`EconomyConfig`](crate::EconomyConfig) policy and publishing accepted
//! purchases on the grid's economy channel.

use std::sync::Arc;

use sl_wire::{
    BUY_CURRENCY_METHOD, BUY_LAND_PREP_METHOD, CURRENCY_HELPER_PATH, GET_CURRENCY_QUOTE_METHOD,
    HelperOutcome, LAND_TOOL_HELPER_PATH, PREFLIGHT_BUY_LAND_PREP_METHOD, XmlRpcError,
    build_buy_currency_response, build_buy_land_prep_response, build_currency_quote_response,
    build_fault, build_preflight_land_prep_response, parse_buy_currency_request,
    parse_buy_land_prep_request, parse_currency_quote_request, parse_preflight_land_prep_request,
};

use crate::http_answer::HttpAnswer;
use crate::runtime::GridCore;

/// The XML-RPC content type.
const XML_RPC_CONTENT_TYPE: &str = "text/xml";

/// The fault code answered for a method the script does not implement.
const FAULT_UNKNOWN_METHOD: i32 = -32601;
/// The fault code answered for an unparsable request.
const FAULT_BAD_REQUEST: i32 = -32700;

/// Whether `path` is one of the helper scripts (with or without a leading `/`).
pub(crate) fn is_helper_path(path: &str) -> bool {
    let path = path.strip_prefix('/').unwrap_or(path);
    path == CURRENCY_HELPER_PATH || path == LAND_TOOL_HELPER_PATH
}

/// Serves one helper POST.
pub(crate) fn handle_helper(core: &Arc<GridCore>, path: &str, body: &[u8]) -> HttpAnswer {
    let Ok(text) = std::str::from_utf8(body) else {
        return fault(FAULT_BAD_REQUEST, "request body is not UTF-8");
    };
    let script = path.strip_prefix('/').unwrap_or(path);
    let Some(method) = sl_wire::xmlrpc::method_name(text) else {
        return fault(FAULT_BAD_REQUEST, "request is not an XML-RPC call");
    };
    let economy = &core.economy;
    let result = match (script, method.as_str()) {
        (CURRENCY_HELPER_PATH, GET_CURRENCY_QUOTE_METHOD) => parse_currency_quote_request(text)
            .map(|request| build_currency_quote_response(&economy.quote(&request))),
        (CURRENCY_HELPER_PATH, BUY_CURRENCY_METHOD) => {
            parse_buy_currency_request(text).map(|request| {
                let outcome = publish(core, economy.buy_currency(&request));
                build_buy_currency_response(&outcome)
            })
        }
        (LAND_TOOL_HELPER_PATH, PREFLIGHT_BUY_LAND_PREP_METHOD) => {
            parse_preflight_land_prep_request(text).map(|request| {
                build_preflight_land_prep_response(&economy.preflight_land(&request))
            })
        }
        (LAND_TOOL_HELPER_PATH, BUY_LAND_PREP_METHOD) => {
            parse_buy_land_prep_request(text).map(|request| {
                let outcome = publish(core, economy.buy_land(&request));
                build_buy_land_prep_response(&outcome)
            })
        }
        (_, other) => {
            return fault(
                FAULT_UNKNOWN_METHOD,
                &format!("{script} does not implement {other}"),
            );
        }
    };
    match result {
        Ok(xml) => HttpAnswer::ok(XML_RPC_CONTENT_TYPE, xml),
        Err(XmlRpcError::UnexpectedMethod { method }) => {
            fault(FAULT_UNKNOWN_METHOD, &format!("unexpected method {method}"))
        }
        Err(error) => {
            tracing::debug!("unparsable helper request on {script}: {error}");
            fault(FAULT_BAD_REQUEST, &error.to_string())
        }
    }
}

/// Publishes an accepted purchase and maps the outcome to the payload-less
/// commit response.
fn publish(
    core: &Arc<GridCore>,
    outcome: HelperOutcome<crate::economy_policy::EconomyEvent>,
) -> HelperOutcome<()> {
    match outcome {
        HelperOutcome::Ok(event) => {
            tracing::info!("economy: {event:?}");
            // Only lagging subscribers error; the purchase stands regardless.
            drop(core.economy_tx.send(event));
            HelperOutcome::Ok(())
        }
        HelperOutcome::Failed(failure) => HelperOutcome::Failed(failure),
    }
}

/// An XML-RPC fault answer (HTTP 200, as XML-RPC prescribes).
fn fault(code: i32, message: &str) -> HttpAnswer {
    HttpAnswer::ok(XML_RPC_CONTENT_TYPE, build_fault(code, message))
}

#[cfg(test)]
mod test {
    use super::is_helper_path;

    #[test]
    fn helper_paths_are_recognised() {
        assert!(is_helper_path("/currency.php"));
        assert!(is_helper_path("landtool.php"));
        assert!(!is_helper_path("/"));
        assert!(!is_helper_path("/sim/1/currency.php"));
    }
}
