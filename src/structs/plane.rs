use serde::{Serialize, Serializer};

use crate::structs::{cvec::CVec, loadout::GameLoadout};

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
        let loadouts_vec: Vec<&GameLoadout> = unsafe {
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
