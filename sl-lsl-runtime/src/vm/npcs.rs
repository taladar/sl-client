//! What OpenSim's own avatar functions reach (`server-lsl-lib-ossl-avatars`):
//! the NPCs a script creates and drives (`osNpc*`), what the region keeps of
//! an avatar's health, and an avatar's appearance as OpenSim writes it into a
//! notecard (`osOwnerSaveAppearance`) and reads it back (`osNpcCreate`).
//!
//! The host keeps the NPCs — where they stand, where they walk, what they
//! play — and shows them to the viewers; which call may drive which NPC, and
//! what each call does to one, follow `OSSL_Api.cs` and OpenSim's
//! `NPCModule.cs`, and live in the library.

use sl_types::chat::ChatVolume;
use sl_types::lsl::{Rotation, Vector};

/// An NPC in the region, as the library reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Npc {
    /// Its key, an agent key of its own.
    pub key: String,
    /// Its name, `First Last` (`osGetNPCList`).
    pub name: String,
    /// The avatar that owns it, or [`None`] for one created unowned
    /// (`OS_NPC_NOT_OWNED`), which any script may drive.
    pub owner: Option<String>,
    /// Where it stands in the region (`osNpcGetPos`).
    pub position: Vector,
    /// How it is turned (`osNpcGetRot`).
    pub rotation: Rotation,
}

/// What `osNpcCreate` asks the region for.
#[derive(Debug, Clone, PartialEq)]
pub struct NpcSpec {
    /// Its first name.
    pub first: String,
    /// Its last name.
    pub last: String,
    /// Where it appears.
    pub position: Vector,
    /// The avatar that owns it, or [`None`] for an unowned one.
    pub owner: Option<String>,
    /// Whether sensors detect it as an agent (`OS_NPC_SENSE_AS_AGENT`).
    pub sense_as_agent: bool,
    /// The group it is active in (`OS_NPC_OBJECT_GROUP`), the creating
    /// prim's, or [`None`].
    pub group: Option<String>,
    /// The title over its name tag: `- NPC -` for a named NPC.
    pub group_title: String,
    /// How it looks, read from a notecard; [`None`] for the default avatar.
    pub appearance: Option<Appearance>,
}

/// Why the region created no NPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcRefusal {
    /// The script's owner may not rez at the position (OpenSim's
    /// `CanRezObject`): `no permission to rez NPC at requested location`.
    NoRez,
    /// The region holds as many NPCs as it allows (`MaxNumberNPCsPerScene`):
    /// OpenSim answers the null key without a word.
    Full,
}

/// What a script makes an NPC do.
#[derive(Debug, Clone, PartialEq)]
pub enum NpcAction {
    /// Walk, or fly, to `target` (`osNpcMoveTo`, `osNpcMoveToTarget`).
    MoveTo {
        /// Where to.
        target: Vector,
        /// Never fly there (`OS_NPC_NO_FLY`).
        no_fly: bool,
        /// Land at the end of a flight (`OS_NPC_LAND_AT_TARGET`).
        land_at_target: bool,
        /// Run rather than walk (`OS_NPC_RUNNING`).
        running: bool,
    },
    /// Stop where it is (`osNpcStopMoveToTarget`).
    StopMoving,
    /// Turn to `rotation` (`osNpcSetRot`).
    Turn(Rotation),
    /// Stand up (`osNpcStand`).
    Stand,
    /// Leave the region (`osNpcRemove`).
    Remove,
    /// Touch the prim keyed so, as a viewer's click — a grab, a grab update
    /// and a release — would, if the prim or its root takes touches
    /// (`osNpcTouch`).
    Touch(String),
    /// The "about" text of its profile (`osNpcSetProfileAbout`).
    About(String),
    /// The picture of its profile, a texture's key (`osNpcSetProfileImage`).
    Image(String),
    /// Look as `appearance` says (`osNpcLoadAppearance`).
    Appearance(Box<Appearance>),
    /// Look at a prim or an avatar, or stop looking (`osNpcLookAt`): the
    /// `LookAt` viewer effect, `kind` its `NPCLOOKAT_*` type.
    LookAt {
        /// The look-at type (`NPCLOOKAT_NONE` 0 … `NPCLOOKAT_CLEAR` 10).
        kind: i32,
        /// The prim or avatar looked at, or `NULL_KEY`.
        target: String,
        /// The point looked at, from the target.
        offset: Vector,
    },
}

/// A line an NPC says (`osNpcSay`, `osNpcShout`, `osNpcWhisper`,
/// `osNpcSayTo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcSpeech {
    /// How far it carries.
    pub volume: ChatVolume,
    /// The channel.
    pub channel: i32,
    /// The text.
    pub text: String,
    /// The one avatar or prim it is said to (`osNpcSayTo`), wherever in the
    /// region; [`None`] for a line anybody in earshot hears.
    pub to: Option<String>,
}

/// What the region keeps of an avatar's health (`osGetHealth`,
/// `osGetHealRate`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vitals {
    /// Its health, 100 at full.
    pub health: f32,
    /// What it heals by in a second while under 100.
    pub heal_rate: f32,
}

impl Default for Vitals {
    /// A presence's as OpenSim makes one: full health, healing half a point a
    /// second.
    fn default() -> Self {
        Self {
            health: 100.0,
            heal_rate: 0.5,
        }
    }
}

/// Where a scripted drop puts a worn object (`osDropAttachmentAt`).
#[derive(Debug, Clone, PartialEq)]
pub struct DropAt {
    /// The position in the region.
    pub position: Vector,
    /// The rotation.
    pub rotation: Rotation,
}

/// The texture OpenSim writes for an avatar texture slot nothing fills
/// (`AppearanceManager.DEFAULT_AVATAR_TEXTURE`), and always for slot 0.
pub const DEFAULT_AVATAR_TEXTURE: &str = "c228d1cf-4b5d-4ba8-84f4-899a0796aa97";

/// The **default avatar's** transmitted visual params — the body a grid hands
/// an account that has no stored appearance yet ("Ruth").
///
/// Verbatim from OpenSim's `AvatarAppearance.SetDefaultParams`
/// (`OpenSim/Framework/AvatarAppearance.cs`, BSD-licensed), which is what the
/// local test grid actually sends, so a fixture NPC looks exactly like a
/// default OpenSim avatar. Setting every param to the midpoint of its own range
/// instead — the obvious-looking alternative — produces a badly distorted body,
/// because the ranges are not centred on anything an avatar wants to be.
///
/// The vector is 218 bytes and a receiver reads it positionally against its own
/// [transmitted param list], which in the standard `avatar_lad.xml` is 253
/// params: exactly the 218 classic ones (every id below 10000), then the 33
/// physics params (10000–10032) and two more (11000, 11001). So the classic
/// half lands slot for slot and the rest falls back to each param's default —
/// which is what OpenSim itself relies on.
///
/// [transmitted param list]: https://wiki.secondlife.com/wiki/Appearance
pub const DEFAULT_VISUAL_PARAMS: [u8; 218] = [
    33, 61, 85, 23, 58, 127, 63, 85, 63, 42, 0, 85, 63, 36, 85, 95, 153, 63, 34, 0, 63, 109, 88,
    132, 63, 136, 81, 85, 103, 136, 127, 0, 150, 150, 150, 127, 0, 0, 0, 0, 0, 127, 0, 0, 255, 127,
    114, 127, 99, 63, 127, 140, 127, 127, 0, 0, 0, 191, 0, 104, 0, 0, 0, 0, 0, 0, 0, 0, 0, 145,
    216, 133, 0, 127, 0, 127, 170, 0, 0, 127, 127, 109, 85, 127, 127, 63, 85, 42, 150, 150, 150,
    150, 150, 150, 150, 25, 150, 150, 150, 0, 127, 0, 0, 144, 85, 127, 132, 127, 85, 0, 127, 127,
    127, 127, 127, 127, 59, 127, 85, 127, 127, 106, 47, 79, 127, 127, 204, 2, 141, 66, 0, 0, 127,
    127, 0, 0, 0, 0, 127, 0, 159, 0, 0, 178, 127, 36, 85, 131, 127, 127, 127, 153, 95, 0, 140, 75,
    27, 127, 127, 0, 150, 150, 198, 0, 0, 63, 30, 127, 165, 209, 198, 127, 127, 153, 204, 51, 51,
    255, 255, 255, 204, 0, 255, 150, 150, 150, 150, 150, 150, 150, 150, 150, 150, 0, 150, 150, 150,
    150, 150, 0, 127, 127, 150, 150, 150, 150, 150, 150, 150, 150, 0, 0, 150, 51, 132, 150, 150,
    150,
];

/// The avatar texture slots an appearance notecard lists
/// (`TEXTURE_COUNT_PV7`).
pub const NOTECARD_TEXTURES: usize = 29;

/// The legacy wearable slots an appearance notecard lists, every one empty
/// (`MAXWEARABLE_LEGACY`).
const NOTECARD_WEARABLES: usize = 15;

/// The index, among the transmitted visual parameters, of the shape's `male`
/// parameter (id 80): `osGetGender` reads it.
const MALE_PARAMETER: usize = 31;

/// The first and last HUD attachment points, which a save without HUDs
/// leaves out.
const HUD_POINTS: core::ops::RangeInclusive<i32> = 31..=38;

/// An object an avatar wears, as an appearance lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppearanceAttachment {
    /// The attachment point.
    pub point: i32,
    /// The inventory item it was worn from.
    pub item: String,
    /// The object's asset.
    pub asset: String,
}

/// An avatar's appearance, as OpenSim packs one into a notecard
/// (`AvatarAppearance.PackForNotecard`) and reads it back (`Unpack`).
#[derive(Debug, Clone, PartialEq)]
pub struct Appearance {
    /// The appearance's serial number.
    pub serial: i32,
    /// The avatar's height, in metres.
    pub height: f32,
    /// The hover height the avatar's viewer prefers (`aphz`).
    pub hover: f32,
    /// The texture of each avatar texture slot, by key, as many as the
    /// appearance has (at least [`NOTECARD_TEXTURES`] are written).
    pub textures: Vec<String>,
    /// The bake slots past Project Viewer 7's, as the texture entry's bytes
    /// OpenSim writes when one of them is baked (`te8`).
    pub extra_bakes: Option<Vec<u8>>,
    /// The transmitted visual parameters, one quantized byte each.
    pub visual_params: Vec<u8>,
    /// The objects worn.
    pub attachments: Vec<AppearanceAttachment>,
}

impl Appearance {
    /// Whether the shape is male, as `osGetGender` reads it: the `male`
    /// parameter past its middle. [`None`] for an appearance with too few
    /// parameters to say.
    #[must_use]
    pub fn is_male(&self) -> Option<bool> {
        self.visual_params
            .get(MALE_PARAMETER)
            .map(|byte| f32::from(*byte) / 255.0 > 0.5)
    }

    /// The notecard OpenSim writes of the appearance
    /// (`Util.GetFormattedXml(PackForNotecard(noHuds))`): the LLSD map in its
    /// key order, indented two spaces a level. A save without HUDs leaves out
    /// the objects worn on a HUD point.
    #[must_use]
    pub fn to_notecard(&self, include_huds: bool) -> String {
        let mut out = String::from("<llsd>\n  <map>\n");
        let field = |out: &mut String, key: &str, value: &str| {
            out.push_str("    <key>");
            out.push_str(key);
            out.push_str("</key>\n    ");
            out.push_str(value);
            out.push('\n');
        };
        field(
            &mut out,
            "serial",
            &format!("<integer>{}</integer>", self.serial),
        );
        field(
            &mut out,
            "height",
            &format!("<real>{}</real>", general(f64::from(self.height))),
        );
        field(
            &mut out,
            "aphz",
            &format!("<real>{}</real>", general(f64::from(self.hover))),
        );
        out.push_str("    <key>wearables</key>\n    <array>\n");
        for _ in 0..NOTECARD_WEARABLES {
            out.push_str("      <array></array>\n");
        }
        out.push_str("    </array>\n    <key>textures</key>\n    <array>\n");
        for slot in 0..NOTECARD_TEXTURES {
            let texture = if slot == 0 {
                DEFAULT_AVATAR_TEXTURE
            } else {
                self.textures
                    .get(slot)
                    .map_or(DEFAULT_AVATAR_TEXTURE, String::as_str)
            };
            out.push_str("      <uuid>");
            out.push_str(texture);
            out.push_str("</uuid>\n");
        }
        out.push_str("    </array>\n");
        if let Some(bakes) = &self.extra_bakes {
            field(
                &mut out,
                "te8",
                &format!("<binary>{}</binary>", base64(bakes)),
            );
        }
        field(
            &mut out,
            "visualparams",
            &format!("<binary>{}</binary>", base64(&self.visual_params)),
        );
        let worn: Vec<&AppearanceAttachment> = self
            .attachments
            .iter()
            .filter(|attachment| include_huds || !HUD_POINTS.contains(&attachment.point))
            .collect();
        if worn.is_empty() {
            out.push_str("    <key>attachments</key>\n    <array></array>\n");
        } else {
            out.push_str("    <key>attachments</key>\n    <array>\n");
            for attachment in worn {
                out.push_str("      <map>\n        <key>point</key>\n        <integer>");
                out.push_str(&attachment.point.to_string());
                out.push_str("</integer>\n        <key>item</key>\n        <uuid>");
                out.push_str(&attachment.item);
                out.push_str("</uuid>\n        <key>asset</key>\n        <uuid>");
                out.push_str(&attachment.asset);
                out.push_str("</uuid>\n      </map>\n");
            }
            out.push_str("    </array>\n");
        }
        out.push_str("  </map>\n</llsd>");
        out
    }

    /// The appearance a notecard holds, read as OpenSim's `Unpack` reads it
    /// over the default appearance: [`None`] for text that is no LLSD map,
    /// which OpenSim reports as an error processing the notecard.
    #[must_use]
    pub fn from_notecard(text: &str, default: &Self) -> Option<Self> {
        let document = sl_llsd::parse_llsd_xml(text.trim()).ok()?;
        if !matches!(document, sl_llsd::Llsd::Map(_)) {
            return None;
        }
        let mut appearance = default.clone();
        if let Some(serial) = document.get("serial").and_then(sl_llsd::Llsd::as_i32) {
            appearance.serial = serial;
        }
        if let Some(hover) = document.get("aphz").and_then(sl_llsd::Llsd::as_f32) {
            appearance.hover = hover;
        }
        if let Some(height) = document.get("height").and_then(sl_llsd::Llsd::as_f32) {
            appearance.height = height;
        }
        if let Some(bakes) = document.get("te8").and_then(sl_llsd::Llsd::as_binary) {
            appearance.extra_bakes = Some(bakes.to_vec());
        } else if let Some(textures) = document.get("textures").and_then(sl_llsd::Llsd::as_array) {
            for (slot, texture) in textures.iter().enumerate().take(NOTECARD_TEXTURES) {
                let key = match texture {
                    sl_llsd::Llsd::Uuid(id) => id.to_string(),
                    other => other.as_str().unwrap_or_default().to_ascii_lowercase(),
                };
                if appearance.textures.len() <= slot {
                    appearance
                        .textures
                        .resize(slot.saturating_add(1), DEFAULT_AVATAR_TEXTURE.to_owned());
                }
                if let Some(entry) = appearance.textures.get_mut(slot) {
                    *entry = key;
                }
            }
        }
        if let Some(params) = document
            .get("visualparams")
            .and_then(sl_llsd::Llsd::as_binary)
        {
            appearance.visual_params = params.to_vec();
        }
        if let Some(worn) = document
            .get("attachments")
            .and_then(sl_llsd::Llsd::as_array)
        {
            appearance.attachments = worn
                .iter()
                .map(|attachment| {
                    let key = |name: &str| match attachment.get(name) {
                        Some(sl_llsd::Llsd::Uuid(id)) => id.to_string(),
                        Some(other) => other.as_str().unwrap_or_default().to_ascii_lowercase(),
                        None => crate::value::NULL_KEY.to_owned(),
                    };
                    AppearanceAttachment {
                        point: attachment
                            .get("point")
                            .and_then(sl_llsd::Llsd::as_i32)
                            .unwrap_or_default(),
                        item: key("item"),
                        asset: key("asset"),
                    }
                })
                .collect();
        }
        Some(appearance)
    }
}

/// A double as .NET's `ToString("g")` writes it on Mono: fifteen significant
/// digits, trailing zeros dropped, an exponent past them.
fn general(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    let exact = format!("{value:.14e}");
    let (mantissa, exponent) = exact.split_once('e').unwrap_or((exact.as_str(), "0"));
    let exponent: i32 = exponent.parse().unwrap_or_default();
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let negative = value < 0.0;
    let sign = if negative { "-" } else { "" };
    if !(-4..15).contains(&exponent) {
        let (first, rest) = digits.split_at(1.min(digits.len()));
        let rest = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let mark = if exponent < 0 { '-' } else { '+' };
        return format!("{sign}{first}{rest}E{mark}{:02}", exponent.unsigned_abs());
    }
    let length = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    if exponent < 0 {
        let zeros =
            "0".repeat(usize::try_from(exponent.unsigned_abs().saturating_sub(1)).unwrap_or(0));
        return format!("{sign}0.{zeros}{digits}");
    }
    let whole = exponent.saturating_add(1);
    if whole >= length {
        let zeros = "0".repeat(usize::try_from(whole.saturating_sub(length)).unwrap_or(0));
        return format!("{sign}{digits}{zeros}");
    }
    let (integer, fraction) = digits.split_at(usize::try_from(whole).unwrap_or(0));
    format!("{sign}{integer}.{fraction}")
}

/// `bytes` in standard base64, as OpenSim writes an LLSD binary.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let [a, b, c] = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let triple = u32::from(a) << 16 | u32::from(b) << 8 | u32::from(c);
        for (index, shift) in [18_u32, 12, 6, 0].into_iter().enumerate() {
            if index > chunk.len() {
                out.push('=');
            } else {
                let sextet = usize::try_from(triple >> shift & 0x3F).unwrap_or(0);
                out.push(char::from(ALPHABET.get(sextet).copied().unwrap_or(b'A')));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A small appearance to write.
    fn sample() -> Appearance {
        Appearance {
            serial: 3,
            height: 1.97,
            hover: 0.0,
            textures: vec![DEFAULT_AVATAR_TEXTURE.to_owned(); 45],
            extra_bakes: None,
            visual_params: vec![0, 255, 128],
            attachments: vec![
                AppearanceAttachment {
                    point: 2,
                    item: "11111111-1111-1111-1111-111111111111".to_owned(),
                    asset: "22222222-2222-2222-2222-222222222222".to_owned(),
                },
                AppearanceAttachment {
                    point: 35,
                    item: "33333333-3333-3333-3333-333333333333".to_owned(),
                    asset: "44444444-4444-4444-4444-444444444444".to_owned(),
                },
            ],
        }
    }

    #[test]
    fn doubles_are_written_as_mono_writes_them() {
        assert_eq!(general(f64::from(1.97_f32)), "1.97000002861023");
        assert_eq!(general(0.0), "0");
        assert_eq!(general(2.0), "2");
        assert_eq!(general(0.5), "0.5");
        assert_eq!(general(1.0e20), "1E+20");
        assert_eq!(general(-0.000_012_5), "-1.25E-05");
        assert_eq!(general(0.000_125), "0.000125");
    }

    #[test]
    fn base64_pads_as_the_standard_alphabet_does() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
        assert_eq!(base64(&[0, 255, 128]), "AP+A");
    }

    #[test]
    fn a_notecard_reads_back_the_appearance_it_wrote() {
        let written = sample();
        let text = written.to_notecard(true);
        let default = Appearance {
            textures: Vec::new(),
            ..sample()
        };
        let read = Appearance::from_notecard(&text, &default);
        let expected = Appearance {
            height: f32::from_bits(1.97_f32.to_bits()),
            textures: vec![DEFAULT_AVATAR_TEXTURE.to_owned(); NOTECARD_TEXTURES],
            ..sample()
        };
        assert_eq!(read, Some(expected));
    }

    #[test]
    fn a_save_without_huds_leaves_the_hud_attachments_out() {
        let text = sample().to_notecard(false);
        assert!(text.contains("11111111-1111-1111-1111-111111111111"));
        assert!(!text.contains("33333333-3333-3333-3333-333333333333"));
        assert!(
            text.starts_with("<llsd>\n  <map>\n    <key>serial</key>\n    <integer>3</integer>\n")
        );
        assert!(text.contains("    <key>height</key>\n    <real>1.97000002861023</real>\n"));
        assert!(text.contains("      <array></array>\n"));
        assert!(text.contains("    <key>visualparams</key>\n    <binary>AP+A</binary>\n"));
    }

    #[test]
    fn text_that_is_no_map_is_no_appearance() {
        assert_eq!(Appearance::from_notecard("hello", &sample()), None);
        assert_eq!(
            Appearance::from_notecard("<llsd><integer>1</integer></llsd>", &sample()),
            None
        );
    }

    #[test]
    fn the_default_shape_is_female() {
        let mut appearance = sample();
        appearance.visual_params = vec![0; 40];
        assert_eq!(appearance.is_male(), Some(false));
        if let Some(male) = appearance.visual_params.get_mut(MALE_PARAMETER) {
            *male = 255;
        }
        assert_eq!(appearance.is_male(), Some(true));
        appearance.visual_params.clear();
        assert_eq!(appearance.is_male(), None);
    }
}
