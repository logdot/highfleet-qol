use serde::{Deserialize, Serialize};

/// Smallest accepted refuel rate multiplier.
pub const MIN_REFUEL_MULTIPLIER: f32 = 0.01;
/// Largest accepted refuel rate multiplier.
pub const MAX_REFUEL_MULTIPLIER: f32 = 10000.0;

/// Settings for the `refuel` section of `qol.json`.
///
/// Fleets docked at a station take `min(free tank space, time * rate, station stock)` of fuel
/// each frame. These multipliers scale the rate, so a very large value just means "fill up
/// at once"; it can never deliver more than the tank and the station allow.
///
/// Only has an effect on Steam 1.163.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RefuelConfig {
    /// Turns the refuel rate change on. Disabled by default.
    pub enabled: bool,
    /// How much faster the player's fleet refuels. `1.0` is the normal rate.
    ///
    /// Values are limited to `0.01` through `10000.0`.
    pub player_rate_multiplier: f32,
    /// The same for AI fleets, which use the same code as the player. `1.0` is the normal rate.
    ///
    /// Values are limited to `0.01` through `10000.0`.
    pub ai_rate_multiplier: f32,
}

impl Default for RefuelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            player_rate_multiplier: 1.0,
            ai_rate_multiplier: 1.0,
        }
    }
}

impl RefuelConfig {
    /// Returns `value` limited to the accepted range, or `1.0` if it is not a number.
    pub fn clamp_multiplier(value: f32) -> f32 {
        if value.is_nan() {
            1.0
        } else {
            value.clamp(MIN_REFUEL_MULTIPLIER, MAX_REFUEL_MULTIPLIER)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_fields_use_defaults() {
        let config: RefuelConfig = serde_json::from_str("{}").unwrap();

        assert!(!config.enabled);
        assert_eq!(config.player_rate_multiplier, 1.0);
        assert_eq!(config.ai_rate_multiplier, 1.0);
    }

    #[test]
    fn partial_section_keeps_other_defaults() {
        let config: RefuelConfig =
            serde_json::from_str(r#"{"enabled":true,"player_rate_multiplier":5.0}"#).unwrap();

        assert!(config.enabled);
        assert_eq!(config.player_rate_multiplier, 5.0);
        assert_eq!(config.ai_rate_multiplier, 1.0);
    }

    #[test]
    fn multipliers_are_clamped() {
        assert_eq!(RefuelConfig::clamp_multiplier(0.0), MIN_REFUEL_MULTIPLIER);
        assert_eq!(RefuelConfig::clamp_multiplier(1.0e9), MAX_REFUEL_MULTIPLIER);
        assert_eq!(RefuelConfig::clamp_multiplier(f32::NAN), 1.0);
        assert_eq!(RefuelConfig::clamp_multiplier(3.0), 3.0);
    }
}
