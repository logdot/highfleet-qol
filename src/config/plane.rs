use serde::{de::Deserializer, Deserialize, Serialize};

use super::ConfigLoadout;

/// Health assigned to an aircraft when no custom value is configured.
pub const DEFAULT_PLANE_HEALTH: f32 = 30.0;

/// Settings and loadouts for one aircraft type.
///
/// Older configuration files that store a loadout list directly are still accepted.
/// Those entries use [`DEFAULT_PLANE_HEALTH`].
#[derive(Debug, Clone, Serialize)]
pub struct ConfigPlane {
    /// Current and maximum health assigned whenever this aircraft launches.
    ///
    /// The value must be greater than zero. The default is `30.0`.
    pub health: f32,
    /// Loadouts available to this aircraft.
    pub loadouts: Vec<ConfigLoadout>,
}

impl Default for ConfigPlane {
    fn default() -> Self {
        Self {
            health: DEFAULT_PLANE_HEALTH,
            loadouts: Vec::new(),
        }
    }
}

impl From<Vec<ConfigLoadout>> for ConfigPlane {
    fn from(loadouts: Vec<ConfigLoadout>) -> Self {
        Self {
            loadouts,
            ..Self::default()
        }
    }
}

impl<'de> Deserialize<'de> for ConfigPlane {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Current {
                #[serde(default = "default_plane_health")]
                health: f32,
                loadouts: Vec<ConfigLoadout>,
            },
            Legacy(Vec<ConfigLoadout>),
        }

        match Wire::deserialize(deserializer)? {
            Wire::Current { health, loadouts } => Ok(Self { health, loadouts }),
            Wire::Legacy(loadouts) => Ok(loadouts.into()),
        }
    }
}

fn default_plane_health() -> f32 {
    DEFAULT_PLANE_HEALTH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_legacy_plane_loadout_arrays_with_default_health() {
        let plane: ConfigPlane = serde_json::from_str("[]").unwrap();

        assert_eq!(plane.health, DEFAULT_PLANE_HEALTH);
        assert!(plane.loadouts.is_empty());
    }

    #[test]
    fn deserializes_current_plane_settings() {
        let plane: ConfigPlane = serde_json::from_str(r#"{"health":45.0,"loadouts":[]}"#).unwrap();

        assert_eq!(plane.health, 45.0);
        assert!(plane.loadouts.is_empty());
    }

    #[test]
    fn defaults_omitted_health_and_serializes_the_current_shape() {
        let plane: ConfigPlane = serde_json::from_str(r#"{"loadouts":[]}"#).unwrap();
        let serialized = serde_json::to_value(&plane).unwrap();

        assert_eq!(plane.health, DEFAULT_PLANE_HEALTH);
        assert_eq!(serialized["health"], DEFAULT_PLANE_HEALTH);
        assert!(serialized["loadouts"].as_array().unwrap().is_empty());
    }
}
