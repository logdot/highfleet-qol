use serde::{Deserialize, Serialize};

/// Ready-made speed curves for [`SpeedConfig::preset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeedPreset {
    /// The game's own curve for the running version: [`Classic`](Self::Classic) on 1.151 and
    /// [`Modern`](Self::Modern) on 1.163. Only 1.163 is supported for now, so this is
    /// [`Modern`](Self::Modern).
    Vanilla,
    /// The 1.151 curve, `25 * TWR`. Speed grows linearly with thrust-to-weight ratio.
    Classic,
    /// The 1.163 curve, `50 * (TWR - 1) ^ 0.5`.
    Modern,
    /// Use [`SpeedConfig::coefficient`], [`SpeedConfig::subtract`] and [`SpeedConfig::exponent`].
    Custom,
}

/// Settings for the `speed` section of `qol.json`.
///
/// The cruise speed on the map is `coefficient * max(TWR - subtract, 0) ^ exponent` in m/s;
/// the ship editor shows it in km/h (m/s times 3.6). Steam 1.163 uses
/// `50 * (TWR - 1) ^ 0.5`, Steam 1.151 used `25 * TWR`.
///
/// The defaults leave the game's own curve and speed limit alone.
///
/// The game caches each ship's speed, so reopen or edit a ship to see a new value.
/// Only has an effect on Steam 1.163.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpeedConfig {
    /// `vanilla` for the game's own curve, `classic` for the 1.151 curve, `modern` for the 1.163
    /// curve, or `custom` to use the three numbers below.
    pub preset: SpeedPreset,
    /// Multiplier in m/s. Used by the `custom` preset. Must not be negative.
    pub coefficient: f32,
    /// Subtracted from the thrust-to-weight ratio first. Used by the `custom` preset.
    pub subtract: f32,
    /// Power the result is raised to. Used by the `custom` preset. Must be above 0 and at most 8.
    pub exponent: f32,
    /// Speed limit in km/h. `0` means no limit.
    pub max_kmh: f32,
}

impl Default for SpeedConfig {
    fn default() -> Self {
        Self {
            preset: SpeedPreset::Vanilla,
            coefficient: 50.0,
            subtract: 1.0,
            exponent: 0.5,
            max_kmh: 0.0,
        }
    }
}

/// A validated speed curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedCurve {
    /// Multiplier in m/s.
    pub coefficient: f32,
    /// Subtracted from the thrust-to-weight ratio.
    pub subtract: f32,
    /// Power applied to the result.
    pub exponent: f32,
    /// Speed limit in m/s, or `0.0` for none.
    pub max_mps: f32,
}

impl SpeedConfig {
    /// Returns the curve these settings describe, or `None` if a value is out of range.
    pub fn curve(&self) -> Option<SpeedCurve> {
        let (coefficient, subtract, exponent) = match self.preset {
            SpeedPreset::Classic => (25.0, 0.0, 1.0),
            SpeedPreset::Vanilla | SpeedPreset::Modern => (50.0, 1.0, 0.5),
            SpeedPreset::Custom => (self.coefficient, self.subtract, self.exponent),
        };

        let valid = coefficient >= 0.0
            && exponent > 0.0
            && exponent <= 8.0
            && subtract > -1000.0
            && subtract < 1000.0
            && self.max_kmh >= 0.0
            && coefficient.is_finite()
            && self.max_kmh.is_finite();
        valid.then_some(SpeedCurve {
            coefficient,
            subtract,
            exponent,
            max_mps: self.max_kmh / 3.6,
        })
    }
}

impl SpeedCurve {
    /// Whether this is the game's own 1.163 curve with no speed limit.
    pub fn is_vanilla(&self) -> bool {
        self.coefficient == 50.0
            && self.subtract == 1.0
            && self.exponent == 0.5
            && self.max_mps == 0.0
    }

    /// Cruise speed in m/s for a thrust-to-weight ratio.
    pub fn speed_mps(&self, twr: f32) -> f32 {
        let base = twr - self.subtract;
        let mut speed = if base > 0.0 {
            self.coefficient * base.powf(self.exponent)
        } else {
            0.0
        };
        if self.max_mps > 0.0 && speed > self.max_mps {
            speed = self.max_mps;
        }
        speed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_fields_use_defaults() {
        let config: SpeedConfig = serde_json::from_str("{}").unwrap();

        assert_eq!(config.preset, SpeedPreset::Vanilla);
        assert!(config.curve().unwrap().is_vanilla());
    }

    #[test]
    fn classic_is_linear() {
        let config: SpeedConfig = serde_json::from_str(r#"{"preset":"classic"}"#).unwrap();
        let curve = config.curve().unwrap();

        assert_eq!(curve.speed_mps(2.0), 50.0);
        assert_eq!(curve.speed_mps(4.0), 100.0);
    }

    #[test]
    fn custom_reproduces_the_vanilla_1_163_curve() {
        let config: SpeedConfig = serde_json::from_str(
            r#"{"preset":"custom","coefficient":50.0,"subtract":1.0,"exponent":0.5}"#,
        )
        .unwrap();
        let curve = config.curve().unwrap();

        assert_eq!(curve.speed_mps(5.0), 100.0);
        assert_eq!(curve.speed_mps(1.0), 0.0);
        assert_eq!(curve.speed_mps(0.5), 0.0);
    }

    #[test]
    fn speed_limit_is_in_kmh() {
        let config: SpeedConfig = serde_json::from_str(r#"{"max_kmh":360.0}"#).unwrap();
        let curve = config.curve().unwrap();

        assert!((curve.speed_mps(100.0) - 100.0).abs() < 1.0e-3);
    }

    #[test]
    fn invalid_values_are_rejected() {
        for json in [
            r#"{"preset":"custom","exponent":0.0}"#,
            r#"{"preset":"custom","exponent":9.0}"#,
            r#"{"preset":"custom","coefficient":-1.0}"#,
            r#"{"preset":"custom","subtract":5000.0}"#,
            r#"{"max_kmh":-5.0}"#,
        ] {
            let config: SpeedConfig = serde_json::from_str(json).unwrap();
            assert!(config.curve().is_none(), "{json}");
        }
    }
}
