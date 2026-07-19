//! Locale-aware **value formatting** for Second Life / OpenSim clients: numbers
//! (the grouping separator and decimal mark), the Linden dollar (L$) currency,
//! and dates / times — all backed by **CLDR through ICU4X** (the `icu` crate
//! family). The same [`LocaleFormatters::integer`] call renders `1,234,567` in
//! English, `1.234.567` in German, `1 234 567` in Polish and `١٬٢٣٤٬٥٦٧` in
//! Arabic.
//!
//! It is the value-formatting counterpart to the *text* side of localisation
//! (the viewer's Fluent scaffold, `viewer-i18n-fluent-scaffold`): Fluent decides
//! the words, the plural branch and the gender; this crate decides how the
//! numbers and dates *inside* those words are written for the locale. The split
//! exists because `fluent-rs` implements only the *selection* half of its
//! `NUMBER()` / `DATETIME()` builtins (enough to drive a plural), not the
//! *formatting* half — a number interpolated into a Fluent string comes out with
//! a bare `to_string()`, no grouping, no locale decimal mark, no digit shaping.
//!
//! It also formats **currency** (L$) and reads numbers back the other way:
//! [`LocaleFormatters::parse_number`] parses a value a user typed in the
//! locale's own conventions (a German `1.234,5`) for a localized float input
//! field.
//!
//! Like the other geometry / asset crates (`sl-prim`, `sl-material`, …) it is
//! **pure**: no Bevy, no I/O. The CLDR data is compiled into the binary by
//! ICU4X's `compiled_data` feature (on by default for each `icu_*` component),
//! so there is nothing to load at runtime.
//!
//! # Example
//!
//! ```
//! # use sl_l10n::{LocaleFormatters, CivilDateTime, DateTimeStyle, DateTimeLength};
//! let fmt = LocaleFormatters::from_tag("pl").unwrap();
//! assert_eq!(fmt.integer(1_234_567), "1\u{a0}234\u{a0}567");
//!
//! let when = CivilDateTime { year: 2026, month: 7, day: 19, hour: 14, minute: 30, second: 0 };
//! let stamp = fmt
//!     .datetime(when, DateTimeStyle::Time, DateTimeLength::Short)
//!     .unwrap();
//! assert!(stamp.contains("14"));
//! ```
//!
//! # Scope
//!
//! Full CLDR **unit** formatting (distance in metres vs feet, area, file sizes)
//! needs ICU4X's still-unstable `icu_experimental`; it is deliberately out of
//! scope here. A caller that needs a unit today formats the magnitude with
//! [`LocaleFormatters::decimal`] and appends a bundle-supplied unit label.

use fixed_decimal::{Decimal, FloatPrecision};
use icu_calendar::Date;
use icu_datetime::DateTimeFormatter;
use icu_datetime::fieldsets::{T, YMD, YMDT};
use icu_datetime::options::Length;
use icu_decimal::DecimalFormatter;
use icu_decimal::options::{DecimalFormatterOptions, GroupingStrategy};
use icu_locale_core::Locale;
use icu_time::{DateTime, Time};
use std::fmt::{self, Debug, Formatter};

/// The Linden-dollar currency symbol, prefixed by [`LocaleFormatters::currency_l`].
///
/// L$ is not a CLDR currency with its own per-locale placement data, so only the
/// *amount* is locale-formatted; the symbol and its placement are this crate's
/// (documented) default, and a caller that needs per-locale placement supplies
/// it through the Fluent bundle instead (see the crate README).
const LINDEN_SYMBOL: &str = "L$";

/// An error building a formatter or formatting a value.
///
/// Construction errors are ICU data-load failures (effectively impossible with
/// the compiled-in data, but surfaced rather than hidden); formatting errors are
/// an out-of-range date/time component or a non-finite float.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    /// The BCP-47 locale tag did not parse.
    #[error("invalid locale tag: {0}")]
    Locale(#[from] icu_locale_core::ParseError),
    /// An ICU formatter could not load its data (decimal / date-time).
    #[error("locale data unavailable: {0}")]
    Data(#[from] icu_provider::DataError),
    /// A date-time formatter could not be built for the requested field set.
    #[error("date/time formatter unavailable: {0}")]
    DateTime(#[from] icu_datetime::DateTimeFormatterLoadError),
    /// A date or time component was out of range (e.g. month 13, hour 24).
    #[error("date/time component out of range: {0}")]
    Range(#[from] icu_calendar::RangeError),
    /// The value was not a finite number (NaN or infinity). `fixed_decimal`'s
    /// [`LimitError`](fixed_decimal::LimitError) is a marker that does not
    /// implement [`std::error::Error`], so it is carried as data, not a source.
    #[error("value is not a finite number (NaN or infinity)")]
    NotFinite(fixed_decimal::LimitError),
    /// A string did not parse as a number in this locale's conventions.
    #[error("could not parse a number from {input:?}")]
    ParseNumber {
        /// The offending input.
        input: String,
    },
}

/// Which parts of a [`CivilDateTime`] a [`LocaleFormatters::datetime`] call
/// renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateTimeStyle {
    /// The date alone (year, month, day).
    Date,
    /// The time alone (hour, minute, and — at longer lengths — second).
    Time,
    /// The date and the time together.
    DateTime,
}

/// The verbosity of a formatted date / time, mapped to CLDR's three standard
/// lengths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateTimeLength {
    /// The most compact form (e.g. `7/19/26`, `14:30`).
    Short,
    /// The default form (e.g. `Jul 19, 2026`, `2:30:00 PM`).
    Medium,
    /// The most explicit form (e.g. `July 19, 2026`).
    Long,
}

impl DateTimeLength {
    /// The ICU length this maps to.
    const fn icu(self) -> Length {
        match self {
            Self::Short => Length::Short,
            Self::Medium => Length::Medium,
            Self::Long => Length::Long,
        }
    }
}

/// A civil (wall-clock) date and time, the input to [`LocaleFormatters::datetime`].
///
/// It is the caller's job to convert an epoch / UTC instant into the wall-clock
/// components of the desired zone (local, or SL's Pacific "SLT") before
/// formatting — this crate formats the components it is handed and does not do
/// time-zone conversion. Components are validated when formatted, so an
/// out-of-range value surfaces as [`FormatError::Range`] rather than a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CivilDateTime {
    /// The proleptic Gregorian year (e.g. `2026`).
    pub year: i32,
    /// The month, `1`..=`12`.
    pub month: u8,
    /// The day of the month, `1`..=`31`.
    pub day: u8,
    /// The hour, `0`..=`23`.
    pub hour: u8,
    /// The minute, `0`..=`59`.
    pub minute: u8,
    /// The second, `0`..=`59` (leap seconds are not represented).
    pub second: u8,
}

impl CivilDateTime {
    /// Build the ICU date-time input this represents, validating each component.
    fn to_icu(self) -> Result<DateTime<icu_calendar::Gregorian>, FormatError> {
        let date = Date::try_new_gregorian(self.year, self.month, self.day)?;
        let time = Time::try_new(self.hour, self.minute, self.second, 0)?;
        Ok(DateTime { date, time })
    }
}

/// The digits and separators a locale writes numbers with, probed once from the
/// decimal formatter so numbers can also be **parsed** back (a localized input
/// field: a German user types `1.234,5`).
///
/// Derived by formatting known values and reading the result, so it stays
/// consistent with what [`LocaleFormatters::integer`] / [`decimal`](LocaleFormatters::decimal)
/// produce, across ICU versions, without a second source of truth.
#[derive(Debug, Clone)]
struct NumberSymbols {
    /// The locale's digit glyphs, indexed `0`..=`9` (`digits[0]` is its zero) —
    /// Arabic-Indic `٠..٩`, Devanagari `०..९`, or plain ASCII.
    digits: [char; 10],
    /// The grouping separator (a comma, a dot, or a (narrow) no-break space).
    group: char,
    /// The decimal mark (a dot or a comma).
    decimal: char,
}

impl NumberSymbols {
    /// Probe the symbols from a built decimal formatter.
    fn probe(formatter: &DecimalFormatter) -> Self {
        let mut digits = ['0'; 10];
        for (index, slot) in digits.iter_mut().enumerate() {
            let value = i64::try_from(index).unwrap_or(0);
            if let Some(glyph) = formatter
                .format(&Decimal::from(value))
                .to_string()
                .chars()
                .next()
            {
                *slot = glyph;
            }
        }
        // The one non-digit char in a grouped/fractional sample is the separator.
        let grouped = formatter.format(&Decimal::from(1_000_000_i64)).to_string();
        let group = grouped
            .chars()
            .find(|candidate| !digits.contains(candidate))
            .unwrap_or(',');
        // A fractional sample surfaces the decimal mark. `1.5` is exactly
        // representable, so `RoundTrip` yields "1.5" with no rounding noise; if
        // the float entry point somehow fails, fall back to the dot.
        let decimal = Decimal::try_from_f64(1.5, FloatPrecision::RoundTrip)
            .map(|fractional| formatter.format(&fractional).to_string())
            .ok()
            .and_then(|rendered| {
                rendered
                    .chars()
                    .find(|candidate| !digits.contains(candidate))
            })
            .unwrap_or('.');
        Self {
            digits,
            group,
            decimal,
        }
    }
}

/// The set of locale-aware formatters for one locale.
///
/// Built once per locale (parsing the tag and loading the decimal formatter),
/// then reused for every value. A consumer that switches locale at runtime (the
/// viewer, on a language change) rebuilds this.
pub struct LocaleFormatters {
    /// The locale these formatters are for, kept to build the date-time
    /// formatters on demand (their field-set-specific types are not uniform, so
    /// they are not pre-built).
    locale: Locale,
    /// The number formatter, applying the locale's grouping and decimal mark.
    decimal: DecimalFormatter,
    /// The locale's digits and separators, for parsing input back.
    symbols: NumberSymbols,
}

impl Debug for LocaleFormatters {
    /// `DecimalFormatter` is not `Debug`; show the locale, which identifies the
    /// formatter set.
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocaleFormatters")
            .field("locale", &self.locale)
            .finish_non_exhaustive()
    }
}

impl LocaleFormatters {
    /// Build the formatters for a parsed [`Locale`].
    ///
    /// # Errors
    ///
    /// Returns [`FormatError::Data`] if the locale's decimal data cannot be
    /// loaded (not expected with the compiled-in data).
    pub fn new(locale: Locale) -> Result<Self, FormatError> {
        // `Auto` is the CLDR-correct default: group above the locale's minimum
        // grouping size (which is 2 for some locales, so "1234" is ungrouped
        // where the locale says so). `Always` is the opt-in for a value that
        // should always group.
        let mut options = DecimalFormatterOptions::default();
        options.grouping_strategy = Some(GroupingStrategy::Auto);
        let decimal = DecimalFormatter::try_new((&locale).into(), options)?;
        let symbols = NumberSymbols::probe(&decimal);
        Ok(Self {
            locale,
            decimal,
            symbols,
        })
    }

    /// Build the formatters from a BCP-47 locale tag (`"en"`, `"pl"`, `"ar-EG"`).
    ///
    /// A convenience for callers holding a tag string (e.g. a `unic_langid`
    /// `LanguageIdentifier` rendered with `to_string()`), so they need not depend
    /// on `icu_locale_core` to name the [`Locale`] type.
    ///
    /// # Errors
    ///
    /// Returns [`FormatError::Locale`] if the tag does not parse, or
    /// [`FormatError::Data`] as [`new`](Self::new).
    pub fn from_tag(tag: &str) -> Result<Self, FormatError> {
        Self::new(tag.parse::<Locale>()?)
    }

    /// Format a signed integer with the locale's grouping separator.
    #[must_use]
    pub fn integer(&self, value: i64) -> String {
        self.decimal.format(&Decimal::from(value)).to_string()
    }

    /// Format a float to exactly `fraction_digits` decimal places, with the
    /// locale's grouping separator and decimal mark.
    ///
    /// The value is rounded (half-to-even) to `fraction_digits` places and padded
    /// with trailing zeros to that many, so `12.5` at 2 digits is `12.50`.
    ///
    /// # Errors
    ///
    /// Returns [`FormatError::NotFinite`] if `value` is `NaN` or infinite.
    pub fn decimal(&self, value: f64, fraction_digits: u8) -> Result<String, FormatError> {
        let mut decimal = Decimal::try_from_f64(value, FloatPrecision::RoundTrip)
            .map_err(FormatError::NotFinite)?;
        // Position is negative for fractional places: -2 is the hundredths place.
        // `saturating_sub` from zero avoids the negation the arithmetic lint
        // forbids; the u8 range never saturates an i16.
        let position = 0_i16.saturating_sub(i16::from(fraction_digits));
        decimal.round(position);
        decimal.pad_end(position);
        Ok(self.decimal.format(&decimal).to_string())
    }

    /// Format a Linden-dollar amount: the grouped integer prefixed with the
    /// `L$` symbol (e.g. `L$1,234`).
    ///
    /// Only the amount is locale-formatted; the symbol placement is this crate's
    /// documented default — L$ is not a CLDR currency with per-locale placement
    /// data, so a caller that needs per-locale placement supplies it through the
    /// Fluent bundle (word order) instead.
    #[must_use]
    pub fn currency_l(&self, value: i64) -> String {
        let amount = self.integer(value);
        format!("{LINDEN_SYMBOL}{amount}")
    }

    /// Parse a number a user typed in this locale's conventions back into an
    /// `f64` — the inverse of [`decimal`](Self::decimal), for a localized float
    /// input field.
    ///
    /// Accepts the locale's own digits (Arabic-Indic, Devanagari, …) and ASCII
    /// digits, drops the locale grouping separator and any whitespace, and reads
    /// the locale decimal mark (so `1.234,5` parses in German and `1,234.5` in
    /// English). A leading sign and an `e`/`E` exponent are allowed; any other
    /// character makes it an error.
    ///
    /// # Errors
    ///
    /// Returns [`FormatError::ParseNumber`] if the input contains an
    /// unrecognised character or does not form a number.
    pub fn parse_number(&self, input: &str) -> Result<f64, FormatError> {
        /// Build the parse error for `input` without repeating the allocation.
        fn reject(input: &str) -> FormatError {
            FormatError::ParseNumber {
                input: input.to_owned(),
            }
        }
        let mut normalized = String::new();
        for ch in input.trim().chars() {
            if ch == self.symbols.decimal {
                normalized.push('.');
            } else if ch == self.symbols.group || ch.is_whitespace() {
                // Grouping is cosmetic; drop it (and any stray spaces).
            } else if let Some(position) = self.symbols.digits.iter().position(|&d| d == ch) {
                // A locale digit → its ASCII counterpart.
                let ascii = u32::try_from(position)
                    .ok()
                    .and_then(|digit| char::from_digit(digit, 10))
                    .ok_or_else(|| reject(input))?;
                normalized.push(ascii);
            } else if ch.is_ascii_digit() || matches!(ch, '-' | '+' | 'e' | 'E') {
                // ASCII digits and sign / exponent markers pass through, so a
                // user can type ASCII even in a shaped-digit locale.
                normalized.push(ch);
            } else {
                return Err(reject(input));
            }
        }
        // The reason is captured by `input`; the underlying `ParseFloatError`
        // adds nothing, so map through `ok()` rather than `map_err`.
        normalized.parse::<f64>().ok().ok_or_else(|| reject(input))
    }

    /// Format a civil date / time for the locale, at the given style and length.
    ///
    /// The style selects which components appear (date, time, or both); the
    /// calendar and the field order are the locale's own (e.g. `y/m/d` for
    /// Japanese, `d/m/y` for most of Europe).
    ///
    /// # Errors
    ///
    /// Returns [`FormatError::Range`] if a component is out of range,
    /// [`FormatError::DateTime`] if the formatter cannot be built.
    pub fn datetime(
        &self,
        when: CivilDateTime,
        style: DateTimeStyle,
        length: DateTimeLength,
    ) -> Result<String, FormatError> {
        let input = when.to_icu()?;
        let icu_length = length.icu();
        // Each field set is a distinct type, so the arms build and format
        // independently rather than sharing a formatter binding.
        let formatted = match style {
            DateTimeStyle::Date => {
                let formatter = DateTimeFormatter::try_new(
                    (&self.locale).into(),
                    YMD::long().with_length(icu_length),
                )?;
                formatter.format(&input).to_string()
            }
            DateTimeStyle::Time => {
                let formatter = DateTimeFormatter::try_new(
                    (&self.locale).into(),
                    T::long().with_length(icu_length),
                )?;
                formatter.format(&input).to_string()
            }
            DateTimeStyle::DateTime => {
                let formatter = DateTimeFormatter::try_new(
                    (&self.locale).into(),
                    YMDT::long().with_length(icu_length),
                )?;
                formatter.format(&input).to_string()
            }
        };
        Ok(formatted)
    }
}

#[cfg(test)]
mod tests {
    use super::{CivilDateTime, DateTimeLength, DateTimeStyle, FormatError, LocaleFormatters};
    use pretty_assertions::assert_eq;

    /// A sample date-time reused across the date/time tests.
    const SAMPLE: CivilDateTime = CivilDateTime {
        year: 2026,
        month: 7,
        day: 19,
        hour: 14,
        minute: 30,
        second: 5,
    };

    /// The formatters must stay `Send + Sync` so a consumer (the Bevy viewer)
    /// can hold them in a resource — this depends on `icu_provider`'s `sync`
    /// feature (Rc → Arc), which this asserts is still enabled.
    #[test]
    fn formatters_are_send_and_sync() {
        /// Compiles only if `T: Send + Sync`.
        const fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<LocaleFormatters>();
    }

    /// English groups in threes with a comma; the load-bearing base case.
    #[test]
    fn english_groups_with_commas() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("en")?;
        assert_eq!(fmt.integer(1_234_567), "1,234,567");
        assert_eq!(fmt.integer(-42), "-42");
        Ok(())
    }

    /// Polish groups with a non-breaking space and uses a comma as the decimal
    /// mark — the case a naive "comma every three digits" gets wrong. A 4-digit
    /// value is deliberately *not* grouped (Polish `minimumGroupingDigits`), so
    /// the decimal check uses a 7-digit value that does group.
    #[test]
    fn polish_groups_with_nbsp_and_comma_decimal() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("pl")?;
        assert_eq!(fmt.integer(1_234_567), "1\u{a0}234\u{a0}567");
        assert_eq!(fmt.integer(1_234), "1234");
        assert_eq!(fmt.decimal(1_234_567.5, 2)?, "1\u{a0}234\u{a0}567,50");
        Ok(())
    }

    /// German swaps the roles of comma and dot from English.
    #[test]
    fn german_swaps_separators() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("de")?;
        assert_eq!(fmt.integer(1_234_567), "1.234.567");
        assert_eq!(fmt.decimal(1_234.5, 2)?, "1.234,50");
        Ok(())
    }

    /// The fraction-digit count is exact: rounded and zero-padded to the request.
    #[test]
    fn decimal_pads_and_rounds_to_requested_digits() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("en")?;
        assert_eq!(fmt.decimal(12.5, 2)?, "12.50");
        assert_eq!(fmt.decimal(12.567, 2)?, "12.57");
        assert_eq!(fmt.decimal(12.0, 0)?, "12");
        Ok(())
    }

    /// A non-finite float is a clean error, not a panic.
    #[test]
    fn decimal_rejects_non_finite() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("en")?;
        assert!(matches!(
            fmt.decimal(f64::NAN, 2),
            Err(FormatError::NotFinite(_))
        ));
        Ok(())
    }

    /// The currency helper groups the amount and prefixes the L$ symbol.
    #[test]
    fn currency_prefixes_linden_symbol() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("en")?;
        assert_eq!(fmt.currency_l(1_234_567), "L$1,234,567");
        Ok(())
    }

    /// A time formats with the locale's digits and separators; the short form
    /// carries the hour and minute.
    #[test]
    fn time_short_carries_hour_and_minute() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("en")?;
        let stamp = fmt.datetime(SAMPLE, DateTimeStyle::Time, DateTimeLength::Short)?;
        assert!(stamp.contains("30"), "minute present: {stamp}");
        Ok(())
    }

    /// A full date-time renders both halves; the Japanese order puts the year
    /// first, proving the field order is the locale's.
    #[test]
    fn japanese_datetime_orders_year_first() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("ja")?;
        let stamp = fmt.datetime(SAMPLE, DateTimeStyle::DateTime, DateTimeLength::Short)?;
        assert!(stamp.contains("2026"), "year present: {stamp}");
        // Japanese writes the year before the month/day.
        let year_at = stamp.find("2026");
        let day_at = stamp.find("19");
        assert!(
            matches!((year_at, day_at), (Some(y), Some(d)) if y < d),
            "year precedes day: {stamp}"
        );
        Ok(())
    }

    /// An out-of-range component is a clean error, not a panic.
    #[test]
    fn datetime_rejects_out_of_range() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("en")?;
        let bad = CivilDateTime {
            month: 13,
            ..SAMPLE
        };
        assert!(matches!(
            fmt.datetime(bad, DateTimeStyle::Date, DateTimeLength::Short),
            Err(FormatError::Range(_))
        ));
        Ok(())
    }

    /// Parsing is the inverse of formatting: English reads a comma-grouped,
    /// dot-decimal number.
    #[test]
    fn parses_english_input() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("en")?;
        assert!((fmt.parse_number("1,234.5")? - 1234.5).abs() < f64::EPSILON);
        assert!((fmt.parse_number("-42")? - -42.0).abs() < f64::EPSILON);
        assert!((fmt.parse_number("  12 ")? - 12.0).abs() < f64::EPSILON);
        Ok(())
    }

    /// German reads a dot-grouped, comma-decimal number — the case where the two
    /// separators swap roles from English.
    #[test]
    fn parses_german_input() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("de")?;
        assert!((fmt.parse_number("1.234,5")? - 1234.5).abs() < f64::EPSILON);
        Ok(())
    }

    /// A format → parse round trip returns the original value.
    #[test]
    fn format_parse_round_trips() -> Result<(), FormatError> {
        for tag in ["en", "de", "pl", "ar"] {
            let fmt = LocaleFormatters::from_tag(tag)?;
            let rendered = fmt.decimal(1_234.5, 1)?;
            let parsed = fmt.parse_number(&rendered)?;
            assert!(
                (parsed - 1234.5).abs() < 1e-9,
                "round trip in {tag}: {rendered} -> {parsed}"
            );
        }
        Ok(())
    }

    /// Junk input is a clean error, not a panic or a silent zero.
    #[test]
    fn parse_rejects_junk() -> Result<(), FormatError> {
        let fmt = LocaleFormatters::from_tag("en")?;
        assert!(matches!(
            fmt.parse_number("12abc"),
            Err(FormatError::ParseNumber { .. })
        ));
        Ok(())
    }

    /// A bad locale tag is a clean error.
    #[test]
    fn bad_tag_is_an_error() {
        assert!(matches!(
            LocaleFormatters::from_tag("not a tag"),
            Err(FormatError::Locale(_))
        ));
    }
}
