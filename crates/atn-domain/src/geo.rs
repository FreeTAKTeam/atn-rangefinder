use std::collections::BTreeMap;

use atn_protocol::{DisplayMarker, RawObservation};
use serde::{Deserialize, Serialize};

use crate::DomainError;

/// Geographic coordinates in degrees with optional ellipsoidal altitude in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeographicPosition {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: Option<f64>,
}

impl GeographicPosition {
    /// # Errors
    /// Rejects nonfinite coordinates/altitude and out-of-domain degrees.
    pub fn validate(&self) -> Result<(), DomainError> {
        if !self.latitude.is_finite() || !(-90.0..=90.0).contains(&self.latitude) {
            return Err(DomainError::Invalid("latitude"));
        }
        if !self.longitude.is_finite() || !(-180.0..=180.0).contains(&self.longitude) {
            return Err(DomainError::Invalid("longitude"));
        }
        if self
            .altitude_m
            .is_some_and(|altitude| !altitude.is_finite())
        {
            return Err(DomainError::Invalid("altitude"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ObserverSource {
    DevicePosition,
    VerifiedColocated,
    VerifiedOffsetResolved,
    Unverified,
}

/// The source/evidence identify the measuring device's resolved position. An
/// arbitrary latest telemetry record is not an eligible observer position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObserverFix {
    pub position: GeographicPosition,
    pub captured_at_ms: u64,
    pub horizontal_accuracy_m: f64,
    pub source: ObserverSource,
    pub evidence_id: String,
}

impl ObserverFix {
    /// Validate the measuring device's eligible position at the point of use.
    ///
    /// # Errors
    /// Rejects malformed, unverified, inaccurate, stale or future observer fixes.
    pub fn validate(
        &self,
        eligibility: &EligibilityPolicy,
        now_ms: u64,
    ) -> Result<(), DomainError> {
        validated_observer(Some(self), eligibility, now_ms).map(|_| ())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservationRecord {
    pub raw: RawObservation,
    pub observed_at_ms: u64,
    /// Ephemeral adapter connection identity, never a published hardware address.
    pub source_session: String,
    pub decoder_version: String,
    pub raw_bytes: Vec<u8>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RangeKind {
    Slant,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PitchConvention {
    PositiveUp,
    PositiveDown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BearingReference {
    TrueNorth,
    #[serde(rename_all = "camelCase")]
    MagneticNorth {
        /// East-positive declination: true bearing = magnetic + declination.
        declination_degrees: f64,
        captured_at_ms: u64,
        evidence_id: String,
    },
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Datum {
    Wgs84,
    Unverified,
}

/// Caller-confirmed valid raw interval; excludes unverified sentinel values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntegerBounds {
    pub min: i16,
    pub max: i16,
}

impl IntegerBounds {
    fn validate(self) -> Result<(), DomainError> {
        if self.min <= self.max {
            Ok(())
        } else {
            Err(DomainError::Invalid("inverted wire bounds"))
        }
    }

    fn contains(self, value: i32) -> bool {
        (i32::from(self.min)..=i32::from(self.max)).contains(&value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RawFieldBounds {
    pub distance: IntegerBounds,
    pub pitch: IntegerBounds,
    pub compass: IntegerBounds,
}

impl RawFieldBounds {
    /// # Errors
    /// Rejects inverted, caller-supplied raw intervals.
    pub fn validate(self) -> Result<(), DomainError> {
        self.distance.validate()?;
        self.pitch.validate()?;
        self.compass.validate()
    }

    fn check(self, distance: i32, pitch: i32, compass: i32) -> Result<(), DomainError> {
        if !self.distance.contains(distance) {
            return Err(DomainError::Invalid(
                "distance outside configured wire bounds",
            ));
        }
        if !self.pitch.contains(pitch) {
            return Err(DomainError::Invalid("pitch outside configured wire bounds"));
        }
        if !self.compass.contains(compass) {
            return Err(DomainError::Invalid(
                "compass outside configured wire bounds",
            ));
        }
        Ok(())
    }
}

/// Explicit configuration attested by the caller for the current device setup.
/// Numeric scaling is not established by the raw codec or protocol documents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SemanticsPolicy {
    pub evidence_id: String,
    pub calibration_confirmed: bool,
    pub distance_meters_per_unit: f64,
    pub angle_degrees_per_unit: f64,
    pub range_kind: RangeKind,
    pub pitch_convention: PitchConvention,
    pub bearing_reference: BearingReference,
    pub datum: Datum,
    pub max_distance_m: f64,
    pub observation_bounds: RawFieldBounds,
    pub display_bounds: RawFieldBounds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EligibilityPolicy {
    pub max_observer_age_ms: u64,
    pub max_observation_age_ms: u64,
    pub max_declination_age_ms: u64,
    pub max_horizontal_accuracy_m: f64,
}

impl EligibilityPolicy {
    /// # Errors
    /// Rejects a nonpositive or nonfinite maximum observer accuracy.
    pub fn validate(&self) -> Result<(), DomainError> {
        positive_finite(self.max_horizontal_accuracy_m, "maximum observer accuracy")
    }
}

impl SemanticsPolicy {
    /// Validate explicit read-only configuration without asserting calibration.
    ///
    /// # Errors
    /// Rejects invalid scaling, range bounds or declination values.
    pub fn validate_configuration(&self) -> Result<(), DomainError> {
        positive_finite(self.distance_meters_per_unit, "distance scale")?;
        positive_finite(self.angle_degrees_per_unit, "angle scale")?;
        positive_finite(self.max_distance_m, "maximum distance")?;
        self.observation_bounds.validate()?;
        self.display_bounds.validate()?;
        if let BearingReference::MagneticNorth {
            declination_degrees,
            ..
        } = self.bearing_reference
        {
            if !declination_degrees.is_finite() || !(-180.0..=180.0).contains(&declination_degrees)
            {
                return Err(DomainError::Invalid("magnetic declination"));
            }
        }
        Ok(())
    }

    /// # Errors
    /// Rejects unverified calibration, datum or north reference, invalid
    /// configuration, and stale/future declination evidence.
    pub fn validate(
        &self,
        eligibility: &EligibilityPolicy,
        now_ms: u64,
    ) -> Result<(), DomainError> {
        eligibility.validate()?;
        if !self.calibration_confirmed || self.evidence_id.trim().is_empty() {
            return Err(DomainError::Unverified("calibration evidence"));
        }
        self.validate_configuration()?;
        if self.datum != Datum::Wgs84 {
            return Err(DomainError::Unverified("coordinate datum"));
        }
        match &self.bearing_reference {
            BearingReference::TrueNorth => Ok(()),
            BearingReference::Unverified => Err(DomainError::Unverified("north reference")),
            BearingReference::MagneticNorth {
                declination_degrees,
                captured_at_ms,
                evidence_id,
            } => {
                if !declination_degrees.is_finite()
                    || !(-180.0..=180.0).contains(declination_degrees)
                {
                    return Err(DomainError::Invalid("magnetic declination"));
                }
                if evidence_id.trim().is_empty() {
                    return Err(DomainError::Unverified("declination evidence"));
                }
                fresh(
                    *captured_at_ms,
                    now_ms,
                    eligibility.max_declination_age_ms,
                    "declination",
                )
            }
        }
    }

    fn declination(&self) -> f64 {
        match self.bearing_reference {
            BearingReference::MagneticNorth {
                declination_degrees,
                ..
            } => declination_degrees,
            BearingReference::TrueNorth | BearingReference::Unverified => 0.0,
        }
    }

    fn up_sign(&self) -> f64 {
        match self.pitch_convention {
            PitchConvention::PositiveUp => 1.0,
            PitchConvention::PositiveDown => -1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlePoint {
    pub id: String,
    pub revision: String,
    pub position: GeographicPosition,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkerStyle {
    pub slot_id: u16,
    pub marker_type: u8,
    pub color: u8,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkerPitchPolicy {
    /// Explicit caller choice matching a relative, level display representation.
    HorizontalDisplay,
    /// Requires ellipsoidal altitudes for both observer and PLE.
    UseAltitudes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MapState {
    Draft,
}

/// Uncertainty inputs are preserved without inventing a derived survey accuracy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivationUncertainty {
    pub observer_horizontal_accuracy_m: f64,
    pub measurement_accuracy_known: bool,
    pub derived_accuracy_m: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedDraft {
    pub state: MapState,
    pub position: GeographicPosition,
    pub observation: ObservationRecord,
    pub observer: ObserverFix,
    pub semantics: SemanticsPolicy,
    pub horizontal_distance_m: f64,
    pub true_bearing_degrees: f64,
    pub derived_at_ms: u64,
    pub uncertainty: DerivationUncertainty,
    pub projection_method: String,
}

/// Derive a draft only after every location and physical-semantic gate passes.
///
/// # Errors
/// Rejects absent/stale/future/inaccurate fixes, unresolved semantics, malformed
/// measurements or provenance, and a failed ellipsoidal projection.
pub fn derive_draft(
    observation: &ObservationRecord,
    observer: Option<&ObserverFix>,
    semantics: &SemanticsPolicy,
    eligibility: &EligibilityPolicy,
    now_ms: u64,
) -> Result<DerivedDraft, DomainError> {
    semantics.validate(eligibility, now_ms)?;
    let observer = validated_observer(observer, eligibility, now_ms)?;
    fresh(
        observation.observed_at_ms,
        now_ms,
        eligibility.max_observation_age_ms,
        "observation",
    )?;
    if observation.source_session.trim().is_empty() || observation.decoder_version.trim().is_empty()
    {
        return Err(DomainError::Invalid("observation provenance"));
    }
    if !matches!(atn_protocol::parse_inbound(&observation.raw_bytes),
        Ok(atn_protocol::InboundFrame::Measurement(raw)) if raw == observation.raw)
    {
        return Err(DomainError::Invalid(
            "raw observation and retained frame disagree",
        ));
    }
    semantics.observation_bounds.check(
        i32::from(observation.raw.distance),
        i32::from(observation.raw.pitch),
        i32::from(observation.raw.compass),
    )?;
    let distance = f64::from(observation.raw.distance) * semantics.distance_meters_per_unit;
    if !distance.is_finite() || !(0.0..=semantics.max_distance_m).contains(&distance) {
        return Err(DomainError::Invalid("observation distance"));
    }
    let raw_pitch = f64::from(observation.raw.pitch) * semantics.angle_degrees_per_unit;
    if !raw_pitch.is_finite() || !(-90.0..=90.0).contains(&raw_pitch) {
        return Err(DomainError::Invalid("observation inclination"));
    }
    let compass = f64::from(observation.raw.compass) * semantics.angle_degrees_per_unit;
    if !compass.is_finite() {
        return Err(DomainError::Invalid("observation compass"));
    }
    let pitch = (raw_pitch * semantics.up_sign()).to_radians();
    let (horizontal_distance, vertical_delta) = match semantics.range_kind {
        RangeKind::Slant => (distance * pitch.cos(), Some(distance * pitch.sin())),
        RangeKind::Horizontal => {
            // A vertical observation cannot resolve altitude from horizontal range.
            let delta = if raw_pitch.abs() >= 90.0 {
                None
            } else {
                Some(distance * pitch.tan())
            };
            (distance, delta)
        }
    };
    let bearing = (compass + semantics.declination()).rem_euclid(360.0);
    let mut position = direct_wgs84(&observer.position, horizontal_distance, bearing)?;
    position.altitude_m = observer
        .position
        .altitude_m
        .zip(vertical_delta)
        .map(|(a, b)| a + b);
    position.validate()?;
    Ok(DerivedDraft {
        state: MapState::Draft,
        position,
        observation: observation.clone(),
        observer: observer.clone(),
        semantics: semantics.clone(),
        horizontal_distance_m: horizontal_distance,
        true_bearing_degrees: bearing,
        derived_at_ms: now_ms,
        uncertainty: DerivationUncertainty {
            observer_horizontal_accuracy_m: observer.horizontal_accuracy_m,
            measurement_accuracy_known: false,
            derived_accuracy_m: None,
        },
        projection_method: "WGS84 ellipsoid; Vincenty direct; draft observation".to_owned(),
    })
}

/// Convert an external PLE to relative device fields. Geographic degrees are
/// never copied into range/pitch/compass fields.
///
/// # Errors
/// Rejects ineligible observers, unresolved semantics, missing required
/// altitudes, indeterminate geodesics, range limits and numeric narrowing.
pub fn marker_for_ple(
    ple: &PlePoint,
    observer: Option<&ObserverFix>,
    semantics: &SemanticsPolicy,
    eligibility: &EligibilityPolicy,
    style: &MarkerStyle,
    pitch_policy: MarkerPitchPolicy,
    now_ms: u64,
) -> Result<DisplayMarker, DomainError> {
    semantics.validate(eligibility, now_ms)?;
    let observer = validated_observer(observer, eligibility, now_ms)?;
    if ple.id.trim().is_empty() {
        return Err(DomainError::Invalid("PLE identity"));
    }
    ple.position.validate()?;
    let (horizontal, true_bearing) = inverse_wgs84(&observer.position, &ple.position)?;
    let (distance, pitch_degrees) = match pitch_policy {
        MarkerPitchPolicy::HorizontalDisplay => (horizontal, 0.0),
        MarkerPitchPolicy::UseAltitudes => {
            let (observer_altitude, point_altitude) = observer
                .position
                .altitude_m
                .zip(ple.position.altitude_m)
                .ok_or(DomainError::Invalid("both ellipsoidal altitudes required"))?;
            let vertical = point_altitude - observer_altitude;
            if !vertical.is_finite() {
                return Err(DomainError::Invalid("altitude difference"));
            }
            let distance = match semantics.range_kind {
                RangeKind::Slant => horizontal.hypot(vertical),
                RangeKind::Horizontal => horizontal,
            };
            (
                distance,
                vertical.atan2(horizontal).to_degrees() * semantics.up_sign(),
            )
        }
    };
    if !distance.is_finite() || distance > semantics.max_distance_m {
        return Err(DomainError::Invalid("PLE range exceeds configured maximum"));
    }
    let compass = (true_bearing - semantics.declination()).rem_euclid(360.0);
    let raw_distance = protocol_integer(distance / semantics.distance_meters_per_unit, "distance")?;
    let raw_pitch = protocol_integer(pitch_degrees / semantics.angle_degrees_per_unit, "pitch")?;
    let raw_compass = protocol_integer(compass / semantics.angle_degrees_per_unit, "compass")?;
    semantics
        .display_bounds
        .check(raw_distance, raw_pitch, raw_compass)?;
    DisplayMarker::new(
        style.slot_id,
        style.marker_type,
        style.enabled,
        style.color,
        raw_distance,
        raw_pitch,
        raw_compass,
    )
    .map_err(Into::into)
}

fn validated_observer<'a>(
    observer: Option<&'a ObserverFix>,
    eligibility: &EligibilityPolicy,
    now_ms: u64,
) -> Result<&'a ObserverFix, DomainError> {
    eligibility.validate()?;
    let observer = observer.ok_or(DomainError::MissingObserver)?;
    observer.position.validate()?;
    if observer.source == ObserverSource::Unverified || observer.evidence_id.trim().is_empty() {
        return Err(DomainError::Unverified("observer co-location/source"));
    }
    if !observer.horizontal_accuracy_m.is_finite() || observer.horizontal_accuracy_m < 0.0 {
        return Err(DomainError::Invalid("observer accuracy"));
    }
    if observer.horizontal_accuracy_m > eligibility.max_horizontal_accuracy_m {
        return Err(DomainError::InaccurateObserver);
    }
    fresh(
        observer.captured_at_ms,
        now_ms,
        eligibility.max_observer_age_ms,
        "observer",
    )?;
    Ok(observer)
}

fn positive_finite(value: f64, name: &'static str) -> Result<(), DomainError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(DomainError::Invalid(name))
    }
}

fn fresh(
    timestamp: u64,
    now: u64,
    maximum_age: u64,
    name: &'static str,
) -> Result<(), DomainError> {
    let age = now
        .checked_sub(timestamp)
        .ok_or(DomainError::Future(name))?;
    if age <= maximum_age {
        Ok(())
    } else {
        Err(DomainError::Stale(name))
    }
}

#[allow(clippy::cast_possible_truncation)]
fn protocol_integer(value: f64, name: &'static str) -> Result<i32, DomainError> {
    let rounded = value.round();
    if !rounded.is_finite() || !(f64::from(i16::MIN)..=f64::from(i16::MAX)).contains(&rounded) {
        return Err(DomainError::NumericOverflow(name));
    }
    Ok(rounded as i32)
}

// WGS84 defining constants, not device calibration or an operational default.
const WGS84_A: f64 = 6_378_137.0;
const WGS84_F: f64 = 1.0 / 298.257_223_563;
const WGS84_B: f64 = WGS84_A * (1.0 - WGS84_F);
// Bounded numerical solver work, independent of user freshness/range policies.
const ITERATIONS: usize = 200;
const CONVERGENCE_RADIANS: f64 = 1.0e-12;

fn coefficients(cos_sq_alpha: f64) -> (f64, f64) {
    let u_sq = cos_sq_alpha * (WGS84_A * WGS84_A - WGS84_B * WGS84_B) / (WGS84_B * WGS84_B);
    let a = 1.0 + u_sq / 16_384.0 * (4096.0 + u_sq * (-768.0 + u_sq * (320.0 - 175.0 * u_sq)));
    let b = u_sq / 1024.0 * (256.0 + u_sq * (-128.0 + u_sq * (74.0 - 47.0 * u_sq)));
    (a, b)
}

fn sigma_delta(b: f64, sigma: f64, cos_two_sigma_m: f64) -> f64 {
    let sin_sigma = sigma.sin();
    let cos_sigma = sigma.cos();
    b * sin_sigma
        * (cos_two_sigma_m
            + b / 4.0
                * (cos_sigma * (-1.0 + 2.0 * cos_two_sigma_m.powi(2))
                    - b / 6.0
                        * cos_two_sigma_m
                        * (-3.0 + 4.0 * sin_sigma.powi(2))
                        * (-3.0 + 4.0 * cos_two_sigma_m.powi(2))))
}

fn direct_wgs84(
    origin: &GeographicPosition,
    distance: f64,
    bearing_degrees: f64,
) -> Result<GeographicPosition, DomainError> {
    if distance == 0.0 {
        return Ok(origin.clone());
    }
    let alpha1 = bearing_degrees.to_radians();
    let u1 = ((1.0 - WGS84_F) * origin.latitude.to_radians().tan()).atan();
    let (sin_u1, cos_u1) = u1.sin_cos();
    let sigma1 = u1.tan().atan2(alpha1.cos());
    let sin_alpha = cos_u1 * alpha1.sin();
    let cos_sq_alpha = 1.0 - sin_alpha * sin_alpha;
    let (a, b) = coefficients(cos_sq_alpha);
    let baseline = distance / (WGS84_B * a);
    if !baseline.is_finite() {
        return Err(DomainError::Geodesic);
    }
    let mut sigma = baseline;
    let mut converged = false;
    for _ in 0..ITERATIONS {
        let updated = baseline + sigma_delta(b, sigma, (2.0 * sigma1 + sigma).cos());
        if (updated - sigma).abs() <= CONVERGENCE_RADIANS {
            sigma = updated;
            converged = true;
            break;
        }
        sigma = updated;
    }
    if !converged {
        return Err(DomainError::Geodesic);
    }
    let (sin_sigma, cos_sigma) = sigma.sin_cos();
    let temporary = sin_u1 * sin_sigma - cos_u1 * cos_sigma * alpha1.cos();
    let latitude = (sin_u1 * cos_sigma + cos_u1 * sin_sigma * alpha1.cos())
        .atan2((1.0 - WGS84_F) * (sin_alpha * sin_alpha + temporary * temporary).sqrt());
    let lambda =
        (sin_sigma * alpha1.sin()).atan2(cos_u1 * cos_sigma - sin_u1 * sin_sigma * alpha1.cos());
    let c = WGS84_F / 16.0 * cos_sq_alpha * (4.0 + WGS84_F * (4.0 - 3.0 * cos_sq_alpha));
    let cos_two_sigma_m = (2.0 * sigma1 + sigma).cos();
    let longitude_delta = lambda
        - (1.0 - c)
            * WGS84_F
            * sin_alpha
            * (sigma
                + c * sin_sigma
                    * (cos_two_sigma_m + c * cos_sigma * (-1.0 + 2.0 * cos_two_sigma_m.powi(2))));
    let result = GeographicPosition {
        latitude: latitude.to_degrees(),
        longitude: (origin.longitude + longitude_delta.to_degrees() + 180.0).rem_euclid(360.0)
            - 180.0,
        altitude_m: origin.altitude_m,
    };
    result.validate()?;
    Ok(result)
}

#[allow(clippy::float_cmp)] // Exact identical coordinates mean zero displacement.
fn inverse_wgs84(
    origin: &GeographicPosition,
    point: &GeographicPosition,
) -> Result<(f64, f64), DomainError> {
    if origin.latitude == point.latitude && origin.longitude == point.longitude {
        return Ok((0.0, 0.0));
    }
    let u1 = ((1.0 - WGS84_F) * origin.latitude.to_radians().tan()).atan();
    let u2 = ((1.0 - WGS84_F) * point.latitude.to_radians().tan()).atan();
    let (sin_u1, cos_u1) = u1.sin_cos();
    let (sin_u2, cos_u2) = u2.sin_cos();
    let longitude_delta = (point.longitude - origin.longitude).to_radians();
    let mut lambda = longitude_delta;
    let mut solution = None;
    for _ in 0..ITERATIONS {
        let (sin_lambda, cos_lambda) = lambda.sin_cos();
        let sin_sigma = ((cos_u2 * sin_lambda).powi(2)
            + (cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_lambda).powi(2))
        .sqrt();
        let cos_sigma = sin_u1 * sin_u2 + cos_u1 * cos_u2 * cos_lambda;
        if sin_sigma <= f64::EPSILON {
            // Antipodes have no unique initial bearing; never call them co-located.
            return if cos_sigma > 0.0 {
                Ok((0.0, 0.0))
            } else {
                Err(DomainError::Geodesic)
            };
        }
        let sigma = sin_sigma.atan2(cos_sigma);
        let sin_alpha = cos_u1 * cos_u2 * sin_lambda / sin_sigma;
        let cos_sq_alpha = 1.0 - sin_alpha * sin_alpha;
        let cos_two_sigma_m = if cos_sq_alpha <= f64::EPSILON {
            0.0
        } else {
            cos_sigma - 2.0 * sin_u1 * sin_u2 / cos_sq_alpha
        };
        let c = WGS84_F / 16.0 * cos_sq_alpha * (4.0 + WGS84_F * (4.0 - 3.0 * cos_sq_alpha));
        let updated = longitude_delta
            + (1.0 - c)
                * WGS84_F
                * sin_alpha
                * (sigma
                    + c * sin_sigma
                        * (cos_two_sigma_m
                            + c * cos_sigma * (-1.0 + 2.0 * cos_two_sigma_m.powi(2))));
        if (updated - lambda).abs() <= CONVERGENCE_RADIANS {
            solution = Some((sigma, cos_sq_alpha, cos_two_sigma_m, lambda));
            break;
        }
        lambda = updated;
    }
    let (sigma, cos_sq_alpha, cos_two_sigma_m, lambda) = solution.ok_or(DomainError::Geodesic)?;
    let (a, b) = coefficients(cos_sq_alpha);
    let distance = WGS84_B * a * (sigma - sigma_delta(b, sigma, cos_two_sigma_m));
    let bearing = (cos_u2 * lambda.sin()).atan2(cos_u1 * sin_u2 - sin_u1 * cos_u2 * lambda.cos());
    if !distance.is_finite() || !bearing.is_finite() {
        return Err(DomainError::Geodesic);
    }
    Ok((distance, bearing.to_degrees().rem_euclid(360.0)))
}
