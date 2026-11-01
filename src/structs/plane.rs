use highfleet::general::CVec;
use serde::{Serialize, Serializer};

use crate::structs::loadout::GameLoadout;

#[repr(C)]
#[derive(Debug, Default, Clone)]
pub struct Plane {
    pub _padding: [u8; 8],
    pub loadouts: CVec<*const GameLoadout>,
}

impl Serialize for Plane {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("Plane", 2)?;

        state.serialize_field("_padding", &self._padding)?;

        // Convert the pointer vector to serializable game loadouts.
        let loadouts_vec: Vec<&GameLoadout> = self
            .loadouts
            .as_slice()
            .iter()
            .filter_map(|&loadout| unsafe { loadout.as_ref() })
            .collect();

        state.serialize_field("loadouts", &loadouts_vec)?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structs::loadout::GameItemMunition;
    use highfleet::general::{EscadraString, Tll, TllContainer};
    use std::mem::{align_of, offset_of, size_of};

    fn loadout() -> GameLoadout {
        let mut loadout = GameLoadout {
            oid: EscadraString::from("LOADOUT_TEST_WITH_HEAP_STORAGE"),
            ..GameLoadout::default()
        };
        loadout.vec_parts.insert(GameItemMunition {
            name: EscadraString::from("ITEM_TEST_WITH_HEAP_STORAGE"),
            count: 2,
            _padding: [0; 4],
        });
        loadout
    }

    #[test]
    fn shared_containers_preserve_native_plane_and_loadout_layouts() {
        assert_eq!(size_of::<Plane>(), 0x20);
        assert_eq!(align_of::<Plane>(), 8);
        assert_eq!(offset_of!(Plane, loadouts), 8);
        assert_eq!(size_of::<GameItemMunition>(), 0x28);
        assert_eq!(offset_of!(GameItemMunition, count), 0x20);
        assert_eq!(size_of::<GameLoadout>(), 0x60);
        assert_eq!(offset_of!(GameLoadout, vec_parts), 0x40);
        assert_eq!(offset_of!(GameLoadout, launch_loadout_weight), 0x58);
        assert_eq!(offset_of!(GameLoadout, has_gun37mm), 0x5c);
        type PlaneNode = Tll<EscadraString, Plane>;
        assert_eq!(size_of::<TllContainer<EscadraString, Plane>>(), 0x10);
        assert_eq!(size_of::<PlaneNode>(), 0x60);
        assert_eq!(offset_of!(PlaneNode, key), 0x20);
        assert_eq!(offset_of!(PlaneNode, data), 0x40);
    }

    #[test]
    fn cloned_loadout_tree_owns_independent_strings_and_vectors() {
        let mut original = TllContainer::new();
        let value = loadout();
        original.insert(value.oid.clone(), value);
        let mut cloned = original.clone();
        {
            let original_values = original.get_items();
            let cloned_values = cloned.get_items();
            assert_ne!(
                original_values[0].vec_parts.as_ptr(),
                cloned_values[0].vec_parts.as_ptr()
            );
        }
        drop(original);
        let values = cloned.get_items();
        assert_eq!(values[0].oid.get_string(), "LOADOUT_TEST_WITH_HEAP_STORAGE");
        let parts = values[0].vec_parts.as_slice();
        assert_eq!(parts[0].name.get_string(), "ITEM_TEST_WITH_HEAP_STORAGE");
        assert_eq!(parts[0].count, 2);
    }

    #[test]
    fn plane_serialization_skips_nulls_and_cloning_keeps_targets_borrowed() {
        let loadout = loadout();
        let mut original = Plane::default();
        original.loadouts.insert(std::ptr::from_ref(&loadout));
        original.loadouts.insert(std::ptr::null());
        let cloned = original.clone();
        assert_ne!(original.loadouts.as_ptr(), cloned.loadouts.as_ptr());
        assert_eq!(original.loadouts.as_slice(), cloned.loadouts.as_slice());
        drop(original);
        let json = serde_json::to_value(&cloned).unwrap();
        assert_eq!(json["loadouts"].as_array().unwrap().len(), 1);
        assert_eq!(json["loadouts"][0]["vec_parts"][0]["count"], 2);
        drop(cloned);
        assert_eq!(loadout.vec_parts.len(), 1);
    }
}
