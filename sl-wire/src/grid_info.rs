//! The `get_grid_info` surface: the login URI's sibling a viewer's grid
//! manager queries to learn a grid's name, nickname, and helper pages before
//! it ever logs in.
//!
//! OpenSim serves it three ways from the login host (`GridInfoHandlers.cs`):
//! `GET /get_grid_info` as a flat `<gridinfo><key>value</key>…</gridinfo>`
//! document, the XML-RPC method `get_grid_info` (a struct of the same
//! strings), and a script-only JSON variant (not modelled here — no viewer
//! consumes it). The key set is whatever the grid's `[GridInfoService]`
//! section holds, so [`GridInfo`] is an ordered list of string entries with
//! typed accessors for the keys Firestorm's grid manager reads
//! (`fsgridhandler.cpp`, `LLGridManager::gridInfoResponderCB`).

use std::collections::HashMap;

use sl_llsd::{Llsd, parse_guarded_xml, push_escaped};

use crate::xmlrpc::{XmlRpcError, XmlRpcResponse, build_method_response, parse_method_response};

/// The path, relative to the login URI, the viewer appends to fetch the XML
/// form (`<login-uri>/get_grid_info`).
pub const GRID_INFO_PATH: &str = "get_grid_info";

/// The XML-RPC method name of the struct form, POSTed to the login URI.
pub const GRID_INFO_METHOD: &str = "get_grid_info";

/// The login URI (`login`).
pub const KEY_LOGIN: &str = "login";
/// The human-readable grid name (`gridname`).
pub const KEY_GRIDNAME: &str = "gridname";
/// The short grid nickname (`gridnick`).
pub const KEY_GRIDNICK: &str = "gridnick";
/// The welcome / login-splash page (`welcome`).
pub const KEY_WELCOME: &str = "welcome";
/// The economy helper URI base (`economy`); `helperuri` is its alias.
pub const KEY_ECONOMY: &str = "economy";
/// The alias of [`KEY_ECONOMY`] some grids emit (`helperuri`).
pub const KEY_HELPERURI: &str = "helperuri";
/// The "about this grid" page (`about`).
pub const KEY_ABOUT: &str = "about";
/// The account-registration page (`register`).
pub const KEY_REGISTER: &str = "register";
/// The forgot-password page (`password`).
pub const KEY_PASSWORD: &str = "password";
/// The help page (`help`).
pub const KEY_HELP: &str = "help";
/// The server platform name (`platform`), e.g. `OpenSim`.
pub const KEY_PLATFORM: &str = "platform";
/// A free-text message of the day (`message`).
pub const KEY_MESSAGE: &str = "message";
/// The web-search endpoint (`search`).
pub const KEY_SEARCH: &str = "search";
/// The HyperGrid gatekeeper URI (`gatekeeper`).
pub const KEY_GATEKEEPER: &str = "gatekeeper";
/// The HyperGrid user-agent service URI (`uas`).
pub const KEY_UAS: &str = "uas";
/// The web-profile base URL (`web_profile_url`; `profileuri` is an alias).
pub const KEY_WEB_PROFILE_URL: &str = "web_profile_url";
/// The alias of [`KEY_WEB_PROFILE_URL`] (`profileuri`).
pub const KEY_PROFILEURI: &str = "profileuri";

/// A grid's `get_grid_info` entries: an ordered list of string key/value
/// pairs (order is preserved so a builder's output is byte-stable and an
/// unknown key is never dropped).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GridInfo {
    /// The entries in insertion / document order.
    entries: Vec<(String, String)>,
}

impl GridInfo {
    /// An empty grid info.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Sets `key` to `value`, replacing an existing entry in place (keeping
    /// its position) or appending a new one.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            entry.1 = value;
        } else {
            self.entries.push((key, value));
        }
    }

    /// Builder-style [`insert`](Self::insert).
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.insert(key, value);
        self
    }

    /// The value of `key`, if present.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Iterates the entries in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Whether no entry is present.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The number of entries.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// The `login` URI, if present and parseable.
    #[must_use]
    pub fn login_uri(&self) -> Option<url::Url> {
        self.url(KEY_LOGIN)
    }

    /// The `gridname`.
    #[must_use]
    pub fn grid_name(&self) -> Option<&str> {
        self.get(KEY_GRIDNAME)
    }

    /// The `gridnick`.
    #[must_use]
    pub fn grid_nick(&self) -> Option<&str> {
        self.get(KEY_GRIDNICK)
    }

    /// The economy helper URI: `economy`, else its alias `helperuri` (the
    /// precedence Firestorm applies — `economy` overrides `helperuri` when
    /// both are present).
    #[must_use]
    pub fn helper_uri(&self) -> Option<url::Url> {
        self.url(KEY_ECONOMY).or_else(|| self.url(KEY_HELPERURI))
    }

    /// The `welcome` page.
    #[must_use]
    pub fn welcome(&self) -> Option<url::Url> {
        self.url(KEY_WELCOME)
    }

    /// The `about` page.
    #[must_use]
    pub fn about(&self) -> Option<url::Url> {
        self.url(KEY_ABOUT)
    }

    /// The `register` page.
    #[must_use]
    pub fn register(&self) -> Option<url::Url> {
        self.url(KEY_REGISTER)
    }

    /// The `password` (forgot-password) page.
    #[must_use]
    pub fn password(&self) -> Option<url::Url> {
        self.url(KEY_PASSWORD)
    }

    /// The `help` page.
    #[must_use]
    pub fn help(&self) -> Option<url::Url> {
        self.url(KEY_HELP)
    }

    /// The `search` endpoint.
    #[must_use]
    pub fn search(&self) -> Option<url::Url> {
        self.url(KEY_SEARCH)
    }

    /// The HyperGrid `gatekeeper` URI.
    #[must_use]
    pub fn gatekeeper(&self) -> Option<url::Url> {
        self.url(KEY_GATEKEEPER)
    }

    /// The HyperGrid `uas` URI.
    #[must_use]
    pub fn uas(&self) -> Option<url::Url> {
        self.url(KEY_UAS)
    }

    /// The web-profile base: `web_profile_url`, else its alias `profileuri`.
    #[must_use]
    pub fn web_profile_url(&self) -> Option<url::Url> {
        self.url(KEY_WEB_PROFILE_URL)
            .or_else(|| self.url(KEY_PROFILEURI))
    }

    /// The `platform` name.
    #[must_use]
    pub fn platform(&self) -> Option<&str> {
        self.get(KEY_PLATFORM)
    }

    /// The `message` of the day.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.get(KEY_MESSAGE)
    }

    /// Parses the value of `key` as a URL; an absent, empty, or unparsable
    /// value is `None` (grid configs routinely carry placeholder text here).
    fn url(&self, key: &str) -> Option<url::Url> {
        self.get(key)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| url::Url::parse(value).ok())
    }
}

impl<K: Into<String>, V: Into<String>> FromIterator<(K, V)> for GridInfo {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut info = Self::new();
        for (key, value) in iter {
            info.insert(key, value);
        }
        info
    }
}

/// Builds the `GET /get_grid_info` body: `<gridinfo>` with one child element
/// per entry, values XML-escaped, in entry order (OpenSim's exact shape).
#[must_use]
pub fn build_grid_info_xml(info: &GridInfo) -> String {
    let mut out = String::from("<gridinfo>");
    for (key, value) in info.iter() {
        out.push('<');
        out.push_str(key);
        out.push('>');
        push_escaped(&mut out, value);
        out.push_str("</");
        out.push_str(key);
        out.push('>');
    }
    out.push_str("</gridinfo>");
    out
}

/// Parses a `<gridinfo>` document into its entries, in document order. Every
/// child element is kept, known key or not.
///
/// # Errors
///
/// Returns [`XmlRpcError::Xml`] if the body is not well-formed XML or is
/// nested past [`sl_llsd::MAX_NESTING_DEPTH`], and [`XmlRpcError::NotXmlRpc`]
/// if the root element is not `<gridinfo>`.
pub fn parse_grid_info_xml(xml: &str) -> Result<GridInfo, XmlRpcError> {
    let document = parse_guarded_xml(xml)?;
    let root = document.root_element();
    if !root.has_tag_name("gridinfo") {
        return Err(XmlRpcError::NotXmlRpc);
    }
    Ok(root
        .children()
        .filter(roxmltree::Node::is_element)
        .map(|node| {
            (
                node.tag_name().name().to_owned(),
                node.text().unwrap_or_default().to_owned(),
            )
        })
        .collect())
}

/// Builds the XML-RPC `get_grid_info` response: one struct of string members
/// (emitted in sorted key order, like every struct the
/// [`xmlrpc`](crate::xmlrpc) codec writes).
#[must_use]
pub fn build_grid_info_xmlrpc_response(info: &GridInfo) -> String {
    let map: HashMap<String, Llsd> = info
        .iter()
        .map(|(key, value)| (key.to_owned(), Llsd::String(value.to_owned())))
        .collect();
    build_method_response(&[Llsd::Map(map)])
}

/// Parses an XML-RPC `get_grid_info` response into its entries (sorted by
/// key, since a struct carries no order).
///
/// # Errors
///
/// Returns [`XmlRpcError::Xml`] / [`XmlRpcError::NotXmlRpc`] for a malformed
/// document and [`XmlRpcError::Llsd`] when the response is a fault or its
/// first parameter is not a struct.
pub fn parse_grid_info_xmlrpc_response(xml: &str) -> Result<GridInfo, XmlRpcError> {
    let response = parse_method_response(xml)?;
    let Some(Llsd::Map(map)) = response.first_param() else {
        let value = match response {
            XmlRpcResponse::Fault { message, .. } => format!("fault: {message}"),
            XmlRpcResponse::Params(_) => "non-struct response".to_owned(),
        };
        return Err(sl_llsd::LlsdError::MalformedField {
            field: "get_grid_info",
            value,
        }
        .into());
    };
    let mut entries: Vec<(String, String)> = map
        .iter()
        .map(|(key, value)| {
            let text = match value {
                Llsd::String(text) => text.clone(),
                other => other.as_str().map(str::to_owned).unwrap_or_default(),
            };
            (key.clone(), text)
        })
        .collect();
    entries.sort();
    Ok(entries.into_iter().collect())
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::{
        GridInfo, KEY_ECONOMY, KEY_GRIDNAME, KEY_GRIDNICK, KEY_HELPERURI, KEY_LOGIN, KEY_PLATFORM,
        build_grid_info_xml, build_grid_info_xmlrpc_response, parse_grid_info_xml,
        parse_grid_info_xmlrpc_response,
    };
    use crate::xmlrpc::XmlRpcError;

    /// The literal document the local OpenSim standalone answers with.
    const OPENSIM_SAMPLE: &str = "<gridinfo><platform>OpenSim</platform>\
<login>http://127.0.0.1:9000/</login>\
<gridname>the lost continent of hippo</gridname>\
<gridnick>hippogrid</gridnick>\
<welcome>http://127.0.0.1/welcome?a=1&amp;b=2</welcome></gridinfo>";

    #[test]
    fn opensim_sample_parses_in_order_with_typed_accessors() -> Result<(), XmlRpcError> {
        let info = parse_grid_info_xml(OPENSIM_SAMPLE)?;
        assert_eq!(info.len(), 5);
        assert_eq!(info.platform(), Some("OpenSim"));
        assert_eq!(
            info.login_uri().map(String::from),
            Some("http://127.0.0.1:9000/".to_owned())
        );
        assert_eq!(info.grid_name(), Some("the lost continent of hippo"));
        assert_eq!(info.grid_nick(), Some("hippogrid"));
        assert_eq!(
            info.welcome().map(String::from),
            Some("http://127.0.0.1/welcome?a=1&b=2".to_owned())
        );
        assert!(info.helper_uri().is_none());
        assert_eq!(
            info.iter().next(),
            Some((KEY_PLATFORM, "OpenSim")),
            "document order is preserved"
        );
        Ok(())
    }

    #[test]
    fn xml_round_trips_byte_for_byte() -> Result<(), XmlRpcError> {
        let info = parse_grid_info_xml(OPENSIM_SAMPLE)?;
        assert_eq!(build_grid_info_xml(&info), OPENSIM_SAMPLE);
        Ok(())
    }

    #[test]
    fn xmlrpc_form_round_trips() -> Result<(), XmlRpcError> {
        let info = GridInfo::new()
            .with(KEY_LOGIN, "http://grid.example/")
            .with(KEY_GRIDNAME, "Example & Co")
            .with(KEY_GRIDNICK, "example");
        let parsed = parse_grid_info_xmlrpc_response(&build_grid_info_xmlrpc_response(&info))?;
        assert_eq!(parsed.grid_name(), Some("Example & Co"));
        assert_eq!(parsed.grid_nick(), Some("example"));
        assert_eq!(parsed.login_uri(), info.login_uri());
        Ok(())
    }

    #[test]
    fn economy_wins_over_helperuri_and_insert_replaces_in_place() {
        let mut info = GridInfo::new()
            .with(KEY_HELPERURI, "http://helper.example/")
            .with(KEY_ECONOMY, "http://economy.example/");
        assert_eq!(
            info.helper_uri().map(String::from),
            Some("http://economy.example/".to_owned())
        );
        info.insert(KEY_HELPERURI, "http://other.example/");
        assert_eq!(info.len(), 2);
        assert_eq!(
            info.iter().next(),
            Some((KEY_HELPERURI, "http://other.example/"))
        );
        let only_alias: GridInfo = [(KEY_HELPERURI, "http://helper.example/")]
            .into_iter()
            .collect();
        assert_eq!(
            only_alias.helper_uri().map(String::from),
            Some("http://helper.example/".to_owned())
        );
    }

    #[test]
    fn junk_values_and_documents_are_handled() {
        let info = GridInfo::new().with(KEY_LOGIN, "not a url");
        assert!(info.login_uri().is_none());
        assert!(matches!(
            parse_grid_info_xml("<other/>"),
            Err(XmlRpcError::NotXmlRpc)
        ));
        assert!(matches!(
            parse_grid_info_xml("<<"),
            Err(XmlRpcError::Xml(_))
        ));
        assert!(matches!(
            parse_grid_info_xmlrpc_response(&crate::xmlrpc::build_fault(1, "x")),
            Err(XmlRpcError::Llsd(_))
        ));
    }
}
