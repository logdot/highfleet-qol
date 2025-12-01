use serde::{Serialize, Serializer};

use crate::structs::{cvec::CVec, loadout::Loadout};

#[repr(C)]
#[derive(Debug, Clone)]
pub struct Plane {
    pub _padding: [u8; 8],
    pub loadouts: CVec<*const Loadout>,
}

impl Serialize for Plane {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("Plane", 2)?;

        state.serialize_field("_padding", &self._padding)?;

        // Convert CVec<*const Loadout> to Vec<Loadout> by dereferencing the pointers
        let loadouts_vec: Vec<&Loadout> = unsafe {
            if self.loadouts.items.is_null() || self.loadouts.items_end.is_null() {
                Vec::new()
            } else {
                let mut result = Vec::new();
                let mut current = self.loadouts.items;
                while current < self.loadouts.items_end {
                    if !(*current).is_null() {
                        result.push(&(**current));
                    }
                    current = current.add(1);
                }
                result
            }
        };

        state.serialize_field("loadouts", &loadouts_vec)?;
        state.end()
    }
}
