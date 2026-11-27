use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

/// Key names accepted by the `keys` settings, besides single letters, digits and `F1` to `F12`.
pub const NAMED_KEYS: &[(&str, u16)] = &[
    ("backslash", 0xDC),
    ("slash", 0xBF),
    ("comma", 0xBC),
    ("period", 0xBE),
    ("minus", 0xBD),
    ("equals", 0xBB),
    ("semicolon", 0xBA),
    ("quote", 0xDE),
    ("backquote", 0xC0),
    ("leftbracket", 0xDB),
    ("rightbracket", 0xDD),
    ("tab", 0x09),
    ("space", 0x20),
    ("pageup", 0x21),
    ("pagedown", 0x22),
    ("end", 0x23),
    ("home", 0x24),
    ("insert", 0x2D),
    ("delete", 0x2E),
];

/// Turns a key name from `qol.json` into a Windows virtual-key code.
///
/// Names are not case sensitive. A single backslash character is accepted as well, but the
/// name `Backslash` is easier to write in JSON.
pub fn parse_key(name: &str) -> Option<u16> {
    let name = name.trim();
    let lower = name.to_ascii_lowercase();

    if lower == "\\" {
        return Some(0xDC);
    }
    if let Some((_, code)) = NAMED_KEYS.iter().find(|(key, _)| *key == lower) {
        return Some(*code);
    }
    if let Some(number) = lower.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
        if (1..=12).contains(&number) {
            return Some(0x70 + number - 1);
        }
    }

    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(c.to_ascii_uppercase() as u16),
        _ => None,
    }
}

/// Keys used while holding a part in the ship editor.
///
/// Each key is a single letter or digit, `F1` to `F12`, or a name such as `Backslash`, `Tab`,
/// `Space`, `Insert`, `Delete`, `Home`, `End`, `PageUp`, `PageDown`, `Minus`, `Equals`, `Comma`,
/// `Period`, `Slash`, `Semicolon`, `Quote`, `Backquote`, `LeftBracket` or `RightBracket`.
/// Invalid key names are rejected when loading the configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ElevationKeys {
    /// Raises the held part by one floor.
    /// The default is `U`.
    pub raise: ElevationKey,
    /// Lowers the held part by one floor.
    /// The default is `P`.
    pub lower: ElevationKey,
    /// Gives the held part back to the game's automatic elevation.
    /// The default is `Backslash`.
    pub auto: ElevationKey,
}

impl Default for ElevationKeys {
    fn default() -> Self {
        Self {
            raise: ElevationKey::from_name("U").expect("valid default raise key"),
            lower: ElevationKey::from_name("P").expect("valid default lower key"),
            auto: ElevationKey::from_name("Backslash").expect("valid default auto key"),
        }
    }
}

/// A validated key binding, stored in JSON as its display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElevationKey {
    code: u16,
    label: String,
}

impl ElevationKey {
    fn from_name(name: &str) -> Option<Self> {
        let label = name.trim();
        Some(Self {
            code: parse_key(label)?,
            label: label.to_owned(),
        })
    }

    /// Returns the Windows virtual-key code.
    pub fn code(&self) -> u16 {
        self.code
    }

    /// Returns the trimmed display name.
    pub fn label(&self) -> &str {
        &self.label
    }
}

impl Serialize for ElevationKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.label)
    }
}

impl<'de> Deserialize<'de> for ElevationKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::from_name(&name)
            .ok_or_else(|| de::Error::custom(format!("unknown elevation key {name:?}")))
    }
}

/// Settings for the `elevation` section of `qol.json`.
///
/// While holding a part in the ship editor you can raise or lower its floor (elevation) by hand.
/// Elevation decides which parts can block each other's gun arcs and exhaust.
/// A ship saved with manual floors loads the same without this feature.
///
/// Only has an effect on Steam 1.163. Remove the standalone `elevation.dll` when using this;
/// the old `elevation.ini` is not read any more.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ElevationConfig {
    /// Turns manual elevation on. Disabled by default.
    pub enabled: bool,
    /// Highest floor a part can be raised to.
    /// The default is `3`.
    #[serde(deserialize_with = "deserialize_max_floor")]
    max_floor: i32,
    /// Moves mounted modules (guns and similar) together with the part being changed.
    ///
    /// Without it a gun can end up buried under a raised block. Enabled by default.
    pub sync_children: bool,
    /// Floor of a mounted module relative to its mount: `0` is the same floor, `1` one above.
    pub child_offset: i32,
    /// Keys used for raising, lowering and returning to automatic elevation.
    pub keys: ElevationKeys,
}

fn deserialize_max_floor<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i32, D::Error> {
    let floor = i32::deserialize(deserializer)?;
    if floor < 0 {
        return Err(de::Error::custom("max_floor must be non-negative"));
    }
    Ok(floor)
}

impl ElevationConfig {
    /// Returns the validated maximum floor.
    pub fn max_floor(&self) -> i32 {
        self.max_floor
    }

    /// Applies a floor offset within the allowed elevation range.
    pub fn offset_floor(&self, floor: i32, offset: i32) -> i32 {
        floor.saturating_add(offset).clamp(0, self.max_floor)
    }
}

impl Default for ElevationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_floor: 3,
            sync_children: true,
            child_offset: 0,
            keys: ElevationKeys::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_fields_use_defaults() {
        let config: ElevationConfig = serde_json::from_str("{}").unwrap();

        assert!(!config.enabled);
        assert_eq!(config.max_floor, 3);
        assert!(config.sync_children);
        assert_eq!(config.keys, ElevationKeys::default());
    }

    #[test]
    fn rejects_a_max_floor_below_the_minimum() {
        let error = serde_json::from_str::<ElevationConfig>(r#"{"max_floor":-1}"#).unwrap_err();

        assert!(error.to_string().contains("max_floor must be non-negative"));
    }

    #[test]
    fn extreme_child_offsets_still_respect_floor_bounds() {
        for offset in [i32::MIN, i32::MAX] {
            let json = format!(r#"{{"child_offset":{offset}}}"#);
            let config: ElevationConfig = serde_json::from_str(&json).unwrap();
            let expected = if offset < 0 { 0 } else { config.max_floor() };

            assert_eq!(config.offset_floor(1, config.child_offset), expected);
        }
    }

    #[test]
    fn partial_keys_keep_other_defaults() {
        let config: ElevationConfig =
            serde_json::from_str(r#"{"enabled":true,"keys":{"raise":" F5 "}}"#).unwrap();
        let keys = &config.keys;

        assert!(config.enabled);
        assert_eq!(keys.raise.code(), 0x74);
        assert_eq!(keys.lower.code(), b'P' as u16);
        assert_eq!(keys.auto.code(), 0xDC);
        assert_eq!(serde_json::to_value(keys).unwrap()["raise"], "F5");
    }

    #[test]
    fn parses_letters_digits_function_and_named_keys() {
        assert_eq!(parse_key("u"), Some(b'U' as u16));
        assert_eq!(parse_key("7"), Some(b'7' as u16));
        assert_eq!(parse_key("F1"), Some(0x70));
        assert_eq!(parse_key("f12"), Some(0x7B));
        assert_eq!(parse_key("Backslash"), Some(0xDC));
        assert_eq!(parse_key("\\"), Some(0xDC));
        assert_eq!(parse_key(" PageUp "), Some(0x21));
    }

    #[test]
    fn rejects_unknown_names() {
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("F13"), None);
        assert_eq!(parse_key("F0"), None);
        assert_eq!(parse_key("Enter2"), None);
        assert_eq!(parse_key("ab"), None);
    }

    #[test]
    fn rejects_invalid_key_names_when_loading_config() {
        for (action, name) in [("raise", "Hyper"), ("lower", "F13"), ("auto", "")] {
            let json = format!(r#"{{"keys":{{"{action}":"{name}"}}}}"#);
            let error = serde_json::from_str::<ElevationConfig>(&json).unwrap_err();

            assert!(error.to_string().contains("unknown elevation key"));
        }
    }
}
