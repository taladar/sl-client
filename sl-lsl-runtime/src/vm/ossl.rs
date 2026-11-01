//! OpenSim's own library families — OSSL (`os*`), LightShare (`ls*`) and MOD
//! (`mod*`) — as its engines gate them: the region's `[OSSL]` configuration
//! ([`Settings`]), what the script's owner is in the region
//! ([`Standing`]), and the check each call passes first (`admit`), with
//! the words of a denial and of a warning as the local OpenSim said them
//! (book chapter `gridspec/lsl.md`, § OSSL; `OSSL_Api.CheckThreatLevel`).
//!
//! Second Life compiles none of it ([`Dialect`](crate::Dialect)), so only an
//! OpenSim flavour ever gets here.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use crate::library::dotnet::canonical_uuid;
use crate::library::{Builtin, CallError, Family, Gate, ThreatLevel};
use crate::value::{NULL_KEY, Value};
use crate::vm::ScriptCtx;

/// The stock `config-include/osslDefaultEnable.ini` the local OpenSim runs,
/// vendored (provenance in `opensim/README.md`).
const STOCK_INI: &str = include_str!("../../opensim/osslDefaultEnable.ini");

/// [`Settings::stock`], parsed once.
static STOCK: LazyLock<Settings> = LazyLock::new(|| Settings::from_ini(STOCK_INI));

/// The stock configuration, shared.
pub(crate) fn stock_ossl() -> &'static Settings {
    &STOCK
}

/// Who an `Allow_` line may name, besides avatar keys (`AllowedControlFlags`
/// in `OSSL_Api.cs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Allowed {
    /// The owner of the parcel the prim is over (`PARCEL_OWNER`).
    ParcelOwner,
    /// A script whose group is the parcel's, when the parcel has one
    /// (`PARCEL_GROUP_MEMBER`).
    ParcelGroupMember,
    /// An estate manager who is not the estate owner (`ESTATE_MANAGER`).
    EstateManager,
    /// The estate owner (`ESTATE_OWNER`).
    EstateOwner,
    /// A god with god mode on in the region (`ACTIVE_GOD`).
    ActiveGod,
    /// Any god, an administrator (`GOD`).
    God,
    /// A grid god (`GRID_GOD`).
    GridGod,
}

/// What one function's `Allow_` and `Creators_` lines say, when at least
/// one of them is not blank.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Permit {
    /// Allowed for everyone (`Allow_<name> = true`).
    pub everyone: bool,
    /// Allowed for no one (`Allow_<name> = false`, whatever a `Creators_`
    /// line says: OpenSim does not read one then).
    pub disabled: bool,
    /// The kinds of owner allowed.
    pub allowed: Vec<Allowed>,
    /// The owners allowed by key.
    pub owners: Vec<String>,
    /// The script creators allowed by key (`Creators_<name>`).
    pub creators: Vec<String>,
}

impl Permit {
    /// Whether the lines allow no one: OpenSim's "disabled in region
    /// configuration".
    const fn is_nobody(&self) -> bool {
        self.disabled
            || !self.everyone
                && self.allowed.is_empty()
                && self.owners.is_empty()
                && self.creators.is_empty()
    }
}

/// Which of OpenSim's own families a region runs at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Families {
    /// `AllowOSFunctions`: OSSL.
    pub ossl: bool,
    /// `AllowLightShareFunctions`.
    pub lightshare: bool,
    /// `AllowMODFunctions`.
    pub modules: bool,
}

/// A region's `[OSSL]` configuration: which families are on, the threat
/// level a function without an `Allow_` line must not exceed, and each
/// function's `Allow_` and `Creators_` lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Which families run.
    pub families: Families,
    /// `PermissionErrorToOwner`: a denial goes to the owner alone, rather
    /// than on `DEBUG_CHANNEL` to everyone near.
    pub permission_error_to_owner: bool,
    /// `OSFunctionThreatLevel`.
    pub threat_level: ThreatLevel,
    /// Each function's lines, by the name they are written under; a function
    /// with none is judged by its threat level.
    pub permits: BTreeMap<String, Permit>,
    /// The values OpenSim would warn about and ignore, as `key = value`: an
    /// `Allow_` token that is neither a known word nor a key, an unknown
    /// threat level, an include this reader does not follow.
    pub ignored: Vec<String>,
}

impl Default for Settings {
    /// OpenSim's built-in defaults, with no configuration file at all:
    /// everything on, `VeryLow`, no `Allow_` lines.
    fn default() -> Self {
        Self {
            families: Families {
                ossl: true,
                lightshare: true,
                modules: true,
            },
            permission_error_to_owner: false,
            threat_level: ThreatLevel::VeryLow,
            permits: BTreeMap::new(),
            ignored: Vec::new(),
        }
    }
}

impl Settings {
    /// The stock `osslDefaultEnable.ini` the local OpenSim runs:
    /// `VeryLow`, every family on, and its `Allow_` lines.
    #[must_use]
    pub fn stock() -> Self {
        STOCK.clone()
    }

    /// Read an OpenSim configuration's `[OSSL]` section as OpenSim does:
    /// `key = value` lines, `;` comments (inline too), `${OSSL|name}`
    /// replaced by that key's value, quotes around a value dropped. What
    /// OpenSim would ignore with a warning lands in [`Self::ignored`].
    #[must_use]
    pub fn from_ini(text: &str) -> Self {
        let mut values: Vec<(String, String)> = Vec::new();
        let mut in_ossl = false;
        for line in text.lines() {
            let line = line.split(';').next().unwrap_or_default().trim();
            if let Some(section) = line
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
            {
                in_ossl = section == "OSSL";
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_owned();
            let value = value.trim().trim_matches('"').to_owned();
            if in_ossl {
                values.push((key, value));
            } else if !key.is_empty() {
                values.push((format!("[outside OSSL] {key}"), value));
            }
        }
        let mut config = Self::default();
        let mut expanded: BTreeMap<String, String> = BTreeMap::new();
        for (key, value) in values {
            if let Some(outside) = key.strip_prefix("[outside OSSL] ") {
                config.ignored.push(format!("{outside} = {value}"));
                continue;
            }
            let value = expand(&value, &expanded);
            let _old = expanded.insert(key.clone(), value.clone());
            config.read(&key, &value);
        }
        config
    }

    /// Take in one `key = value` line of the section.
    fn read(&mut self, key: &str, value: &str) {
        let flag = || value.trim().eq_ignore_ascii_case("true");
        match key {
            "AllowOSFunctions" => self.families.ossl = flag(),
            "AllowLightShareFunctions" => self.families.lightshare = flag(),
            "AllowMODFunctions" => self.families.modules = flag(),
            "PermissionErrorToOwner" => self.permission_error_to_owner = flag(),
            "OSFunctionThreatLevel" => match ThreatLevel::named(value.trim()) {
                Some(level) => self.threat_level = level,
                None => self.ignored.push(format!("{key} = {value}")),
            },
            _ if key.starts_with("Include-") => self.ignored.push(format!("{key} = {value}")),
            _ => {
                // A blank line is no line: the function is judged by its
                // threat level, unless its other line says otherwise.
                if value.trim().is_empty() {
                    return;
                }
                if let Some(name) = key.strip_prefix("Allow_") {
                    let permit = self.permits.entry(name.to_owned()).or_default();
                    match parse_bool(value) {
                        Some(true) => permit.everyone = true,
                        Some(false) => permit.disabled = true,
                        None => {
                            for token in value.split(',') {
                                if let Some(bad) = allow_token(permit, token) {
                                    self.ignored.push(format!("{key} = {bad}"));
                                }
                            }
                        }
                    }
                } else if let Some(name) = key.strip_prefix("Creators_") {
                    let permit = self.permits.entry(name.to_owned()).or_default();
                    for token in value
                        .split(',')
                        .map(str::trim)
                        .filter(|token| !token.is_empty())
                    {
                        match canonical_key(token) {
                            Some(id) => permit.creators.push(id),
                            None => self.ignored.push(format!("{key} = {token}")),
                        }
                    }
                }
            }
        }
    }

    /// Why the region does not allow the function checked under `name` at
    /// `level` for a script whose owner stands as `standing`, as OpenSim
    /// words it (`CheckThreatLevelTest`); [`None`] when it does.
    #[must_use]
    pub fn refusal(&self, name: &str, level: ThreatLevel, standing: &Standing) -> Option<String> {
        let Some(permit) = self.permits.get(name) else {
            return (level > self.threat_level).then(|| {
                format!(
                    "{name} permission denied.  Allowed threat level is {} but function threat \
                     level is {}.",
                    self.threat_level.name(),
                    level.name()
                )
            });
        };
        if permit.everyone && !permit.disabled {
            return None;
        }
        if permit.is_nobody() {
            return Some(format!("{name} disabled in region configuration"));
        }
        if permit.owners.contains(&standing.owner) {
            return None;
        }
        // OpenSim's `ESTATE_MANAGER` is a manager who is not the owner.
        let holds = |allowed: Allowed| {
            standing.is(allowed)
                && !(allowed == Allowed::EstateManager && standing.is(Allowed::EstateOwner))
        };
        if permit.allowed.iter().copied().any(holds) {
            return None;
        }
        if permit.creators.is_empty() {
            return Some(format!("{name} permission denied."));
        }
        if !permit.creators.contains(&standing.creator) {
            return Some(format!(
                "{name} permission denied. Script creator is not in the list of users allowed to \
                 execute this function and prim owner also has no permission."
            ));
        }
        if standing.creator != standing.owner && standing.modifiable {
            return Some(format!(
                "{name} permission denied. Script creator is not prim owner."
            ));
        }
        None
    }
}

/// `value` with every `${OSSL|key}` replaced by that key's value so far
/// (an unknown key by nothing).
fn expand(value: &str, known: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find("${OSSL|") {
        out.push_str(rest.get(..at).unwrap_or_default());
        let after = rest.get(at.saturating_add(7)..).unwrap_or_default();
        let Some(end) = after.find('}') else {
            out.push_str(rest.get(at..).unwrap_or_default());
            return out;
        };
        let key = after.get(..end).unwrap_or_default();
        out.push_str(known.get(key).map_or("", String::as_str));
        rest = after.get(end.saturating_add(1)..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// A value as .NET's `bool.TryParse` reads it.
fn parse_bool(value: &str) -> Option<bool> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("true") {
        Some(true)
    } else if value.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// A key in its canonical lower-case hyphenated form, or [`None`] for text
/// that is no key or is the null key, which OpenSim skips.
fn canonical_key(token: &str) -> Option<String> {
    canonical_uuid(token.trim()).filter(|id| id != NULL_KEY)
}

/// Add one `Allow_` token to `permit`; the token back when OpenSim would
/// warn about it.
fn allow_token(permit: &mut Permit, token: &str) -> Option<String> {
    let token = token.trim();
    let allowed = match token.to_ascii_uppercase().as_str() {
        "" => return None,
        "PARCEL_OWNER" => Allowed::ParcelOwner,
        "PARCEL_GROUP_MEMBER" => Allowed::ParcelGroupMember,
        "ESTATE_MANAGER" => Allowed::EstateManager,
        "ESTATE_OWNER" => Allowed::EstateOwner,
        "ACTIVE_GOD" => Allowed::ActiveGod,
        "GOD" => Allowed::God,
        "GRID_GOD" => Allowed::GridGod,
        _ => {
            return match canonical_uuid(token) {
                Some(id) if id == NULL_KEY => None,
                Some(id) => {
                    permit.owners.push(id);
                    None
                }
                None => Some(token.to_owned()),
            };
        }
    };
    if !permit.allowed.contains(&allowed) {
        permit.allowed.push(allowed);
    }
    None
}

/// What a script's owner is in the region where the script runs: what an
/// `Allow_` line may ask of it. The host answers it
/// ([`Host::standing`](crate::vm::Host::standing)).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Standing {
    /// The key of the script's owner.
    pub owner: String,
    /// The key of the script's creator.
    pub creator: String,
    /// Whether the next owner may modify the script (`Creators_` refuses a
    /// modifiable script whose creator is not its owner).
    pub modifiable: bool,
    /// What the owner is, of what an `Allow_` line may name: the parcel's
    /// owner or of its group, the estate's owner or one of its managers
    /// (an owner who is a manager too is both), a god.
    pub roles: Vec<Allowed>,
}

impl Standing {
    /// Whether the owner is `role`.
    #[must_use]
    pub fn is(&self, role: Allowed) -> bool {
        self.roles.contains(&role)
    }
}

/// The simulator a script runs on, as OpenSim's identity readers answer it
/// (`osGetSimulatorVersion`, `osGetGridName`, …). The host answers it
/// ([`Host::simulator`](crate::vm::Host::simulator)).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Simulator {
    /// The simulator's version string (`sim_version`).
    pub version: String,
    /// The grid's name (`[GridInfoService] gridname`).
    pub grid_name: String,
    /// The grid's nickname (`[GridInfoService] gridnick`).
    pub grid_nick: String,
    /// The physics engine's type (`ubODE`).
    pub physics_engine_type: String,
    /// The physics engine's name and version (`ubODE 1.0-0.13.2`).
    pub physics_engine_name: String,
}

/// What a call is to do once its gate has been passed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Admitted {
    /// Run the function.
    Run,
    /// Skip it, returning this (a disabled LightShare or MOD function, which
    /// shouts and answers rather than throwing).
    Skip(Option<Value>),
}

/// The text OpenSim throws to end the event with an OSSL error, with the
/// `(OWNER)` mark that sends it to the owner alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thrown {
    /// The message: `OSSL Permission Error: …`, `OSSL Runtime Error: …`.
    pub text: String,
    /// Whether it goes to the owner alone (`PermissionErrorToOwner`).
    pub to_owner: bool,
}

/// What OpenSim does before it runs `builtin`: in an OpenSim flavour, the
/// family's switch and the function's permission, then a deprecated
/// function's warning (a shout and a second's sleep); nothing in Second
/// Life's, which compiles none of these.
///
/// # Errors
///
/// [`CallError::Thrown`] when the region does not allow the call: the event
/// ends there.
pub(crate) fn admit(builtin: &Builtin, ctx: &mut ScriptCtx<'_>) -> Result<Admitted, CallError> {
    if !ctx.flavour().is_opensim() || !builtin.family.is_opensim() {
        return Ok(Admitted::Run);
    }
    let thrown = |text: String, to_owner: bool| CallError::Thrown {
        id: builtin.id,
        thrown: Thrown { text, to_owner },
    };
    let config = ctx.ossl();
    match builtin.gate {
        Gate::Unchecked => {}
        Gate::Enabled => {
            if !config.families.ossl {
                return Err(thrown(
                    "OSSL Runtime Error: permission denied. All unsafe OSSL funtions disabled"
                        .to_owned(),
                    false,
                ));
            }
        }
        Gate::Threat { level, name } => {
            // OpenSim marks a denial for the owner with `(OWNER)`, which
            // both engines strip before they send it to the owner alone.
            let owner = config.permission_error_to_owner;
            if !config.families.ossl {
                return Err(thrown(
                    "OSSL Permission Error: All unsafe OSSL funtions disabled".to_owned(),
                    owner,
                ));
            }
            if let Some(reason) = config.refusal(name, level, &ctx.standing()) {
                return Err(thrown(format!("OSSL Permission Error: {reason}"), owner));
            }
        }
        Gate::LightShare => {
            if !config.families.lightshare {
                ctx.shout_error("LightShare functions are not enabled.".to_owned());
                return Ok(Admitted::Skip(builtin.ret.map(Value::default_of)));
            }
        }
        Gate::Mod => {
            if !config.families.modules {
                ctx.shout_error("Module command functions not enabled".to_owned());
                // `modSendCommand` answers the null key.
                return Ok(Admitted::Skip(
                    builtin
                        .ret
                        .map(|_ty| Value::Key(crate::value::NULL_KEY.to_owned())),
                ));
            }
        }
    }
    if let (Some(replacement), Family::Ossl) = (builtin.replacement, builtin.family) {
        warn(
            ctx,
            format!(
                "Use of function {} is deprecated. Use {replacement} instead.",
                builtin.name
            ),
        );
    }
    Ok(Admitted::Run)
}

/// OpenSim's `OSSLShoutError`: shout `text` on `DEBUG_CHANNEL` and sleep
/// the script a second; the call carries on.
pub(crate) fn warn(ctx: &mut ScriptCtx<'_>, text: String) {
    ctx.shout_error(text);
    ctx.sleep(1.0);
}
