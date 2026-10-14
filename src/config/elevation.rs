use serde::{Deserialize, Serialize};

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
/// An unknown name is reported in the log and that key keeps its default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ElevationKeys {
    /// Raises the held part by one floor. The default is `U`.
    pub raise: String,
    /// Lowers the held part by one floor. The default is `P`.
    pub lower: String,
    /// Gives the held part back to the game's automatic elevation. The default is `Backslash`.
    pub auto: String,
}

impl Default for ElevationKeys {
    fn default() -> Self {
        Self {
            raise: "U".into(),
            lower: "P".into(),
            auto: "Backslash".into(),
        }
    }
}

/// Virtual-key codes and display names that were checked and are ready to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedKeys {
    /// Virtual-key code for raising.
    pub raise: u16,
    /// Virtual-key code for lowering.
    pub lower: u16,
    /// Virtual-key code for returning to automatic elevation.
    pub auto: u16,
    /// Text shown for the raise key in the hint.
    pub raise_label: String,
    /// Text shown for the lower key in the hint.
    pub lower_label: String,
    /// Text shown for the auto key in the hint.
    pub auto_label: String,
}

impl ElevationKeys {
    /// Resolves the three key names. Unknown names are logged and replaced by their defaults.
    pub fn resolve(&self) -> ResolvedKeys {
        let defaults = ElevationKeys::default();
        let pick = |setting: &str, configured: &String, default: &String| -> (u16, String) {
            match parse_key(configured) {
                Some(code) => (code, configured.trim().to_string()),
                None => {
                    log::error!(
                        "Elevation: unknown key \"{configured}\" for {setting}, using \"{default}\""
                    );
                    (parse_key(default).unwrap_or(0), default.clone())
                }
            }
        };

        let (raise, raise_label) = pick("raise", &self.raise, &defaults.raise);
        let (lower, lower_label) = pick("lower", &self.lower, &defaults.lower);
        let (auto, auto_label) = pick("auto", &self.auto, &defaults.auto);
        ResolvedKeys {
            raise,
            lower,
            auto,
            raise_label,
            lower_label,
            auto_label,
        }
    }
}

/// Where the elevation readout is shown, for [`ElevationConfig::hint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ElevationHint {
    /// Drawn inside the game with the game's own font.
    Native,
    /// A separate transparent window above the game.
    Window,
    /// No readout.
    Off,
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
    /// Highest floor a part can be raised to. The default is `3`.
    pub max_floor: i32,
    /// Moves mounted modules (guns and similar) together with the part being changed.
    ///
    /// Without it a gun can end up buried under a raised block. Enabled by default.
    pub sync_children: bool,
    /// Floor of a mounted module relative to its mount: `0` is the same floor, `1` one above.
    pub child_offset: i32,
    /// How the readout with the keys, the held part, its floor and its mounted modules is shown.
    ///
    /// `native` draws it inside the game with the game's own font, on the right of the editor.
    /// `window` uses a separate transparent window above the game instead, which can help if the
    /// native text does not appear. `off` shows nothing; the keys still work.
    pub hint: ElevationHint,
    /// Keys used for raising, lowering and returning to automatic elevation.
    pub keys: ElevationKeys,
}

impl Default for ElevationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_floor: 3,
            sync_children: true,
            child_offset: 0,
            hint: ElevationHint::Native,
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
        assert_eq!(config.hint, ElevationHint::Native);
        assert_eq!(config.keys.resolve(), ElevationKeys::default().resolve());
    }

    #[test]
    fn hint_modes_are_read_by_name() {
        for (name, mode) in [
            ("native", ElevationHint::Native),
            ("window", ElevationHint::Window),
            ("off", ElevationHint::Off),
        ] {
            let json = format!(r#"{{"hint":"{name}"}}"#);
            let config: ElevationConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(config.hint, mode);
        }
    }

    #[test]
    fn partial_keys_keep_other_defaults() {
        let config: ElevationConfig =
            serde_json::from_str(r#"{"enabled":true,"keys":{"raise":"F5"}}"#).unwrap();
        let keys = config.keys.resolve();

        assert!(config.enabled);
        assert_eq!(keys.raise, 0x74);
        assert_eq!(keys.lower, b'P' as u16);
        assert_eq!(keys.auto, 0xDC);
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
    fn unknown_key_names_fall_back_to_their_default() {
        let keys = ElevationKeys {
            raise: "Hyper".into(),
            lower: "O".into(),
            auto: "".into(),
        }
        .resolve();

        assert_eq!(keys.raise, b'U' as u16);
        assert_eq!(keys.raise_label, "U");
        assert_eq!(keys.lower, b'O' as u16);
        assert_eq!(keys.auto, 0xDC);
    }
}
