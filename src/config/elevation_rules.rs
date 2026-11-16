use serde::{Deserialize, Serialize};

/// Which firing-arc rules the game uses, for [`Config::elevation_rules`](super::Config).
///
/// Elevation here is the height level (`m_floor`) of a part. It decides which parts can block a
/// gun's firing arc, and the KAZ module has its own arc tests. Steam 1.163 made both stricter
/// than Steam 1.151. Only has an effect on Steam 1.163.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ElevationRules {
    /// The game's own rules for the running version: [`Classic`](Self::Classic) on 1.151 and
    /// [`Modern`](Self::Modern) on 1.163. Only 1.163 is supported for now, so this is
    /// [`Modern`](Self::Modern).
    #[default]
    Vanilla,
    /// The 1.151 rules. Free-standing parts (swivel guns, engines, legs) no longer block a gun
    /// that sits on the same height level, and the KAZ module again accepts targets within
    /// 68.75 degrees of its facing and treats its rear cone like 1.151 did (34 degrees wide,
    /// blocked above 50%).
    Classic,
    /// The 1.163 rules.
    Modern,
}

impl ElevationRules {
    /// Whether the 1.151 rules should be patched in.
    pub fn is_classic(self) -> bool {
        self == Self::Classic
    }
}
