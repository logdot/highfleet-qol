//! User-facing settings for HighFleet QOL.
//!
//! The mod normally reads these settings from `Modloader/config/qol.json`.
//! If that file does not exist, QOL creates one using [`Config::default`].
//! Most users can change the generated JSON file directly without needing to work with Rust code.

use std::{collections::HashMap, error::Error};

use highfleet::general::EscadraString;
use serde::{de::Deserializer, Deserialize, Serialize};

use crate::plane;
pub use crate::structs::loadout::{ConfigItemMunition, ConfigLoadout};

/// All settings available in `qol.json`.
///
/// Boolean settings are enabled with `true` and disabled with `false`.
/// The default values provide the standard QOL experience while leaving more disruptive gameplay changes turned off.
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Stops mod-added combat interface elements from moving with screen shake.
    ///
    /// Disabled by default.
    pub enable_anti_wobble: bool,
    /// Allows guns to fire through parts of the ship carrying them.
    ///
    /// Disabled by default so the normal gun-blocking rules remain active.
    pub enable_unblocked_guns: bool,
    /// Greatly reduces combat screen shake from gunfire, thrusters, and similar effects.
    ///
    /// Disabled by default.
    pub enable_reduced_shake: bool,
    /// Prevents a crash in version 1.163 when a missile fuze loses its linked target.
    ///
    /// Enabled by default. Version 1.151 does not require this fix.
    #[serde(default = "default_flare_crash_fix")]
    pub enable_flare_crash_fix: bool,
    /// Stops armor-piercing projectiles from being forced to a shorter lifetime.
    ///
    /// Disabled by default.
    #[serde(default)]
    pub enable_unblocked_ttl: bool,
    /// Enables configurable zoom levels during combat.
    ///
    /// Enabled by default.
    pub enable_arcade_zoom: bool,
    /// Highest zoom level the player can select.
    ///
    /// This level must have a matching entry in [`Config::zoom_levels`]. The default is `5`.
    pub max_zoom_level: u8,
    /// Lowest zoom level the player can select and the level used when combat begins.
    ///
    /// The default is `3`.
    pub min_zoom_level: u8,
    /// Zoom value for each level, starting with level `0`.
    ///
    /// The list must contain every level from `0` through [`Config::max_zoom_level`].
    pub zoom_levels: Vec<f32>,
    /// Loadouts available to each aircraft type.
    ///
    /// Each key is an aircraft's internal name and its value is the list of loadouts that aircraft can use.
    /// The default configuration copies these values from the game.
    pub planes: HashMap<EscadraString, Vec<ConfigLoadout>>,
    /// Adds the entries in [`Config::shop_parts`] to city shops.
    ///
    /// Disabled by default.
    pub enable_shop_parts: bool,
    /// Additional parts that can appear in city shops, grouped by internal part name.
    ///
    /// A part can have one rule or several rules.
    /// Multiple rules are useful when it should have different chances or quantities in different city types.
    #[serde(default, deserialize_with = "deserialize_shop_parts")]
    pub shop_parts: HashMap<String, Vec<ShopPart>>,
    /// Multiplies the money received when selling ship parts.
    ///
    /// `1.0` gives the normal amount, `2.0` doubles it, and `0.5` halves it.
    #[serde(default = "default_sell_multiplier")]
    pub sell_multiplier: f32,
}

/// Controls how often and in what quantity one part can appear in city shops.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShopPart {
    /// Chance that the part will be added when a shop is generated.
    ///
    /// Use a value from `0.0` (never) to `1.0` (always). For example, `0.25` is a 25% chance.
    pub probability: f32,
    /// Smallest number of copies that can be added when the chance succeeds.
    pub min_parts: u32,
    /// Largest number of copies that can be added when the chance succeeds.
    pub max_parts: u32,
    /// City type numbers where this rule is allowed to apply.
    ///
    /// Valid city types are `1` through `7`.
    /// Leave the list empty or omit it to allow the part to appear in every city type.
    ///
    /// City types:
    ///
    /// - 1 UR
    /// - 2 Repair city
    /// - 3 Merchant city
    /// - 4 Fuel city
    /// - 5 Intel city
    /// - 6 Mercenary city
    /// - 7 Fleet HQ (save city)
    #[serde(default)]
    pub city_types: Vec<u32>,
}

/// Accepts either a single `ShopPart` object or an array of `ShopPart` objects.
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

fn default_flare_crash_fix() -> bool {
    true
}

fn default_sell_multiplier() -> f32 {
    1.0
}

/// Custom deserializer for `shop_parts`.
/// It accepts each value as either a single `ShopPart` object or an array of `ShopPart` objects.
/// This allows both formats to coexist in the same config file.
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
            enable_flare_crash_fix: true,
            enable_unblocked_ttl: false,
            enable_arcade_zoom: true,
            max_zoom_level: 5,
            min_zoom_level: 3,
            zoom_levels: vec![14.0, 7.0, 1.0, 0.7, 0.5, 0.3],
            planes: plane_config,
            enable_shop_parts: false,
            shop_parts: HashMap::new(),
            sell_multiplier: 1.0,
        }
    }
}

impl Config {
    /// Loads QOL settings from a JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or does not contain valid QOL settings.
    pub fn load(path: &str) -> Result<Self, Box<dyn Error>> {
        let config_str = std::fs::read_to_string(path)?;

        Ok(serde_json::from_str(&config_str)?)
    }

    /// Saves these settings as a neatly formatted JSON file.
    ///
    /// Missing parent folders are created automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if the settings cannot be converted to JSON, a folder cannot
    /// be created, or the file cannot be written.
    pub fn save(&self, path: &str) -> Result<(), Box<dyn Error>> {
        let config_str = serde_json::to_string_pretty(self)?;

        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap())?;
        std::fs::write(path, config_str)?;

        Ok(())
    }
}
