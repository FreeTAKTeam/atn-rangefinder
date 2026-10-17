//! REM plugin map objects, inspected at REM `91124cffd8f7fa7df94684b672c2228d97f0522d`.
//! REM serializes the RCH `MarkerRecord` through `api_value()` (nested `position`,
//! RFC 3339 timestamps) and appends `textTruncated`. Within API major 1 fields are
//! only ever added, so unknown fields are tolerated. No storage, normalization,
//! networking or identity authority is recreated here.
use atn_domain::{DomainError, GeographicPosition, PlePoint};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapPosition {
    pub lat: f64,
    pub lon: f64,
}

/// One REM map object as delivered in `map.markers.*` results and `map.session.*`
/// payloads. Host identity (`object_destination_hash`) is retained exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemMapObject {
    pub object_destination_hash: String,
    #[serde(default)]
    pub origin_rch: Option<String>,
    #[serde(rename = "type", default)]
    pub marker_type: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub position: MapPosition,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub stale_at: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    pub updated_at: String,
    /// REM bounded a text field in this payload; the shared marker is unchanged.
    #[serde(rename = "textTruncated", default)]
    pub text_truncated: Option<bool>,
}

impl RemMapObject {
    /// Host `updated_at` as Unix milliseconds. It is provenance and a staleness
    /// guard, not a guaranteed monotonic concurrency token.
    ///
    /// # Errors
    /// Rejects a timestamp REM did not write as RFC 3339.
    pub fn updated_at_ms(&self) -> Result<i64, DomainError> {
        rfc3339_to_unix_ms(&self.updated_at).ok_or(DomainError::Invalid(
            "REM object updated_at is not RFC 3339",
        ))
    }

    /// # Errors
    /// Rejects missing marker identity or an unparseable host timestamp.
    pub fn as_ple(&self) -> Result<PlePoint, DomainError> {
        if self.object_destination_hash.trim().is_empty() {
            return Err(DomainError::Invalid("REM object identity"));
        }
        self.updated_at_ms()?;
        Ok(PlePoint {
            id: self.object_destination_hash.clone(),
            revision: self.updated_at.clone(),
            position: GeographicPosition {
                latitude: self.position.lat,
                longitude: self.position.lon,
                // REM markers carry no altitude; no vertical datum is assumed.
                altitude_m: None,
            },
            metadata: json!({"rem": self}),
        })
    }
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)` to Unix milliseconds.
/// Fractions beyond milliseconds are truncated. No calendar crate is pulled in
/// for one fixed host format.
#[must_use]
pub fn rfc3339_to_unix_ms(value: &str) -> Option<i64> {
    let value = value.trim();
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if !matches!(bytes[10], b'T' | b't' | b' ') || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let field = |range: std::ops::Range<usize>| value.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (field(0..4)?, field(5..7)?, field(8..10)?);
    let (hour, minute, second) = (field(11..13)?, field(14..16)?, field(17..19)?);
    let mut index = 19;
    let mut millis = 0i64;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        let digits = value.get(start..index)?;
        if digits.is_empty() {
            return None;
        }
        let padded = format!("{digits:0<3}");
        millis = padded.get(0..3)?.parse().ok()?;
    }
    let offset_minutes = match bytes.get(index) {
        Some(b'Z' | b'z') if index + 1 == bytes.len() => 0,
        Some(sign @ (b'+' | b'-')) if index + 6 == bytes.len() && bytes[index + 3] == b':' => {
            let hours = field(index + 1..index + 3)?;
            let minutes = field(index + 4..index + 6)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let total = hours * 60 + minutes;
            if *sign == b'-' { -total } else { total }
        }
        _ => return None,
    };
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(hour * 3600 + minute * 60 + second)?
        .checked_sub(offset_minutes * 60)?;
    seconds.checked_mul(1000)?.checked_add(millis)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era.checked_mul(146_097)?
        .checked_add(day_of_era)?
        .checked_sub(719_468)
}

#[cfg(test)]
mod tests {
    use super::rfc3339_to_unix_ms;

    #[test]
    fn parses_rem_timestamp_forms() {
        assert_eq!(rfc3339_to_unix_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            rfc3339_to_unix_ms("2026-10-16T12:34:56.789Z"),
            Some(1_792_154_096_789)
        );
        assert_eq!(
            rfc3339_to_unix_ms("2026-10-16T12:34:56.7Z"),
            Some(1_792_154_096_700)
        );
        assert_eq!(
            rfc3339_to_unix_ms("2026-10-16T13:34:56+01:00"),
            Some(1_792_154_096_000)
        );
        assert_eq!(rfc3339_to_unix_ms("2026-10-16T12:34:56"), None);
        assert_eq!(rfc3339_to_unix_ms("2026-13-16T12:34:56Z"), None);
        assert_eq!(rfc3339_to_unix_ms("not a time"), None);
    }
}
