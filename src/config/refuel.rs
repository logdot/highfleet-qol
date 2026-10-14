use serde::{Deserialize, Deserializer, Serialize};

/// Settings for the `refuel` section of `qol.json`.
///
/// Fleets docked at a station take `min(free tank space, time * rate, station stock)` of fuel
/// each frame. These multipliers scale the rate, so a very large value just means "fill up
/// at once"; it can never deliver more than the tank and the station allow.
///
/// Both multipliers default to `1.0`, the game's own rate, which leaves the game untouched.
/// Any finite value is accepted, including `0` (never refuel) and negative values (drain fuel).
///
/// Only has an effect on Steam 1.163.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RefuelConfig {
    /// How much faster the player's fleet refuels. `1.0` is the normal rate.
    #[serde(deserialize_with = "finite_multiplier")]
    pub player_rate_multiplier: f32,
    /// The same for AI fleets, which use the same code as the player. `1.0` is the normal rate.
    #[serde(deserialize_with = "finite_multiplier")]
    pub ai_rate_multiplier: f32,
}

impl Default for RefuelConfig {
    fn default() -> Self {
        Self {
            player_rate_multiplier: 1.0,
            ai_rate_multiplier: 1.0,
        }
    }
}

/// Reads an `f32` and rejects values that are not finite.
fn finite_multiplier<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f32, D::Error> {
    let value = f32::deserialize(deserializer)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(
            "multiplier must be a finite number",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_fields_use_defaults() {
        let config: RefuelConfig = serde_json::from_str("{}").unwrap();

        assert_eq!(config.player_rate_multiplier, 1.0);
        assert_eq!(config.ai_rate_multiplier, 1.0);
    }

    #[test]
    fn partial_section_keeps_other_defaults() {
        let config: RefuelConfig =
            serde_json::from_str(r#"{"player_rate_multiplier":5.0}"#).unwrap();

        assert_eq!(config.player_rate_multiplier, 5.0);
        assert_eq!(config.ai_rate_multiplier, 1.0);
    }

    #[test]
    fn zero_and_negative_multipliers_are_allowed() {
        let config: RefuelConfig =
            serde_json::from_str(r#"{"player_rate_multiplier":0.0,"ai_rate_multiplier":-2.0}"#)
                .unwrap();

        assert_eq!(config.player_rate_multiplier, 0.0);
        assert_eq!(config.ai_rate_multiplier, -2.0);
    }

    #[test]
    fn non_numbers_are_rejected() {
        assert!(
            serde_json::from_str::<RefuelConfig>(r#"{"player_rate_multiplier":"fast"}"#).is_err()
        );
        assert!(serde_json::from_str::<RefuelConfig>(r#"{"ai_rate_multiplier":1e999}"#).is_err());
    }
}
