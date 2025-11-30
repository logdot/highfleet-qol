use highfleet::general::EscadraString;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Simple type that stores the size of the TLL and a pointer to the sentinel node.
struct TLLContainer<T> {
    /// Pointer to the sentinel node.
    sentinel: *const TLL<T>,
    /// How many items are in the TLL.
    size: usize,
}

/// This is a c std::map
/// In other words, this is a red-black tree.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct TLL<T> {
    /// The left child node of this node.
    /// If this is the sentinel node, this points to the leftmost node in the tree.
    left: *const TLL<T>,
    /// The parent node of this node.
    /// If this is the sentinel node, this points to the root node.
    parent: *const TLL<T>,
    /// The right child node of this node.
    /// If this is the sentinel node, this points to the rightmost node in the tree.
    right: *const TLL<T>,
    /// Is this node red?
    is_red: bool,
    /// Is this node the sentinel?
    /// This means it is either the root parent node or a null node when traversing the tree.
    is_sentinel: bool,
    _padding: [u8; 6],
    data: T,
}

impl<T: Copy> From<TLL<T>> for Vec<T> {
    fn from(tll: TLL<T>) -> Self {
        let mut result = Vec::new();

        if tll.is_sentinel {
            if tll.parent.is_null() || tll.parent.is_sentinel {
                return result;
            }

            unsafe {
                in_order_traverse(tll.parent, &mut result);
            }
        } else {
            unsafe {
                in_order_traverse(&tll as *const TLL<T>, &mut result);
            }
        }

        result
    }
}

unsafe fn in_order_traverse<T: Copy>(node: *const TLL<T>, result: &mut Vec<T>) {
    if node.is_null() || (*node).is_sentinel {
        return;
    }

    in_order_traverse((*node).left, result);
    result.push((*node).data);
    in_order_traverse((*node).right, result);
}
