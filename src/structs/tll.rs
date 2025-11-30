use std::hash::Hash;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
/// Simple type that stores the size of the TLL and a pointer to the sentinel node.
pub struct TllContainer<T, U> {
    /// Pointer to the sentinel node.
    pub sentinel: *const Tll<T, U>,
    /// How many items are in the TLL.
    pub size: usize,
}

/// This is a c std::map
/// In other words, this is a red-black tree.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Tll<T, U> {
    /// The left child node of this node.
    /// If this is the sentinel node, this points to the leftmost node in the tree.
    pub left: *const Tll<T, U>,
    /// The parent node of this node.
    /// If this is the sentinel node, this points to the root node.
    pub parent: *const Tll<T, U>,
    /// The right child node of this node.
    /// If this is the sentinel node, this points to the rightmost node in the tree.
    pub right: *const Tll<T, U>,
    /// Is this node red?
    pub is_red: bool,
    /// Is this node the sentinel?
    /// This means it is either the root parent node or a null node when traversing the tree.
    pub is_sentinel: bool,
    _padding: [u8; 6],
    pub key: T,
    pub data: U,
}

impl<T, U> From<&Tll<T, U>> for Vec<&U> {
    fn from(tll: &Tll<T, U>) -> Self {
        let mut result = Vec::new();

        unsafe {
            if tll.is_sentinel {
                if tll.parent.is_null() || (*tll.parent).is_sentinel {
                    return Vec::new();
                }

                in_order_traverse(tll.parent, &mut result);
            } else {
                in_order_traverse(tll as *const Tll<T, U>, &mut result);
            }
        }

        result.into_iter().map(|(_, data)| data).collect()
    }
}

impl<T: Eq + Hash, U> From<&Tll<T, U>> for std::collections::HashMap<&T, &U> {
    fn from(tll: &Tll<T, U>) -> Self {
        let mut result = std::collections::HashMap::new();

        unsafe {
            if tll.is_sentinel {
                if tll.parent.is_null() || (*tll.parent).is_sentinel {
                    return result;
                }

                let mut items = Vec::new();
                in_order_traverse(tll.parent, &mut items);

                for item in items {
                    let key = item.0;
                    let data = item.1;
                    result.insert(key, data);
                }
            } else {
                let mut items = Vec::new();
                in_order_traverse(tll as *const Tll<T, U>, &mut items);

                for item in items {
                    let key = item.0;
                    let data = item.1;
                    result.insert(key, data);
                }
            }
        }

        result
    }
}

unsafe fn in_order_traverse<T, U>(node: *const Tll<T, U>, result: &mut Vec<(&T, &U)>) {
    if node.is_null() || (*node).is_sentinel {
        return;
    }

    in_order_traverse((*node).left, result);
    let node_ref = &*node;
    result.push((&node_ref.key, &node_ref.data));
    in_order_traverse((*node).right, result);
}
