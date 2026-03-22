use std::{collections::HashMap, error::Error};

use highfleet::v1_163::EscadraString;
use serde::{de::Deserializer, Deserialize, Serialize};

use crate::{plane, structs::loadout::Loadout};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub enable_anti_wobble: bool,
    pub enable_unblocked_guns: bool,
    pub enable_reduced_shake: bool,
    #[serde(default)]
    pub enable_unblocked_ttl: bool,
    pub enable_arcade_zoom: bool,
    pub max_zoom_level: u8,
    pub min_zoom_level: u8,
    pub zoom_levels: Vec<f32>,
    pub planes: HashMap<EscadraString, Vec<Loadout>>,
    pub enable_shop_parts: bool,
    #[serde(default, deserialize_with = "deserialize_shop_parts")]
    pub shop_parts: HashMap<String, Vec<ShopPart>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShopPart {
    pub probability: f32,
    pub min_parts: u32,
    pub max_parts: u32,
    /// Optional list of city types (1–7) where this part can appear.
    /// If empty or omitted, the part appears in all city types.
    #[serde(default)]
    pub city_types: Vec<u32>,
}

/// Accepts either a single `ShopPart` object or an array of `ShopPart` objects.
/// Used via `#[serde(untagged)]` so serde tries each variant in declaration order.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    Many(Vec<ShopPart>),
    One(ShopPart),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<ShopPart> {
        match self {
            OneOrMany::One(part) => vec![part],
            OneOrMany::Many(parts) => parts,
        }
    }
}

/// Custom deserializer for `shop_parts` that accepts each value as either a
/// single `ShopPart` object or an array of `ShopPart` objects, allowing both
/// formats to coexist in the same config file.
fn deserialize_shop_parts<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, Vec<ShopPart>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: HashMap<String, OneOrMany> = HashMap::deserialize(deserializer)?;
    Ok(raw.into_iter().map(|(k, v)| (k, v.into_vec())).collect())
}

impl Default for Config {
    fn default() -> Self {
        let plane_config = plane::get_planes();

        Self {
            enable_anti_wobble: false,
            enable_unblocked_guns: false,
            enable_reduced_shake: false,
            enable_unblocked_ttl: false,
            enable_arcade_zoom: true,
            max_zoom_level: 5,
            min_zoom_level: 3,
            zoom_levels: vec![14.0, 7.0, 1.0, 0.7, 0.5, 0.3],
            planes: plane_config,
            enable_shop_parts: false,
            shop_parts: HashMap::new(),
        }
    }
}

impl Config {
    pub fn load(path: &str) -> Result<Self, Box<dyn Error>> {
        let config_str = std::fs::read_to_string(path)?;

        Ok(serde_json::from_str(&config_str)?)
    }

    pub fn save(&self, path: &str) -> Result<(), Box<dyn Error>> {
        let config_str = serde_json::to_string_pretty(self)?;

        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap())?;
        std::fs::write(path, config_str)?;

        Ok(())
    }
}
