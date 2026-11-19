use serde::{Deserialize, Serialize};

/// Which firing-arc rules the game uses, for [`Config::elevation_rules`](super::Config).
///
/// Elevation is the height level (`m_floor`) of a part and affects which parts can block a gun's firing arc.
/// The KAZ module has its own arc tests.
/// Steam 1.163 made both checks stricter than Steam 1.151.
/// This setting only has an effect on Steam 1.163.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ElevationRules {
    /// The game's own rules for the running version: [`Classic`](Self::Classic) on 1.151 and [`Modern`](Self::Modern) on 1.163.
    /// Only 1.163 is patched, so this leaves the game's rules unchanged.
    #[default]
    Vanilla,
    /// The 1.151 rules.
    /// Free-standing parts (swivel guns, engines, legs) do not block a gun on the same height level.
    /// KAZ accepts targets within 68.75 degrees of its facing and is blocked when more than 50% of its 34-degree rear cone is covered.
    Classic,
    /// The 1.163 rules, with stricter free-standing-part and KAZ checks than [`Classic`](Self::Classic).
    /// This leaves the game's rules unchanged and does not enable blocked guns.
    Modern,
}

impl ElevationRules {
    /// Whether the 1.151 rules should be patched in.
    pub fn is_classic(self) -> bool {
        self == Self::Classic
    }
}
