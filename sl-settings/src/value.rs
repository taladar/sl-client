//! The typed values a setting can hold, and their type tags.

use serde::{Deserialize, Serialize};

/// A single setting's value — the Rust-native counterpart of the reference
/// viewer's control types (`LLControlVariable`'s `eControlType`).
///
/// The variants cover the control types the reference viewer actually uses for
/// stored preferences: a boolean toggle, signed / unsigned integers, a float, a
/// string, RGB / RGBA colours, `f32` / `f64` 3-vectors, and a rectangle. The
/// reference's `TYPE_LLSD` escape hatch is deliberately omitted — a settings
/// value is always one concrete typed shape here, not an arbitrary document.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_settings::SettingValue`, where it reads clearly"
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SettingValue {
    /// A boolean toggle (reference `TYPE_BOOLEAN`).
    Bool(bool),
    /// A signed 32-bit integer (reference `TYPE_S32`).
    I32(i32),
    /// An unsigned 32-bit integer (reference `TYPE_U32`).
    U32(u32),
    /// A 32-bit float (reference `TYPE_F32`).
    F32(f32),
    /// A UTF-8 string (reference `TYPE_STRING`).
    String(String),
    /// A linear RGB colour, one `f32` per channel (reference `TYPE_COL3`).
    Color3([f32; 3]),
    /// A linear RGBA colour, one `f32` per channel (reference `TYPE_COL4`).
    Color4([f32; 4]),
    /// A 3-vector of `f32` (reference `TYPE_VEC3`).
    Vec3([f32; 3]),
    /// A 3-vector of `f64` (reference `TYPE_VEC3D`).
    Vec3d([f64; 3]),
    /// A rectangle `[left, top, right, bottom]` in pixels (reference
    /// `TYPE_RECT`).
    Rect([i32; 4]),
}

/// The type tag of a [`SettingValue`], used to check a value being written
/// against the type a setting was declared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    /// See [`SettingValue::Bool`].
    Bool,
    /// See [`SettingValue::I32`].
    I32,
    /// See [`SettingValue::U32`].
    U32,
    /// See [`SettingValue::F32`].
    F32,
    /// See [`SettingValue::String`].
    String,
    /// See [`SettingValue::Color3`].
    Color3,
    /// See [`SettingValue::Color4`].
    Color4,
    /// See [`SettingValue::Vec3`].
    Vec3,
    /// See [`SettingValue::Vec3d`].
    Vec3d,
    /// See [`SettingValue::Rect`].
    Rect,
}

impl SettingValue {
    /// The [`SettingKind`] type tag of this value.
    #[must_use]
    pub const fn kind(&self) -> SettingKind {
        match self {
            Self::Bool(_) => SettingKind::Bool,
            Self::I32(_) => SettingKind::I32,
            Self::U32(_) => SettingKind::U32,
            Self::F32(_) => SettingKind::F32,
            Self::String(_) => SettingKind::String,
            Self::Color3(_) => SettingKind::Color3,
            Self::Color4(_) => SettingKind::Color4,
            Self::Vec3(_) => SettingKind::Vec3,
            Self::Vec3d(_) => SettingKind::Vec3d,
            Self::Rect(_) => SettingKind::Rect,
        }
    }

    /// The boolean this holds, or `None` if it is a different type.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The signed integer this holds, or `None` if it is a different type.
    #[must_use]
    pub const fn as_i32(&self) -> Option<i32> {
        match self {
            Self::I32(value) => Some(*value),
            _ => None,
        }
    }

    /// The unsigned integer this holds, or `None` if it is a different type.
    #[must_use]
    pub const fn as_u32(&self) -> Option<u32> {
        match self {
            Self::U32(value) => Some(*value),
            _ => None,
        }
    }

    /// The float this holds, or `None` if it is a different type.
    #[must_use]
    pub const fn as_f32(&self) -> Option<f32> {
        match self {
            Self::F32(value) => Some(*value),
            _ => None,
        }
    }

    /// The string this holds, or `None` if it is a different type.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// The RGB colour this holds, or `None` if it is a different type.
    #[must_use]
    pub const fn as_color3(&self) -> Option<[f32; 3]> {
        match self {
            Self::Color3(value) => Some(*value),
            _ => None,
        }
    }

    /// The RGBA colour this holds, or `None` if it is a different type.
    #[must_use]
    pub const fn as_color4(&self) -> Option<[f32; 4]> {
        match self {
            Self::Color4(value) => Some(*value),
            _ => None,
        }
    }

    /// The `f32` 3-vector this holds, or `None` if it is a different type.
    #[must_use]
    pub const fn as_vec3(&self) -> Option<[f32; 3]> {
        match self {
            Self::Vec3(value) => Some(*value),
            _ => None,
        }
    }

    /// The `f64` 3-vector this holds, or `None` if it is a different type.
    #[must_use]
    pub const fn as_vec3d(&self) -> Option<[f64; 3]> {
        match self {
            Self::Vec3d(value) => Some(*value),
            _ => None,
        }
    }

    /// The rectangle this holds, or `None` if it is a different type.
    #[must_use]
    pub const fn as_rect(&self) -> Option<[i32; 4]> {
        match self {
            Self::Rect(value) => Some(*value),
            _ => None,
        }
    }

    /// This value read as `kind`, or `None` if it cannot be.
    ///
    /// This exists for one caller: an override loaded from disk *before* its
    /// setting was declared has no declared type to be read at, so it is
    /// inferred from its TOML shape (see `crate::toml_format`) — an integer
    /// literal becomes an [`I32`](Self::I32) whatever the setting turns out to
    /// be. When the declaration arrives,
    /// [`SettingsStore`](crate::SettingsStore) re-reads the stored value
    /// through here, so a late registration cannot cost the user their saved
    /// value.
    ///
    /// The conversions are exactly the ones the TOML reader would have made
    /// from the same literals had the declaration been known: an integer
    /// literal reads as a signed or unsigned integer or as a float, an
    /// all-integer array reads as a float array, and the three-element and
    /// four-element array types are interchangeable within their length.
    /// Anything else is `None`, which drops the override the same way a
    /// declared value that no longer fits its type is dropped on load.
    ///
    /// The one place this is coarser than reading the file would have been is
    /// [`Vec3d`](Self::Vec3d): a late-declared `f64` vector is widened from the
    /// `f32`s the untyped read produced, so it keeps `f32` precision.
    #[must_use]
    pub(crate) fn retyped_as(&self, kind: SettingKind) -> Option<Self> {
        if self.kind() == kind {
            return Some(self.clone());
        }
        match (self, kind) {
            (Self::I32(number), SettingKind::U32) => u32::try_from(*number).ok().map(Self::U32),
            (Self::U32(number), SettingKind::I32) => i32::try_from(*number).ok().map(Self::I32),
            (Self::I32(number), SettingKind::F32) => {
                integer_as_f32(i64::from(*number)).map(Self::F32)
            }
            (Self::U32(number), SettingKind::F32) => {
                integer_as_f32(i64::from(*number)).map(Self::F32)
            }
            (Self::Vec3(vector), SettingKind::Color3) => Some(Self::Color3(*vector)),
            (Self::Vec3(vector), SettingKind::Vec3d) => Some(Self::Vec3d(vector.map(f64::from))),
            (Self::Color3(colour), SettingKind::Vec3) => Some(Self::Vec3(*colour)),
            (Self::Color3(colour), SettingKind::Vec3d) => Some(Self::Vec3d(colour.map(f64::from))),
            (Self::Rect(rect), SettingKind::Color4) => rect_as_f32(*rect).map(Self::Color4),
            _ => None,
        }
    }
}

/// An integer literal read as an `f32`, through its decimal text so the result
/// matches the TOML reader's own integer-to-float path.
fn integer_as_f32(number: i64) -> Option<f32> {
    format!("{number}").parse::<f32>().ok()
}

/// A rectangle's four integers read as `f32`s — the reading a declared
/// [`SettingKind::Color4`] would have taken from the same four integer
/// literals.
fn rect_as_f32(rect: [i32; 4]) -> Option<[f32; 4]> {
    let mut out = Vec::with_capacity(rect.len());
    for number in rect {
        out.push(integer_as_f32(i64::from(number))?);
    }
    <[f32; 4]>::try_from(out).ok()
}
