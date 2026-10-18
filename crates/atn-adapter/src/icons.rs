//! Binox marker colours and icons from REM symbols. NATO affiliation symbols
//! use the standard affiliation colours; every other symbol is generic blue.
//! Icons follow the device's observed raster format: a 40×40 RGBA PNG with
//! 30×30 content (a filled circle) centred at (5,5), here carrying the white
//! NATO frame shape, or a dot for generic symbols.
use atn_protocol::encode_rgba_png;

/// NATO affiliation of a REM catalog symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Affiliation {
    Friendly,
    Hostile,
    Neutral,
    Unknown,
    Generic,
}

impl Affiliation {
    /// REM symbol ids (r3akt-map-core catalog); unknown ids are generic.
    #[must_use]
    pub fn of_symbol(symbol: Option<&str>) -> Self {
        match symbol.map(str::to_ascii_lowercase).as_deref() {
            Some("friendly") => Self::Friendly,
            Some("hostile") => Self::Hostile,
            Some("neutral") => Self::Neutral,
            Some("unknown") => Self::Unknown,
            _ => Self::Generic,
        }
    }

    /// Binox colour index: 0 red, 2 yellow, 3 green, 4 blue.
    #[must_use]
    pub const fn binox_color(self) -> u8 {
        match self {
            Self::Hostile => 0,
            Self::Unknown => 2,
            Self::Neutral => 3,
            Self::Friendly | Self::Generic => 4,
        }
    }

    const fn rgb(self) -> [u8; 3] {
        match self {
            Self::Hostile => [255, 48, 48],
            Self::Unknown => [255, 214, 0],
            Self::Neutral => [0, 176, 64],
            Self::Friendly | Self::Generic => [0, 128, 255],
        }
    }

    /// Signed distance to the white glyph outline (negative inside).
    fn glyph(self, x: f64, y: f64) -> f64 {
        let boxed = |w: f64, h: f64| {
            let (qx, qy) = (x.abs() - w, y.abs() - h);
            qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0)
        };
        match self {
            Self::Friendly => boxed(8.0, 5.5).abs() - 1.0,
            Self::Hostile => ((x.abs() + y.abs() - 9.0) / std::f64::consts::SQRT_2).abs() - 1.0,
            Self::Neutral => boxed(6.5, 6.5).abs() - 1.0,
            // The lobes overlap at the centre; keep it free of stroke.
            Self::Unknown if x.hypot(y) < 2.5 => 5.0,
            Self::Unknown => {
                let lobe = |cx: f64, cy: f64| (x - cx).hypot(y - cy) - 4.6;
                lobe(4.4, 0.0)
                    .min(lobe(-4.4, 0.0))
                    .min(lobe(0.0, 4.4))
                    .min(lobe(0.0, -4.4))
                    .abs()
                    - 1.0
            }
            Self::Generic => x.hypot(y) - 4.0,
        }
    }

    /// The 40×40 RGBA PNG icon for this affiliation.
    ///
    /// # Errors
    /// Fails only if PNG encoding fails.
    pub fn icon_png(self) -> Result<Vec<u8>, String> {
        const SIZE: u32 = 40;
        let [red, green, blue] = self.rgb();
        let mut pixels = Vec::with_capacity(40 * 40 * 4);
        for row in 0..SIZE {
            for column in 0..SIZE {
                let x = f64::from(column) - 19.5;
                let y = f64::from(row) - 19.5;
                let disc = coverage(x.hypot(y) - 15.0);
                let white = coverage(self.glyph(x, y)) * disc;
                let blend =
                    |base: u8| -> u8 { channel(f64::from(base) * (1.0 - white) + 255.0 * white) };
                pixels.extend_from_slice(&[
                    blend(red),
                    blend(green),
                    blend(blue),
                    channel(255.0 * disc),
                ]);
            }
        }
        encode_rgba_png(SIZE, SIZE, &pixels).map_err(|e| e.to_string())
    }
}

/// Anti-aliased coverage of a signed distance (pixel units).
fn coverage(distance: f64) -> f64 {
    (0.5 - distance).clamp(0.0, 1.0)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=255
fn channel(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::Affiliation;
    use atn_protocol::{PngLimits, validate_png};

    #[test]
    fn nato_symbols_get_standard_colours_and_generic_is_blue() {
        assert_eq!(Affiliation::of_symbol(Some("hostile")).binox_color(), 0);
        assert_eq!(Affiliation::of_symbol(Some("Unknown")).binox_color(), 2);
        assert_eq!(Affiliation::of_symbol(Some("neutral")).binox_color(), 3);
        assert_eq!(Affiliation::of_symbol(Some("friendly")).binox_color(), 4);
        assert_eq!(Affiliation::of_symbol(Some("vehicle")).binox_color(), 4);
        assert_eq!(Affiliation::of_symbol(None), Affiliation::Generic);
    }

    #[test]
    fn every_icon_is_a_valid_40_by_40_png_within_device_limits() {
        let limits = PngLimits {
            max_encoded_bytes: 65_535,
            max_width: 40,
            max_height: 40,
            max_pixels: 1600,
            max_decoded_bytes: 6440,
            max_chunks: 128,
            expected_dimensions: Some((40, 40)),
        };
        for affiliation in [
            Affiliation::Friendly,
            Affiliation::Hostile,
            Affiliation::Neutral,
            Affiliation::Unknown,
            Affiliation::Generic,
        ] {
            let png = affiliation.icon_png().unwrap();
            let info = validate_png(&png, limits).unwrap();
            assert_eq!((info.width, info.height), (40, 40));
            assert!(
                png.len() < 2000,
                "{affiliation:?} icon is {} bytes",
                png.len()
            );
        }
    }
}
