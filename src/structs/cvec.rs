#[derive(Debug, Clone, Copy)]
struct CVec<T> {
    /// Pointer to the start of the items array.
    items: *const T,
    /// Pointer to the end of the items array.
    items_end: *const T,
    /// Pointer to the reserved end of the items array.
    items_rsv_end: *const T,
}
