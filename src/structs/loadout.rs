use highfleet::general::EscadraString;
use serde::{de::Deserializer, Deserialize, Serialize};

use crate::structs::cvec::CVec;

const LEGACY_GUN_AMMO: &str = "ITEM_GUN37";

#[repr(C)]
#[derive(Debug, Clone, Serialize)]
/// Represents an individual munition in the game's native layout.
pub struct GameItemMunition {
    /// Name of the item.
    pub name: EscadraString,
    /// How many of this item a plane can carry.
    pub count: u32,
    #[serde(skip)]
    pub _padding: [u8; 4],
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
/// Represents an individual munition in the QOL configuration.
pub struct ConfigItemMunition {
    /// Name of the item.
    pub name: EscadraString,
    /// How many of this item a plane can carry.
    pub count: u32,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Serialize)]
/// Represents a loadout object in the game's native TLL layout.
pub struct GameLoadout {
    pub oid: EscadraString,
    pub icon: EscadraString,
    pub vec_parts: CVec<GameItemMunition>,
    pub launch_loadout_weight: u32,
    pub has_gun37mm: bool,
    #[serde(skip)]
    pub _padding: [u8; 3],
}

#[derive(Debug, Default, Clone, Serialize)]
/// Represents a loadout in the QOL configuration.
pub struct ConfigLoadout {
    pub oid: EscadraString,
    pub icon: EscadraString,
    pub vec_parts: Vec<ConfigItemMunition>,
    pub launch_loadout_weight: u32,
    /// Ammo definition used by the aircraft's built-in gun.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gun_ammo: Option<EscadraString>,
}

impl<'de> Deserialize<'de> for ConfigLoadout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            oid: EscadraString,
            icon: EscadraString,
            vec_parts: Vec<ConfigItemMunition>,
            launch_loadout_weight: u32,
            #[serde(default)]
            gun_ammo: Option<EscadraString>,
            #[serde(default)]
            has_gun37mm: bool,
        }

        let serialized = Wire::deserialize(deserializer)?;
        let gun_ammo = serialized.gun_ammo.or_else(|| {
            serialized
                .has_gun37mm
                .then(|| EscadraString::from(LEGACY_GUN_AMMO))
        });

        Ok(Self {
            oid: serialized.oid,
            icon: serialized.icon,
            vec_parts: serialized.vec_parts,
            launch_loadout_weight: serialized.launch_loadout_weight,
            gun_ammo,
        })
    }
}

impl From<&GameItemMunition> for ConfigItemMunition {
    fn from(item: &GameItemMunition) -> Self {
        Self {
            name: item.name.clone(),
            count: item.count,
        }
    }
}

impl From<&ConfigItemMunition> for GameItemMunition {
    fn from(item: &ConfigItemMunition) -> Self {
        Self {
            name: item.name.clone(),
            count: item.count,
            _padding: [0; 4],
        }
    }
}

impl From<&GameLoadout> for ConfigLoadout {
    fn from(loadout: &GameLoadout) -> Self {
        Self {
            oid: loadout.oid.clone(),
            icon: loadout.icon.clone(),
            vec_parts: loadout
                .vec_parts
                .items()
                .into_iter()
                .map(ConfigItemMunition::from)
                .collect(),
            launch_loadout_weight: loadout.launch_loadout_weight,
            gun_ammo: loadout
                .has_gun37mm
                .then(|| EscadraString::from(LEGACY_GUN_AMMO)),
        }
    }
}

impl From<&ConfigLoadout> for GameLoadout {
    fn from(loadout: &ConfigLoadout) -> Self {
        let mut vec_parts = CVec::empty();
        for item in &loadout.vec_parts {
            vec_parts.insert(GameItemMunition::from(item));
        }

        Self {
            oid: loadout.oid.clone(),
            icon: loadout.icon.clone(),
            vec_parts,
            launch_loadout_weight: loadout.launch_loadout_weight,
            has_gun37mm: loadout.gun_ammo.is_some(),
            _padding: [0; 3],
        }
    }
}

const _: () = assert!(std::mem::size_of::<GameItemMunition>() == 0x28);
const _: () = assert!(std::mem::size_of::<GameLoadout>() == 0x60);

#[cfg(test)]
mod tests {
    use super::*;

    const LOADOUT_PREFIX: &str = r#"{
        "oid": "LOADOUT_TEST",
        "icon": "LOADOUT_ICON",
        "vec_parts": [],
        "launch_loadout_weight": 10"#;

    #[test]
    fn deserializes_explicit_gun_ammo() {
        let json = format!(r#"{LOADOUT_PREFIX}, "gun_ammo": "ITEM_GUN57"}}"#);
        let loadout: ConfigLoadout = serde_json::from_str(&json).unwrap();

        assert_eq!(
            loadout.gun_ammo.as_ref().map(EscadraString::get_string),
            Some("ITEM_GUN57")
        );
        assert!(GameLoadout::from(&loadout).has_gun37mm);
    }

    #[test]
    fn converts_legacy_gun_flag_to_explicit_ammo() {
        let json = format!(r#"{LOADOUT_PREFIX}, "has_gun37mm": true}}"#);
        let loadout: ConfigLoadout = serde_json::from_str(&json).unwrap();

        assert_eq!(
            loadout.gun_ammo.as_ref().map(EscadraString::get_string),
            Some(LEGACY_GUN_AMMO)
        );

        let serialized = serde_json::to_value(&loadout).unwrap();
        assert_eq!(serialized["gun_ammo"], LEGACY_GUN_AMMO);
        assert!(serialized.get("has_gun37mm").is_none());
    }

    #[test]
    fn leaves_loadout_without_a_gun_when_no_ammo_is_configured() {
        let json = format!(r#"{LOADOUT_PREFIX}}}"#);
        let loadout: ConfigLoadout = serde_json::from_str(&json).unwrap();

        assert!(loadout.gun_ammo.is_none());
        assert!(!GameLoadout::from(&loadout).has_gun37mm);
    }
}
