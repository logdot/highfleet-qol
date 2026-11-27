use serde::{Deserialize, Serialize};

/// Which firing-arc rules the game uses, for [`Config::elevation_rules`](super::Config).
///
/// Elevation is the height level (`m_floor`) of a part.
/// It affects which parts can block a gun's firing arc.
/// The KAZ module has its own arc tests.
/// Steam 1.163 made both checks stricter than Steam 1.151.
/// This setting only has an effect on Steam 1.163.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ElevationRules {
    /// The game's own rules for the running version.
    /// These are [`ClassicKaz`](Self::ClassicKaz) on 1.151 and [`Modern`](Self::Modern) on 1.163.
    /// Only 1.163 is patched, so this leaves the game's rules unchanged.
    #[default]
    Vanilla,
    /// The 1.163 rules.
    /// Free-standing parts block guns on the same height level.
    /// Parts carrying engines receive no adjacency elevation.
    /// The KAZ intercept cone is pi/4 (45 degrees).
    /// This leaves the game's rules unchanged and does not enable blocked guns.
    Modern,
    /// The 1.151 part-floor rules.
    /// Free-standing parts (swivel guns, engines, legs) do not block a gun on the same height level.
    /// Parts carrying engines receive adjacency elevation like parts carrying guns.
    /// The KAZ intercept cone is left as the game has it.
    Classic,
    /// [`Classic`](Self::Classic) plus the 1.151 KAZ intercept cone (1.2 rad, about 68.75 degrees).
    ///
    /// The KAZ covered cone, threshold and sector ray start are `OL.seria` data and are not patched.
    /// For the full 1.151 KAZ, also change these fields on `MDL_KAZ`.
    /// Set `m_mdl_sectors_type=3`, `m_mdl_sectors_threshold=0.5` and `m_mdl_sectors_shift=1`.
    #[serde(rename = "classic_kaz")]
    ClassicKaz,
}

impl ElevationRules {
    /// Whether the 1.151 part-floor patches should be prepared.
    pub fn has_classic_floors(self) -> bool {
        matches!(self, Self::Classic | Self::ClassicKaz)
    }

    /// Whether the 1.151 KAZ intercept cone should be patched in.
    pub fn forces_classic_kaz_cone(self) -> bool {
        self == Self::ClassicKaz
    }
}
