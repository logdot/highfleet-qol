use serde::Serialize;

#[derive(Debug, Clone, Copy)]
/// Basically a c std::Vector
pub struct CVec<T> {
    /// Pointer to the start of the items array.
    pub items: *const T,
    /// Pointer to the end of the items array.
    pub items_end: *const T,
    /// Pointer to the reserved end of the items array.
    pub items_rsv_end: *const T,
}

impl<T: Serialize> Serialize for CVec<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let vec: Vec<&T> = self.into();
        vec.serialize(serializer)
    }
}

impl<T> From<&CVec<T>> for Vec<&T> {
    fn from(cvec: &CVec<T>) -> Self {
        if cvec.items.is_null() || cvec.items_end.is_null() {
            log::warn!("CVec has null pointers, returning empty Vec");
            return Vec::new();
        }

        let count = unsafe { cvec.items_end.offset_from(cvec.items) };
        log::info!("CVec has {} items", count);
        if count < 0 {
            log::error!("CVec has negative item count, returning empty Vec");
            return Vec::new();
        }

        if count == 0 {
            log::info!("CVec has zero items, returning empty Vec");
            return Vec::new();
        }

        if count > 10_000 {
            log::warn!("CVec has an unusually high number of items: {}", count);
        }

        let mut result = Vec::with_capacity(count as usize);

        unsafe {
            let mut current = cvec.items;
            let mut items_read = 0;

            while current < cvec.items_end {
                if current.is_null() {
                    log::warn!(
                        "Encountered null pointer in CVec at index {}, stopping read",
                        items_read
                    );
                    break;
                }

                result.push(&(*current));
                current = current.add(1);
                items_read += 1;
            }
        }

        log::info!("Converted CVec to Vec with {} items", result.len());
        result
    }
}
