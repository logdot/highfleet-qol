//! User-facing settings for HighFleet QOL.
//!
//! The mod normally reads these settings from `Modloader/config/qol.json`.
//! If that file does not exist, QOL creates one using [`Config::default`].
//! Most users can change the generated JSON file directly without needing to work with Rust code.

use std::{collections::HashMap, error::Error, fmt, num::NonZeroU32};

use highfleet::general::EscadraString;
use serde::{
    de::{self, Deserializer, Visitor},
    Deserialize, Serialize,
};

mod elevation_rules;
mod loadout;
mod plane;
mod refuel;
mod speed;

pub use elevation_rules::ElevationRules;
pub use loadout::{ConfigItemMunition, ConfigLoadout};
pub use plane::{ConfigPlane, DEFAULT_PLANE_HEALTH};
pub use refuel::RefuelConfig;
pub use speed::{SpeedConfig, SpeedCurve, SpeedPreset};

use crate::plane as plane_runtime;

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
    #[serde(default = "default_enabled")]
    pub enable_flare_crash_fix: bool,
    /// Scales the engines' anti-tilt torque by the frame time, so it behaves the same at any frame rate.
    /// This does not change anything at the vanilla 50 fps.
    ///
    /// Enabled by default.
    /// Turn it off if another mod already applies the same frame-time factor to that torque.
    #[serde(default = "default_enabled")]
    pub enable_antitilt_fps_fix: bool,
    /// Restore the native intro, skippable with a fresh key press or click.
    /// Steam 1.163 only; disabled by default.
    #[serde(default)]
    pub enable_intro: bool,
    /// Show gun pre-fire warning lines for both sides.
    /// Enabled by default; Steam 1.163 only.
    #[serde(default = "default_enabled")]
    pub enemy_laser_warning: bool,
    /// Play the enemy pre-fire sound warning.
    /// Enabled by default; Steam 1.163 only.
    #[serde(default = "default_enabled")]
    pub enemy_sound_warning: bool,
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
    /// Settings and loadouts for each aircraft type.
    ///
    /// Each key is an aircraft's internal name.
    /// The default configuration copies the aircraft and loadout names from the game.
    /// The standard `30.0` health is automatically assigned.
    pub planes: HashMap<EscadraString, ConfigPlane>,
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
    /// Part menus to add to city shops and the shipworks.
    ///
    /// Each entry is a bar (an `m_oid` with stats) plus the parts it offers.
    /// A menu with a vanilla bar oid (`ITEM_HULL`, `ITEM_ARMOR`, `ITEM_FUEL`, `ITEM_AMMO` or `ITEM_LEG`) adds its parts to that existing menu.
    /// Every other bar oid becomes a new part menu with its own stock.
    #[serde(default)]
    pub part_menus: Vec<PartMenuConfig>,
    /// Multiplies the money received when selling ship parts.
    ///
    /// `1.0` gives the normal amount, `2.0` doubles it, and `0.5` halves it.
    #[serde(default = "default_sell_multiplier")]
    pub sell_multiplier: f32,
    /// Changes how fast fleets refuel at stations. Only has an effect on Steam 1.163.
    ///
    /// Uses the game's own rate by default.
    #[serde(default)]
    pub refuel: RefuelConfig,
    /// Changes the map cruise-speed formula. Only has an effect on Steam 1.163.
    ///
    /// Uses the game's own curve by default.
    #[serde(default)]
    pub speed: SpeedConfig,
    /// Chooses the firing-arc rules on Steam 1.163.
    /// `classic` restores the 1.151 rules.
    /// These cover free-standing parts, engines and the KAZ intercept cone.
    /// `modern` uses the 1.163 rules, which make those checks stricter.
    /// `vanilla` uses the game's own rules and is the default.
    #[serde(default)]
    pub elevation_rules: ElevationRules,
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

fn default_enabled() -> bool {
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

/// One custom part menu: a shop bar (and optional shipworks button) plus the parts it offers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartMenuConfig {
    /// `m_oid` of the shop bar / constructor button.
    ///
    /// For a new menu the oid must exist in `parts.seria` and have a record in `OL.seria`.
    /// That record supplies the icon and name.
    /// For a vanilla bar oid (`ITEM_HULL`, `ITEM_ARMOR`, `ITEM_FUEL`, `ITEM_AMMO` or `ITEM_LEG`) the parts are added to that existing menu instead.
    pub bar: String,
    /// Parts offered by the menu, in order.
    ///
    /// A bare string uses the part's native resource count for a vanilla bar and one unit for a custom resource.
    /// The object form accepts an explicit `cost`:
    ///
    /// ```json
    /// "parts": ["MDL_QUARTERS_01", { "oid": "MDL_GENERATOR_01", "cost": 2 }]
    /// ```
    #[serde(default)]
    pub parts: Vec<PartMenuPart>,
    /// Which resource the menu spends.
    ///
    /// Omitted means "auto": a vanilla bar uses its own resource and any other bar gets a new custom resource.
    /// `"none"` disables resource gating (the parts cost money only).
    /// `"custom"` forces a new custom resource.
    /// `"hull"`, `"armor"`, `"fuel"`, `"ammo"`, `"leg"` or the matching numeric id (`10`, `20`, `30`, `40`, `50`) select a vanilla resource; the bar oid must then be the matching `ITEM_*` bar.
    #[serde(default)]
    pub resource: Option<PartMenuResource>,
    /// Spawn rules, one object or an array of objects.
    ///
    /// The first rule whose `city_types` matches the current city is used; an empty list matches every city type.
    /// For a new bar `min_parts`/`max_parts` are its starting stock and `probability` decides whether it appears.
    /// For a vanilla bar the stock is the game's, so only `probability` and `city_types` are used.
    #[serde(default, deserialize_with = "deserialize_part_menu_rules")]
    pub rules: Vec<PartMenuRule>,
}

/// One part inside a [`PartMenuConfig`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PartMenuPart {
    /// A part id.
    ///
    /// A vanilla bar uses the part's native resource count; a custom resource uses one unit.
    Name(String),
    /// A part id with an explicit resource cost.
    Detailed {
        /// Part id (`m_oid` in `parts.seria`).
        oid: String,
        /// Resource units one copy costs.
        /// It must be at least 1.
        #[serde(default = "default_menu_cost")]
        cost: NonZeroU32,
    },
}

impl PartMenuPart {
    /// Returns the part id and its explicit resource cost, if one was given.
    #[must_use]
    pub fn oid_and_cost(&self) -> (&str, Option<NonZeroU32>) {
        match self {
            Self::Name(oid) => (oid, None),
            Self::Detailed { oid, cost } => (oid, Some(*cost)),
        }
    }
}

/// The resource a [`PartMenuConfig`] spends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartMenuResource {
    /// No resource gating; the parts cost money only.
    None,
    /// A new custom resource allocated at startup.
    Custom,
    /// A vanilla resource id: `10` hull, `20` armor, `30` fuel, `40` ammo, `50` leg.
    Vanilla(u32),
    /// A specific unused custom resource id.
    CustomId(u32),
}

impl<'de> Deserialize<'de> for PartMenuResource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ResourceVisitor;

        impl Visitor<'_> for ResourceVisitor {
            type Value = PartMenuResource;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a part-menu resource name or id")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                match value.to_ascii_lowercase().as_str() {
                    "none" => Ok(PartMenuResource::None),
                    "custom" => Ok(PartMenuResource::Custom),
                    "hull" => Ok(PartMenuResource::Vanilla(10)),
                    "armor" => Ok(PartMenuResource::Vanilla(20)),
                    "fuel" => Ok(PartMenuResource::Vanilla(30)),
                    "ammo" => Ok(PartMenuResource::Vanilla(40)),
                    "leg" => Ok(PartMenuResource::Vanilla(50)),
                    _ => Err(E::custom(format!("unknown part-menu resource \"{value}\""))),
                }
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let id = u32::try_from(value)
                    .map_err(|_| E::custom("part-menu resource id is too large"))?;
                match id {
                    10 | 20 | 30 | 40 | 50 => Ok(PartMenuResource::Vanilla(id)),
                    61..=98 => Ok(PartMenuResource::CustomId(id)),
                    _ => Err(E::custom(format!(
                        "part-menu resource id {id} is not a vanilla id and is outside 61..=98"
                    ))),
                }
            }
        }

        deserializer.deserialize_any(ResourceVisitor)
    }
}

impl Serialize for PartMenuResource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::None => serializer.serialize_str("none"),
            Self::Custom => serializer.serialize_str("custom"),
            Self::Vanilla(id) | Self::CustomId(id) => serializer.serialize_u32(*id),
        }
    }
}

/// One spawn rule for a [`PartMenuConfig`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartMenuRule {
    /// Chance that the menu appears in a matching city.
    ///
    /// `1.0` always appears and `0.25` is a 25% chance.
    #[serde(default = "default_menu_probability")]
    pub probability: f32,
    /// Smallest stock the new bar can start with.
    #[serde(default = "default_menu_count")]
    pub min_parts: u32,
    /// Largest stock the new bar can start with.
    #[serde(default = "default_menu_count")]
    pub max_parts: u32,
    /// City type numbers where this rule applies; empty means every city type.
    #[serde(default)]
    pub city_types: Vec<u32>,
}

/// Accepts one [`PartMenuRule`] or an array of them.
fn deserialize_part_menu_rules<'de, D>(deserializer: D) -> Result<Vec<PartMenuRule>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        Many(Vec<PartMenuRule>),
        One(PartMenuRule),
    }

    let raw: Option<OneOrMany> = Option::deserialize(deserializer)?;
    Ok(match raw {
        None => Vec::new(),
        Some(OneOrMany::Many(rules)) => rules,
        Some(OneOrMany::One(rule)) => vec![rule],
    })
}

fn default_menu_probability() -> f32 {
    1.0
}

fn default_menu_count() -> u32 {
    1
}

fn default_menu_cost() -> NonZeroU32 {
    NonZeroU32::new(1).expect("1 is nonzero")
}

impl Default for Config {
    fn default() -> Self {
        let plane_config = plane_runtime::get_planes();

        Self {
            enable_anti_wobble: false,
            enable_unblocked_guns: false,
            enable_reduced_shake: false,
            enable_flare_crash_fix: true,
            enable_antitilt_fps_fix: true,
            enable_intro: false,
            enemy_laser_warning: true,
            enemy_sound_warning: true,
            enable_unblocked_ttl: false,
            enable_arcade_zoom: true,
            max_zoom_level: 5,
            min_zoom_level: 3,
            zoom_levels: vec![14.0, 7.0, 1.0, 0.7, 0.5, 0.3],
            planes: plane_config,
            enable_shop_parts: false,
            shop_parts: HashMap::new(),
            part_menus: Vec::new(),
            sell_multiplier: 1.0,
            refuel: RefuelConfig::default(),
            speed: SpeedConfig::default(),
            elevation_rules: ElevationRules::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A `qol.json` written before the `refuel` and `speed` sections existed.
    const OLD_CONFIG: &str = r#"{
        "enable_anti_wobble": false,
        "enable_unblocked_guns": false,
        "enable_reduced_shake": true,
        "enable_arcade_zoom": true,
        "max_zoom_level": 5,
        "min_zoom_level": 3,
        "zoom_levels": [14.0, 7.0, 1.0, 0.7, 0.5, 0.3],
        "planes": {},
        "enable_shop_parts": false
    }"#;

    #[test]
    fn config_without_refuel_and_speed_sections_still_loads() {
        let config: Config = serde_json::from_str(OLD_CONFIG).unwrap();

        assert!(config.enable_reduced_shake);
        assert_eq!(config.refuel.player_rate_multiplier, 1.0);
        assert_eq!(config.speed.preset, SpeedPreset::Vanilla);
    }

    #[test]
    fn refuel_and_speed_sections_are_read() {
        let json = OLD_CONFIG.replacen(
            "\"planes\"",
            r#""refuel": {"player_rate_multiplier": 4.0},
        "speed": {"preset": "custom", "exponent": 0.75},
        "planes""#,
            1,
        );
        let config: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(config.refuel.player_rate_multiplier, 4.0);
        assert_eq!(config.refuel.ai_rate_multiplier, 1.0);
        assert_eq!(config.speed.preset, SpeedPreset::Custom);
        assert_eq!(config.speed.exponent, 0.75);
    }

    #[test]
    fn part_menus_accept_names_costs_and_single_or_multiple_rules() {
        let json = OLD_CONFIG.replacen(
            "\"planes\"",
            r#""part_menus": [
            {
                "bar": "ITEM_PAPERS",
                "parts": ["MDL_QUARTERS_01", { "oid": "MDL_GENERATOR_01", "cost": 2 }],
                "resource": "custom",
                "rules": { "probability": 0.5, "min_parts": 4, "max_parts": 12, "city_types": [3] }
            },
            {
                "bar": "ITEM_ARMOR",
                "parts": ["MDL_ARMOR1X1_01"],
                "rules": [
                    { "probability": 1.0 },
                    { "probability": 0.25, "city_types": [7] }
                ]
            }
        ],
        "planes""#,
            1,
        );
        let config: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(config.part_menus.len(), 2);

        let papers = &config.part_menus[0];
        assert_eq!(papers.bar, "ITEM_PAPERS");
        assert_eq!(papers.parts.len(), 2);
        assert_eq!(papers.parts[0].oid_and_cost(), ("MDL_QUARTERS_01", None));
        assert_eq!(
            papers.parts[1].oid_and_cost(),
            ("MDL_GENERATOR_01", NonZeroU32::new(2))
        );
        assert_eq!(papers.rules.len(), 1);
        assert_eq!(papers.rules[0].min_parts, 4);
        assert_eq!(papers.rules[0].max_parts, 12);

        let armor = &config.part_menus[1];
        assert!(armor.resource.is_none());
        assert_eq!(armor.rules.len(), 2);
        assert_eq!(armor.rules[1].probability, 0.25);
        assert_eq!(armor.rules[1].city_types, vec![7]);
    }

    #[test]
    fn part_menu_resource_accepts_a_name_or_an_id() {
        let json = OLD_CONFIG.replacen(
            "\"planes\"",
            r#""part_menus": [
            { "bar": "ITEM_HULL", "parts": [], "resource": "none" },
            { "bar": "ITEM_AMMO", "parts": [], "resource": 40 }
        ],
        "planes""#,
            1,
        );
        let config: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(config.part_menus[0].resource, Some(PartMenuResource::None));
        assert_eq!(
            config.part_menus[1].resource,
            Some(PartMenuResource::Vanilla(40))
        );
        assert!(config.part_menus[0].rules.is_empty());
    }
}
