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

impl<T> CVec<T> {
    pub fn empty() -> Self {
        Self {
            items: std::ptr::null(),
            items_end: std::ptr::null(),
            items_rsv_end: std::ptr::null(),
        }
    }

    /// Returns the number of items in the CVec.
    pub fn len(&self) -> usize {
        if self.items.is_null() || self.items_end.is_null() {
            return 0;
        }

        let count = unsafe { self.items_end.offset_from(self.items) };
        if count < 0 {
            log::error!("CVec has negative item count, returning length 0");
            return 0;
        }

        count as usize
    }

    /// Returns true if the CVec is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
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
            return Vec::new();
        }

        let count = unsafe { cvec.items_end.offset_from(cvec.items) };
        if count < 0 {
            log::error!("CVec has negative item count, returning empty Vec");
            return Vec::new();
        }

        if count == 0 {
            return Vec::new();
        }

        if count > 10_000 {
            log::warn!("CVec has an unusually high number of items: {}", count);
        }

        let mut result = Vec::with_capacity(count as usize);

        unsafe {
            let mut current = cvec.items;

            while current < cvec.items_end {
                if current.is_null() {
                    break;
                }

                result.push(&(*current));
                current = current.add(1);
            }
        }

        if result.len() < count as usize {
            log::warn!(
                "CVec expected to have {} items, but only read {} items",
                count,
                result.len()
            );
        }

        result
    }
}
