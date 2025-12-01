use std::{fmt, hash::Hash};

#[repr(C)]
#[derive(Debug, Clone)]
/// Simple type that stores the size of the TLL and a pointer to the sentinel node.
/// This wraps a std::map in C++.
pub struct TllContainer<T, U> {
    /// Pointer to the sentinel node.
    pub sentinel: *mut Tll<T, U>,
    /// How many items are in the TLL.
    pub size: usize,
}

/// These are the nodes of a C++ std::map.
/// In other words, this is a red-black tree.
///
/// The sentinel node is a special node that acts as the parent of the root node.
/// It's key and data are unused.
/// It also acts as the null node when traversing the tree.
///
/// For this reason it's assumed that a pointer will never be null, and instead will point to the sentinel node.
/// A null pointer would cause undefined behavior.
///
/// # Safety
/// There are several functions that return references to Tll nodes.
/// All references must outlive the C++ std::map that they came from.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct Tll<T, U> {
    /// The left child node of this node.
    /// If this is the sentinel node, this points to the leftmost node in the tree.
    pub left: *mut Tll<T, U>,
    /// The parent node of this node.
    /// If this is the sentinel node, this points to the root node.
    pub parent: *mut Tll<T, U>,
    /// The right child node of this node.
    /// If this is the sentinel node, this points to the rightmost node in the tree.
    pub right: *mut Tll<T, U>,
    /// Is this node red?
    pub is_red: bool,
    /// Is this node the sentinel?
    /// This means it is either the root parent node or a null node when traversing the tree.
    pub is_sentinel: bool,
    _padding: [u8; 6],
    pub key: T,
    pub data: U,
}

impl<T: Default, U: Default> TllContainer<T, U> {
    pub fn new() -> Self {
        unsafe {
            let sentinel_ptr = libc::malloc(size_of::<Tll<T, U>>()) as *mut Tll<T, U>;

            let sentinel = Tll {
                left: sentinel_ptr,
                parent: sentinel_ptr,
                right: sentinel_ptr,
                is_red: false,
                is_sentinel: true,
                _padding: [0; 6],
                key: T::default(),
                data: U::default(),
            };

            std::ptr::write(sentinel_ptr, sentinel);

            Self {
                sentinel: sentinel_ptr,
                size: 0,
            }
        }
    }
}

impl<T, U> TllContainer<T, U> {
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    pub fn len(&self) -> usize {
        self.size
    }

    /// # Safety
    /// This returns a reference to the sentinel node.
    ///
    /// We assume that the sentinel node is always valid as long as the TllContainer is valid.
    pub fn get_sentinel(&self) -> &Tll<T, U> {
        unsafe { &*self.sentinel }
    }

    pub fn get_items(&self) -> Vec<&U> {
        self.get_sentinel().into()
    }

    pub fn get_map(&self) -> std::collections::HashMap<&T, &U>
    where
        T: Eq + Hash,
    {
        self.get_sentinel().into()
    }

    pub fn insert(&mut self, key: T, value: U)
    where
        T: Ord + fmt::Debug,
    {
        unsafe {
            // Check if the tree is empty, aka there are no nodes other than the sentinel
            let sentinel = self.sentinel;
            if std::ptr::eq((*sentinel).parent, sentinel) {
                // Tree is empty, insert as root
                let new_node_ptr = Tll::new(key, value, sentinel);
                (*new_node_ptr).is_red = false; // Root must be black

                // Update sentinel to point to new root
                (*self.sentinel).parent = new_node_ptr;
                (*self.sentinel).left = new_node_ptr;
                (*self.sentinel).right = new_node_ptr;

                self.size += 1;
                return;
            }

            // Find insertion point with standard BST insertion logic
            let mut current = (*sentinel).parent;
            let mut parent = sentinel;

            while !(*current).is_sentinel {
                parent = current;
                if key < (*current).key {
                    current = (*current).left;
                } else if key > (*current).key {
                    current = (*current).right;
                } else {
                    // Key already exists, update value
                    (*current).data = value;
                    return;
                }
            }

            let new_node = Tll::new(key, value, sentinel);
            (*new_node).parent = parent;

            if (*new_node).key < (*parent).key {
                (*parent).left = new_node;
            } else {
                (*parent).right = new_node;
            }

            self.size += 1;

            self.insert_fixup(new_node);

            self.update_sentinel_min_max();
        }
    }

    unsafe fn insert_fixup(&mut self, mut node: *mut Tll<T, U>)
    where
        T: fmt::Debug,
    {
        let sentinel = self.sentinel;

        while (*(*node).parent).is_red {
            let parent = (*node).parent;
            let Some(gp) = (*node).grandparent() else {
                log::error!(
                    "Parent is red but grandparent is null. Node key: {:?}",
                    (*node).key
                );
                break;
            };

            if std::ptr::eq(parent, (*gp).left) {
                let uncle = (*gp).right;

                if (*uncle).is_red {
                    (*parent).is_red = false;
                    (*uncle).is_red = false;
                    (*gp).is_red = true;
                    node = gp;
                } else {
                    if std::ptr::eq(node, (*parent).right) {
                        node = parent;
                        (*node).rotate_left(sentinel);
                    }

                    // Re-read parent and grandparent after potential rotation
                    let parent = (*node).parent;
                    let grandparent = (*node).grandparent().unwrap();

                    (*parent).is_red = false;
                    (*grandparent).is_red = true;
                    (*grandparent).rotate_right(sentinel);
                }
            } else {
                // Mirror cases for right side
                let uncle = (*gp).left;

                if (*uncle).is_red {
                    (*parent).is_red = false;
                    (*uncle).is_red = false;
                    (*gp).is_red = true;
                    node = gp;
                } else {
                    if std::ptr::eq(node, (*parent).left) {
                        node = parent;
                        (*node).rotate_right(sentinel);
                    }

                    // Re-read after potential rotation
                    let parent = (*node).parent;
                    let grandparent = (*node).grandparent().unwrap();

                    (*parent).is_red = false;
                    (*grandparent).is_red = true;
                    (*grandparent).rotate_left(sentinel);
                }
            }
        }

        // Ensure root is black
        (*(*sentinel).parent).is_red = false;
    }

    unsafe fn update_sentinel_min_max(&mut self) {
        let sentinel = self.sentinel;
        let root = (*sentinel).parent;

        if (*root).is_sentinel {
            // Empty tree
            (*sentinel).left = sentinel;
            (*sentinel).right = sentinel;
            return;
        }

        // Find leftmost (minimum)
        let mut min = root;
        while !(*(*min).left).is_sentinel {
            min = (*min).left;
        }
        (*sentinel).left = min;

        // Find rightmost (maximum)
        let mut max = root;
        while !(*(*max).right).is_sentinel {
            max = (*max).right;
        }
        (*sentinel).right = max;
    }
}

impl<T, U> Tll<T, U> {
    unsafe fn new(key: T, value: U, sentinel: *mut Tll<T, U>) -> *mut Tll<T, U> {
        let node_ptr = libc::malloc(size_of::<Tll<T, U>>()) as *mut Tll<T, U>;

        let node = Tll {
            left: sentinel,
            parent: sentinel,
            right: sentinel,
            is_red: true,
            is_sentinel: false,
            _padding: [0; 6],
            key,
            data: value,
        };

        std::ptr::write(node_ptr, node);
        node_ptr
    }

    unsafe fn rotate_left(&mut self, sentinel: *mut Tll<T, U>) -> *mut Tll<T, U> {
        let x = self.right;
        let y = (*x).left;
        let parent = self.parent;

        // Perform rotation
        (*x).left = self;
        self.right = y;

        // Update parents
        (*x).parent = parent;
        self.parent = x;

        if !(*y).is_sentinel {
            (*y).parent = self;
        }

        if (*parent).is_sentinel {
            // self was the root, update sentinel
            (*sentinel).parent = x;
        } else if std::ptr::eq((*parent).left, self) {
            // self was a left child
            (*parent).left = x;
        } else {
            // self was a right child
            (*parent).right = x;
        }

        x
    }

    unsafe fn rotate_right(&mut self, sentinel: *mut Tll<T, U>) -> *mut Tll<T, U> {
        let x = self.left;
        let y = (*x).right;
        let parent = self.parent;

        // Perform rotation
        (*x).right = self;
        self.left = y;

        // Update parents
        (*x).parent = parent;
        self.parent = x;

        if !(*y).is_sentinel {
            (*y).parent = self;
        }

        if (*parent).is_sentinel {
            // self was the root, update sentinel
            (*sentinel).parent = x;
        } else if std::ptr::eq((*parent).left, self) {
            // self was a left child
            (*parent).left = x;
        } else {
            // self was a right child
            (*parent).right = x;
        }

        x
    }

    unsafe fn grandparent(&self) -> Option<*mut Tll<T, U>> {
        if (*self.parent).is_sentinel || (*(*self.parent).parent).is_sentinel {
            None
        } else {
            Some((*self.parent).parent)
        }
    }

    unsafe fn uncle(&self) -> Option<*mut Tll<T, U>> {
        let gp = self.grandparent()?;

        if std::ptr::eq((*gp).left, self.parent) {
            Some((*gp).right)
        } else {
            Some((*gp).left)
        }
    }

    unsafe fn sibling(&self) -> Option<*mut Tll<T, U>> {
        if (*self.parent).is_sentinel {
            None
        } else if std::ptr::eq((*self.parent).left, self as *const _ as *mut _) {
            Some((*self.parent).right)
        } else {
            Some((*self.parent).left)
        }
    }
}

impl<T, U> Drop for TllContainer<T, U> {
    fn drop(&mut self) {
        unsafe {
            if !self.sentinel.is_null() {
                std::ptr::drop_in_place(self.sentinel);
                libc::free(self.sentinel as *mut libc::c_void);
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create a test node
    unsafe fn create_node(
        key: i32,
        data: &str,
        sentinel: *mut Tll<i32, String>,
    ) -> *mut Tll<i32, String> {
        let node_ptr = libc::malloc(size_of::<Tll<i32, String>>()) as *mut Tll<i32, String>;
        let node = Tll {
            left: sentinel,
            parent: sentinel,
            right: sentinel,
            is_red: true,
            is_sentinel: false,
            _padding: [0; 6],
            key,
            data: data.to_string(),
        };
        std::ptr::write(node_ptr, node);
        node_ptr
    }

    #[test]
    fn test_insert_multiple_nodes() {
        let mut container = TllContainer::<i32, String>::new();

        container.insert(50, "fifty".to_string());
        container.insert(30, "thirty".to_string());
        container.insert(70, "seventy".to_string());
        container.insert(20, "twenty".to_string());
        container.insert(40, "forty".to_string());

        assert_eq!(container.len(), 5);

        // Verify BST property by checking in-order traversal
        let items = container.get_items();
        let values: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            values,
            vec!["twenty", "thirty", "forty", "fifty", "seventy"]
        );
    }

    #[test]
    fn test_insert_duplicate_key() {
        let mut container = TllContainer::<i32, String>::new();

        container.insert(50, "fifty".to_string());
        container.insert(50, "FIFTY_UPDATED".to_string());

        assert_eq!(container.len(), 1);
        let items = container.get_items();
        assert_eq!(items[0].as_str(), "FIFTY_UPDATED");
    }

    #[test]
    fn test_rotate_left_non_root() {
        unsafe {
            let container = TllContainer::<i32, String>::new();
            let sentinel = container.sentinel;

            // Build tree:
            //       50 (root)
            //      /
            //     30
            //    /  \
            //   20   40
            //       /
            //      35

            let root = create_node(50, "fifty", sentinel);
            let n30 = create_node(30, "thirty", sentinel);
            let n20 = create_node(20, "twenty", sentinel);
            let n40 = create_node(40, "forty", sentinel);
            let n35 = create_node(35, "thirty-five", sentinel);

            // Setup tree structure
            (*sentinel).parent = root;
            (*root).left = n30;
            (*n30).parent = root;
            (*n30).left = n20;
            (*n30).right = n40;
            (*n20).parent = n30;
            (*n40).parent = n30;
            (*n40).left = n35;
            (*n35).parent = n40;

            // Rotate left on 30
            // Expected result:
            //       50 (root)
            //      /
            //     40
            //    /  \
            //   30   (sentinel)
            //  /  \
            // 20   35

            let new_subtree_root = (*n30).rotate_left(sentinel);

            // Verify the rotation
            assert_eq!(new_subtree_root, n40, "40 should be new subtree root");
            assert_eq!((*n40).parent, root, "40's parent should be 50");
            assert_eq!((*root).left, n40, "50's left child should be 40");
            assert_eq!((*n40).left, n30, "40's left child should be 30");
            assert_eq!((*n30).parent, n40, "30's parent should be 40");
            assert_eq!((*n30).right, n35, "30's right child should be 35");
            assert_eq!((*n35).parent, n30, "35's parent should be 30");
            assert_eq!((*n30).left, n20, "30's left child should still be 20");
            assert_eq!((*n20).parent, n30, "20's parent should still be 30");

            // Sentinel root should NOT have changed
            assert_eq!((*sentinel).parent, root, "Root should still be 50");

            // Cleanup
            libc::free(root as *mut libc::c_void);
            libc::free(n30 as *mut libc::c_void);
            libc::free(n20 as *mut libc::c_void);
            libc::free(n40 as *mut libc::c_void);
            libc::free(n35 as *mut libc::c_void);
            std::mem::forget(container);
        }
    }

    #[test]
    fn test_rotate_left_root() {
        unsafe {
            let container = TllContainer::<i32, String>::new();
            let sentinel = container.sentinel;

            // Build tree:
            //     30 (root)
            //    /  \
            //   20   40
            //       /  \
            //      35   50

            let root = create_node(30, "thirty", sentinel);
            let n20 = create_node(20, "twenty", sentinel);
            let n40 = create_node(40, "forty", sentinel);
            let n35 = create_node(35, "thirty-five", sentinel);
            let n50 = create_node(50, "fifty", sentinel);

            (*sentinel).parent = root;
            (*root).left = n20;
            (*root).right = n40;
            (*n20).parent = root;
            (*n40).parent = root;
            (*n40).left = n35;
            (*n40).right = n50;
            (*n35).parent = n40;
            (*n50).parent = n40;

            // Rotate left on root
            // Expected:
            //     40 (new root)
            //    /  \
            //   30   50
            //  /  \
            // 20  35

            let new_root = (*root).rotate_left(sentinel);

            assert_eq!(new_root, n40, "40 should be new root");
            assert_eq!((*sentinel).parent, n40, "Sentinel should point to new root");
            assert_eq!(
                (*n40).parent,
                sentinel,
                "New root's parent should be sentinel"
            );
            assert_eq!((*n40).left, root, "40's left should be old root (30)");
            assert_eq!((*root).parent, n40, "30's parent should be 40");
            assert_eq!((*root).right, n35, "30's right should be 35");
            assert_eq!((*n35).parent, root, "35's parent should be 30");

            // Cleanup
            libc::free(root as *mut libc::c_void);
            libc::free(n20 as *mut libc::c_void);
            libc::free(n40 as *mut libc::c_void);
            libc::free(n35 as *mut libc::c_void);
            libc::free(n50 as *mut libc::c_void);
            std::mem::forget(container);
        }
    }

    #[test]
    fn test_rotate_right_non_root() {
        unsafe {
            let container = TllContainer::<i32, String>::new();
            let sentinel = container.sentinel;

            // Build tree:
            //       50 (root)
            //      /
            //     40
            //    /  \
            //   30   (sentinel)
            //  /  \
            // 20   35

            let root = create_node(50, "fifty", sentinel);
            let n40 = create_node(40, "forty", sentinel);
            let n30 = create_node(30, "thirty", sentinel);
            let n20 = create_node(20, "twenty", sentinel);
            let n35 = create_node(35, "thirty-five", sentinel);

            (*sentinel).parent = root;
            (*root).left = n40;
            (*n40).parent = root;
            (*n40).left = n30;
            (*n30).parent = n40;
            (*n30).left = n20;
            (*n30).right = n35;
            (*n20).parent = n30;
            (*n35).parent = n30;

            // Rotate right on 40
            // Expected:
            //       50 (root)
            //      /
            //     30
            //    /  \
            //   20   40
            //       /
            //      35

            let new_subtree_root = (*n40).rotate_right(sentinel);

            assert_eq!(new_subtree_root, n30, "30 should be new subtree root");
            assert_eq!((*n30).parent, root, "30's parent should be 50");
            assert_eq!((*root).left, n30, "50's left child should be 30");
            assert_eq!((*n30).right, n40, "30's right child should be 40");
            assert_eq!((*n40).parent, n30, "40's parent should be 30");
            assert_eq!((*n40).left, n35, "40's left child should be 35");
            assert_eq!((*n35).parent, n40, "35's parent should be 40");

            // Cleanup
            libc::free(root as *mut libc::c_void);
            libc::free(n40 as *mut libc::c_void);
            libc::free(n30 as *mut libc::c_void);
            libc::free(n20 as *mut libc::c_void);
            libc::free(n35 as *mut libc::c_void);
            std::mem::forget(container);
        }
    }

    #[test]
    fn test_rotate_right_root() {
        unsafe {
            let container = TllContainer::<i32, String>::new();
            let sentinel = container.sentinel;

            // Build tree:
            //       40 (root)
            //      /  \
            //     30   50
            //    /  \
            //   20   35

            let root = create_node(40, "forty", sentinel);
            let n30 = create_node(30, "thirty", sentinel);
            let n50 = create_node(50, "fifty", sentinel);
            let n20 = create_node(20, "twenty", sentinel);
            let n35 = create_node(35, "thirty-five", sentinel);

            (*sentinel).parent = root;
            (*root).left = n30;
            (*root).right = n50;
            (*n30).parent = root;
            (*n50).parent = root;
            (*n30).left = n20;
            (*n30).right = n35;
            (*n20).parent = n30;
            (*n35).parent = n30;

            // Rotate right on root
            // Expected:
            //       30 (new root)
            //      /  \
            //     20   40
            //         /  \
            //        35   50

            let new_root = (*root).rotate_right(sentinel);

            assert_eq!(new_root, n30, "30 should be new root");
            assert_eq!((*sentinel).parent, n30, "Sentinel should point to new root");
            assert_eq!(
                (*n30).parent,
                sentinel,
                "New root's parent should be sentinel"
            );
            assert_eq!((*n30).right, root, "30's right should be old root (40)");
            assert_eq!((*root).parent, n30, "40's parent should be 30");
            assert_eq!((*root).left, n35, "40's left should be 35");
            assert_eq!((*n35).parent, root, "35's parent should be 40");

            // Cleanup
            libc::free(root as *mut libc::c_void);
            libc::free(n30 as *mut libc::c_void);
            libc::free(n50 as *mut libc::c_void);
            libc::free(n20 as *mut libc::c_void);
            libc::free(n35 as *mut libc::c_void);
            std::mem::forget(container);
        }
    }

    #[test]
    fn test_rotate_left_with_sentinel_child() {
        unsafe {
            let container = TllContainer::<i32, String>::new();
            let sentinel = container.sentinel;

            // Build simple tree:
            //     30 (root)
            //    /  \
            //   20   40 (no children)

            let root = create_node(30, "thirty", sentinel);
            let n20 = create_node(20, "twenty", sentinel);
            let n40 = create_node(40, "forty", sentinel);

            (*sentinel).parent = root;
            (*root).left = n20;
            (*root).right = n40;
            (*n20).parent = root;
            (*n40).parent = root;

            // Rotate left on root
            // 40 has no left child (sentinel), so 30's right should become sentinel

            let new_root = (*root).rotate_left(sentinel);

            assert_eq!(new_root, n40);
            assert_eq!((*sentinel).parent, n40, "Sentinel should point to new root");
            assert!(
                (*root).right.is_null() || (*(*root).right).is_sentinel,
                "30's right should be sentinel"
            );

            // Cleanup
            libc::free(root as *mut libc::c_void);
            libc::free(n20 as *mut libc::c_void);
            libc::free(n40 as *mut libc::c_void);
            std::mem::forget(container);
        }
    }

    #[test]
    fn test_rotate_right_with_sentinel_child() {
        unsafe {
            let container = TllContainer::<i32, String>::new();
            let sentinel = container.sentinel;

            // Build simple tree:
            //     40 (root)
            //    /  \
            //   30   50 (no children)

            let root = create_node(40, "forty", sentinel);
            let n30 = create_node(30, "thirty", sentinel);
            let n50 = create_node(50, "fifty", sentinel);

            (*sentinel).parent = root;
            (*root).left = n30;
            (*root).right = n50;
            (*n30).parent = root;
            (*n50).parent = root;

            // Rotate right on root
            // 30 has no right child (sentinel), so 40's left should become sentinel

            let new_root = (*root).rotate_right(sentinel);

            assert_eq!(new_root, n30);
            assert_eq!((*sentinel).parent, n30, "Sentinel should point to new root");
            assert!(
                (*root).left.is_null() || (*(*root).left).is_sentinel,
                "40's left should be sentinel"
            );

            // Cleanup
            libc::free(root as *mut libc::c_void);
            libc::free(n30 as *mut libc::c_void);
            libc::free(n50 as *mut libc::c_void);
            std::mem::forget(container);
        }
    }

    // Helper function to verify red-black tree properties
    unsafe fn verify_rb_properties<T: Ord, U>(node: *const Tll<T, U>) -> (bool, usize) {
        // Property 1: Root must be black
        if (*node).is_sentinel {
            return (true, 1);
        }

        // Property 2: Red nodes must have black children
        if (*node).is_red {
            if !(*(*node).left).is_sentinel && (*(*node).left).is_red {
                return (false, 0);
            }
            if !(*(*node).right).is_sentinel && (*(*node).right).is_red {
                return (false, 0);
            }
        }

        // Property 3: All paths must have same black height
        let (left_valid, left_black_height) = verify_rb_properties((*node).left);
        let (right_valid, right_black_height) = verify_rb_properties((*node).right);

        if !left_valid || !right_valid {
            return (false, 0);
        }

        if left_black_height != right_black_height {
            return (false, 0);
        }

        let black_height = left_black_height + if (*node).is_red { 0 } else { 1 };
        (true, black_height)
    }

    #[test]
    fn test_insert_root_is_black() {
        let mut container = TllContainer::<i32, String>::new();
        container.insert(50, "fifty".to_string());

        unsafe {
            let root = (*container.sentinel).parent;
            assert!(!(*root).is_red, "Root must be black");
            assert_eq!(container.len(), 1);
        }
    }

    #[test]
    fn test_insert_maintains_bst_property() {
        let mut container = TllContainer::<i32, String>::new();

        // Insert in random order
        let values = vec![50, 25, 75, 10, 30, 60, 80, 5, 15, 27, 35];
        for &val in &values {
            container.insert(val, format!("value_{}", val));
        }

        // Verify in-order traversal is sorted
        let mut prev = i32::MIN;

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            let mut result = Vec::new();
            in_order_traverse(root, &mut result);

            for (key, _) in result {
                assert!(
                    *key > prev,
                    "BST property violated: {} should be > {}",
                    key,
                    prev
                );
                prev = *key;
            }
        }
    }

    #[test]
    fn test_insert_maintains_rb_properties() {
        let mut container = TllContainer::<i32, String>::new();

        // Insert values that will trigger various rebalancing cases
        let values = vec![50, 25, 75, 10, 30, 60, 80];
        for &val in &values {
            container.insert(val, format!("value_{}", val));
        }

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            // Verify root is black
            assert!(!(*root).is_red, "Root must be black");

            // Verify all red-black properties
            let (valid, _) = verify_rb_properties(root);
            assert!(valid, "Red-black tree properties violated");
        }
    }

    #[test]
    fn test_insert_case_uncle_red_recoloring() {
        let mut container = TllContainer::<i32, String>::new();

        // This sequence triggers uncle-is-red case (recoloring)
        container.insert(50, "fifty".to_string());
        container.insert(25, "twenty-five".to_string());
        container.insert(75, "seventy-five".to_string());
        container.insert(10, "ten".to_string()); // This triggers recoloring

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            // Root must be black
            assert!(!(*root).is_red, "Root must be black");

            // Verify properties maintained
            let (valid, _) = verify_rb_properties(root);
            assert!(valid, "Red-black tree properties violated after recoloring");
        }

        assert_eq!(container.len(), 4);
    }

    #[test]
    fn test_insert_case_left_left_rotation() {
        let mut container = TllContainer::<i32, String>::new();

        // This sequence triggers left-left case (right rotation on grandparent)
        container.insert(50, "fifty".to_string());
        container.insert(30, "thirty".to_string());
        container.insert(20, "twenty".to_string()); // Triggers left-left case

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            // Verify properties
            let (valid, _) = verify_rb_properties(root);
            assert!(
                valid,
                "Red-black tree properties violated after left-left rotation"
            );

            // Root should be black
            assert!(!(*root).is_red, "Root must be black");
        }

        assert_eq!(container.len(), 3);
    }

    #[test]
    fn test_insert_case_left_right_rotation() {
        let mut container = TllContainer::<i32, String>::new();

        // This sequence triggers left-right case (left rotation on parent, then right on grandparent)
        container.insert(50, "fifty".to_string());
        container.insert(30, "thirty".to_string());
        container.insert(40, "forty".to_string()); // Triggers left-right case

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            // Verify properties
            let (valid, _) = verify_rb_properties(root);
            assert!(
                valid,
                "Red-black tree properties violated after left-right rotation"
            );

            // Root should be black
            assert!(!(*root).is_red, "Root must be black");
        }

        assert_eq!(container.len(), 3);
    }

    #[test]
    fn test_insert_case_right_right_rotation() {
        let mut container = TllContainer::<i32, String>::new();

        // This sequence triggers right-right case (left rotation on grandparent)
        container.insert(50, "fifty".to_string());
        container.insert(70, "seventy".to_string());
        container.insert(80, "eighty".to_string()); // Triggers right-right case

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            // Verify properties
            let (valid, _) = verify_rb_properties(root);
            assert!(
                valid,
                "Red-black tree properties violated after right-right rotation"
            );

            // Root should be black
            assert!(!(*root).is_red, "Root must be black");
        }

        assert_eq!(container.len(), 3);
    }

    #[test]
    fn test_insert_case_right_left_rotation() {
        let mut container = TllContainer::<i32, String>::new();

        // This sequence triggers right-left case (right rotation on parent, then left on grandparent)
        container.insert(50, "fifty".to_string());
        container.insert(70, "seventy".to_string());
        container.insert(60, "sixty".to_string()); // Triggers right-left case

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            // Verify properties
            let (valid, _) = verify_rb_properties(root);
            assert!(
                valid,
                "Red-black tree properties violated after right-left rotation"
            );

            // Root should be black
            assert!(!(*root).is_red, "Root must be black");
        }

        assert_eq!(container.len(), 3);
    }

    #[test]
    fn test_insert_sequential_ascending() {
        let mut container = TllContainer::<i32, String>::new();

        // Insert in ascending order (worst case for unbalanced BST)
        for i in 1..=10 {
            container.insert(i, format!("value_{}", i));
        }

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            // Verify properties are maintained even with sequential insertion
            let (valid, _) = verify_rb_properties(root);
            assert!(
                valid,
                "Red-black tree properties violated after sequential insertion"
            );

            // Root should be black
            assert!(!(*root).is_red, "Root must be black");
        }

        assert_eq!(container.len(), 10);

        // Verify BST property
        let items = container.get_items();
        assert_eq!(items.len(), 10);
    }

    #[test]
    fn test_insert_sequential_descending() {
        let mut container = TllContainer::<i32, String>::new();

        // Insert in descending order
        for i in (1..=10).rev() {
            container.insert(i, format!("value_{}", i));
        }

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            // Verify properties
            let (valid, _) = verify_rb_properties(root);
            assert!(
                valid,
                "Red-black tree properties violated after descending insertion"
            );

            // Root should be black
            assert!(!(*root).is_red, "Root must be black");
        }

        assert_eq!(container.len(), 10);
    }

    #[test]
    fn test_insert_alternating_pattern() {
        let mut container = TllContainer::<i32, String>::new();

        // Insert in alternating pattern to trigger various cases
        let values = vec![50, 30, 70, 20, 40, 60, 80, 10, 25, 35, 45];
        for &val in &values {
            container.insert(val, format!("value_{}", val));

            unsafe {
                let sentinel = container.sentinel;
                let root = (*sentinel).parent;

                // After each insertion, verify properties hold
                let (valid, _) = verify_rb_properties(root);
                assert!(
                    valid,
                    "Red-black tree properties violated after inserting {}",
                    val
                );

                // Root must always be black
                assert!(
                    !(*root).is_red,
                    "Root must be black after inserting {}",
                    val
                );
            }
        }

        assert_eq!(container.len(), values.len());

        // Verify final tree structure
        let items = container.get_items();
        assert_eq!(items.len(), values.len());
    }

    #[test]
    fn test_insert_with_updates() {
        let mut container = TllContainer::<i32, String>::new();

        // Insert initial values
        container.insert(50, "fifty_v1".to_string());
        container.insert(30, "thirty_v1".to_string());
        container.insert(70, "seventy_v1".to_string());

        // Update existing values
        container.insert(50, "fifty_v2".to_string());
        container.insert(30, "thirty_v2".to_string());

        // Size should not change on updates
        assert_eq!(container.len(), 3);

        // Verify updated values
        let map = container.get_map();
        assert_eq!(map.get(&50).unwrap().as_str(), "fifty_v2");
        assert_eq!(map.get(&30).unwrap().as_str(), "thirty_v2");
        assert_eq!(map.get(&70).unwrap().as_str(), "seventy_v1");

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            // Properties should still be valid
            let (valid, _) = verify_rb_properties(root);
            assert!(valid, "Red-black tree properties violated after updates");
        }
    }

    #[test]
    fn test_sentinel_min_max_single_node() {
        let mut container = TllContainer::<i32, String>::new();
        container.insert(50, "fifty".to_string());

        unsafe {
            let sentinel = container.sentinel;
            let min_node = (*sentinel).left;
            let max_node = (*sentinel).right;

            // With one node, min and max should be the same
            assert_eq!(
                min_node, max_node,
                "Min and max should be same for single node"
            );
            assert_eq!((*min_node).key, 50, "Min should be 50");
            assert_eq!((*max_node).key, 50, "Max should be 50");
        }
    }

    #[test]
    fn test_sentinel_min_max_ascending_insertion() {
        let mut container = TllContainer::<i32, String>::new();

        // Insert in ascending order
        for i in 1..=10 {
            container.insert(i, format!("value_{}", i));

            unsafe {
                let sentinel = container.sentinel;
                let min_node = (*sentinel).left;
                let max_node = (*sentinel).right;

                // Min should always be 1
                assert_eq!((*min_node).key, 1, "Min should be 1 after inserting {}", i);

                // Max should be the largest value inserted so far
                assert_eq!(
                    (*max_node).key,
                    i,
                    "Max should be {} after inserting {}",
                    i,
                    i
                );
            }
        }
    }

    #[test]
    fn test_sentinel_min_max_descending_insertion() {
        let mut container = TllContainer::<i32, String>::new();

        // Insert in descending order
        for i in (1..=10).rev() {
            container.insert(i, format!("value_{}", i));

            unsafe {
                let sentinel = container.sentinel;
                let min_node = (*sentinel).left;
                let max_node = (*sentinel).right;

                // Min should be the smallest value inserted so far
                assert_eq!(
                    (*min_node).key,
                    i,
                    "Min should be {} after inserting {}",
                    i,
                    i
                );

                // Max should always be 10
                assert_eq!(
                    (*max_node).key,
                    10,
                    "Max should be 10 after inserting {}",
                    i
                );
            }
        }
    }

    #[test]
    fn test_sentinel_min_max_random_insertion() {
        let mut container = TllContainer::<i32, String>::new();

        // Insert in random order
        let values = vec![50, 25, 75, 10, 30, 60, 80, 5, 15, 70, 90];
        let mut current_min = i32::MAX;
        let mut current_max = i32::MIN;

        for &val in &values {
            container.insert(val, format!("value_{}", val));
            current_min = current_min.min(val);
            current_max = current_max.max(val);

            unsafe {
                let sentinel = container.sentinel;
                let min_node = (*sentinel).left;
                let max_node = (*sentinel).right;

                assert_eq!(
                    (*min_node).key,
                    current_min,
                    "Min should be {} after inserting {}",
                    current_min,
                    val
                );

                assert_eq!(
                    (*max_node).key,
                    current_max,
                    "Max should be {} after inserting {}",
                    current_max,
                    val
                );
            }
        }
    }

    #[test]
    fn test_sentinel_min_max_after_rotations() {
        let mut container = TllContainer::<i32, String>::new();

        // This insertion pattern will trigger rotations
        // but min/max should still be maintained correctly
        container.insert(50, "fifty".to_string());
        container.insert(25, "twenty-five".to_string());
        container.insert(75, "seventy-five".to_string());
        container.insert(10, "ten".to_string());
        container.insert(30, "thirty".to_string());
        container.insert(60, "sixty".to_string());
        container.insert(80, "eighty".to_string());
        container.insert(5, "five".to_string()); // New min
        container.insert(90, "ninety".to_string()); // New max

        unsafe {
            let sentinel = container.sentinel;
            let min_node = (*sentinel).left;
            let max_node = (*sentinel).right;

            assert_eq!((*min_node).key, 5, "Min should be 5");
            assert_eq!((*max_node).key, 90, "Max should be 90");

            // Verify min node has no left child
            assert!(
                (*(*min_node).left).is_sentinel,
                "Min node should have no left child"
            );

            // Verify max node has no right child
            assert!(
                (*(*max_node).right).is_sentinel,
                "Max node should have no right child"
            );
        }
    }

    #[test]
    fn test_sentinel_min_max_with_duplicates() {
        let mut container = TllContainer::<i32, String>::new();

        container.insert(50, "fifty".to_string());
        container.insert(25, "twenty-five".to_string());
        container.insert(75, "seventy-five".to_string());

        unsafe {
            let sentinel = container.sentinel;
            let min_before = (*sentinel).left;
            let max_before = (*sentinel).right;

            // Update existing values (no structural change)
            container.insert(50, "FIFTY_UPDATED".to_string());
            container.insert(25, "TWENTY_FIVE_UPDATED".to_string());

            let min_after = (*sentinel).left;
            let max_after = (*sentinel).right;

            // Min and max pointers should not change
            assert_eq!(
                min_before, min_after,
                "Min pointer should not change on update"
            );
            assert_eq!(
                max_before, max_after,
                "Max pointer should not change on update"
            );

            assert_eq!((*min_after).key, 25, "Min should still be 25");
            assert_eq!((*max_after).key, 75, "Max should still be 75");
        }
    }

    #[test]
    fn test_sentinel_min_max_comprehensive() {
        let mut container = TllContainer::<i32, String>::new();

        // Build a tree and verify min/max at each step
        let insertions = vec![
            (50, 50, 50), // (value, expected_min, expected_max)
            (30, 30, 50),
            (70, 30, 70),
            (20, 20, 70),
            (40, 20, 70),
            (60, 20, 70),
            (80, 20, 80),
            (10, 10, 80),
            (90, 10, 90),
        ];

        for (value, expected_min, expected_max) in insertions {
            container.insert(value, format!("value_{}", value));

            unsafe {
                let sentinel = container.sentinel;
                let min_node = (*sentinel).left;
                let max_node = (*sentinel).right;

                assert_eq!(
                    (*min_node).key,
                    expected_min,
                    "After inserting {}, min should be {}",
                    value,
                    expected_min
                );

                assert_eq!(
                    (*max_node).key,
                    expected_max,
                    "After inserting {}, max should be {}",
                    value,
                    expected_max
                );

                // Verify min has no left child
                assert!(
                    (*(*min_node).left).is_sentinel,
                    "Min node should have no left child after inserting {}",
                    value
                );

                // Verify max has no right child
                assert!(
                    (*(*max_node).right).is_sentinel,
                    "Max node should have no right child after inserting {}",
                    value
                );
            }
        }
    }

    #[test]
    fn test_game_loadout_tree_structure() {
        // Recreate the exact tree structure from the game memory dump
        // This tests inserting the 10 loadouts in an order that matches the game
        let mut container = TllContainer::<String, String>::new();

        // Insert loadouts in the order they appear in the game
        let loadouts = vec![
            "LOADOUT_LA29_FAB100",
            "LOADOUT_LA29_FAB250",
            "LOADOUT_LA29_GUN37",
            "LOADOUT_LA29_NURS122",
            "LOADOUT_T7_FAB100",
            "LOADOUT_T7_FAB250",
            "LOADOUT_T7_GUN37",
            "LOADOUT_T7_K13",
            "LOADOUT_T7_NURS122",
            "LOADOUT_T7_NURS340",
        ];

        for loadout in &loadouts {
            container.insert(loadout.to_string(), format!("data_{}", loadout));
        }

        assert_eq!(container.len(), 10);

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            // Verify root is black (unlike the game dump which shows red)
            assert!(!(*root).is_red, "Root must be black");

            // Verify red-black properties
            let (valid, _) = verify_rb_properties(root);
            assert!(valid, "Tree must maintain red-black properties");

            // Verify BST ordering
            let items = container.get_items();
            assert_eq!(items.len(), 10);
        }
    }

    #[test]
    fn test_insert_loadout_la29_gun40() {
        // Test inserting LOADOUT_LA29_GUN40 into a tree with the game's loadouts
        // This is the specific case that was causing the panic
        let mut container = TllContainer::<String, String>::new();

        // Build the game tree
        let loadouts = vec![
            "LOADOUT_LA29_FAB100",
            "LOADOUT_LA29_FAB250",
            "LOADOUT_LA29_GUN37",
            "LOADOUT_LA29_NURS122",
            "LOADOUT_T7_FAB100",
            "LOADOUT_T7_FAB250",
            "LOADOUT_T7_GUN37",
            "LOADOUT_T7_K13",
            "LOADOUT_T7_NURS122",
            "LOADOUT_T7_NURS340",
        ];

        for loadout in &loadouts {
            container.insert(loadout.to_string(), format!("data_{}", loadout));
        }

        // Now insert the problematic loadout
        container.insert(
            "LOADOUT_LA29_GUN40".to_string(),
            "custom_loadout".to_string(),
        );

        assert_eq!(
            container.len(),
            11,
            "Should have 11 loadouts after insertion"
        );

        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;

            // Verify root is still black
            assert!(!(*root).is_red, "Root must remain black after insertion");

            // Verify red-black properties are maintained
            let (valid, _) = verify_rb_properties(root);
            assert!(
                valid,
                "Red-black properties must be maintained after inserting LOADOUT_LA29_GUN40"
            );

            // Verify the new loadout is in the tree
            let map = container.get_map();
            assert!(
                map.contains_key(&"LOADOUT_LA29_GUN40".to_string()),
                "New loadout should be in the tree"
            );

            // Verify BST ordering is maintained
            let mut result = Vec::new();
            in_order_traverse(root, &mut result);
            let keys: Vec<String> = result.iter().map(|(k, _)| (*k).clone()).collect();

            // Check that keys are in sorted order
            let mut sorted_keys = keys.clone();
            sorted_keys.sort();
            assert_eq!(keys, sorted_keys, "Keys should be in sorted order");

            // Verify LOADOUT_LA29_GUN40 is between GUN37 and NURS122
            let gun40_pos = keys.iter().position(|k| k == "LOADOUT_LA29_GUN40").unwrap();
            let gun37_pos = keys.iter().position(|k| k == "LOADOUT_LA29_GUN37").unwrap();
            let nurs122_pos = keys
                .iter()
                .position(|k| k == "LOADOUT_LA29_NURS122")
                .unwrap();

            assert!(
                gun37_pos < gun40_pos && gun40_pos < nurs122_pos,
                "GUN40 should be between GUN37 and NURS122"
            );
        }
    }

    #[test]
    fn test_insert_into_tree_with_red_root_defensive() {
        // Test defensive handling of a tree with a red root (like the game dump showed)
        // This simulates the corrupted state we might encounter from C++
        unsafe {
            let container = TllContainer::<i32, String>::new();
            let sentinel = container.sentinel;

            // Manually create a tree with a red root (simulating C++ state)
            let root = create_node(50, "fifty", sentinel);
            (*root).is_red = true; // VIOLATION: Make root red like in game dump

            (*sentinel).parent = root;
            (*sentinel).left = root;
            (*sentinel).right = root;

            let left = create_node(30, "thirty", sentinel);
            (*left).parent = root;
            (*root).left = left;

            let right = create_node(70, "seventy", sentinel);
            (*right).parent = root;
            (*root).right = right;

            // Now try to work with this tree
            let mut container_mut = container;

            // Insert a new node - this should handle the red root gracefully
            container_mut.insert(40, "forty".to_string());

            // After insert, root should be black (our fixup forces it)
            let root_after = (*sentinel).parent;
            assert!(
                !(*root_after).is_red,
                "Root should be forced to black after insert fixup"
            );

            // Tree should still be valid
            let (valid, _) = verify_rb_properties(root_after);
            assert!(valid, "Tree should be valid after fixing red root");

            // Cleanup
            libc::free(root as *mut libc::c_void);
            libc::free(left as *mut libc::c_void);
            libc::free(right as *mut libc::c_void);
            std::mem::forget(container_mut);
        }
    }

    #[test]
    fn test_all_game_loadouts_sorted() {
        // Verify that all game loadouts maintain sorted order
        let mut container = TllContainer::<String, String>::new();

        let loadouts = vec![
            "LOADOUT_LA29_FAB100",
            "LOADOUT_LA29_FAB250",
            "LOADOUT_LA29_GUN37",
            "LOADOUT_LA29_NURS122",
            "LOADOUT_T7_FAB100",
            "LOADOUT_T7_FAB250",
            "LOADOUT_T7_GUN37",
            "LOADOUT_T7_K13",
            "LOADOUT_T7_NURS122",
            "LOADOUT_T7_NURS340",
        ];

        for loadout in &loadouts {
            container.insert(loadout.to_string(), format!("data_{}", loadout));
        }

        // Get all items in order
        unsafe {
            let sentinel = container.sentinel;
            let root = (*sentinel).parent;
            let mut result = Vec::new();
            in_order_traverse(root, &mut result);

            let keys: Vec<&str> = result.iter().map(|(k, _)| k.as_str()).collect();

            // Expected order (alphabetically sorted)
            let expected = vec![
                "LOADOUT_LA29_FAB100",
                "LOADOUT_LA29_FAB250",
                "LOADOUT_LA29_GUN37",
                "LOADOUT_LA29_NURS122",
                "LOADOUT_T7_FAB100",
                "LOADOUT_T7_FAB250",
                "LOADOUT_T7_GUN37",
                "LOADOUT_T7_K13",
                "LOADOUT_T7_NURS122",
                "LOADOUT_T7_NURS340",
            ];

            assert_eq!(keys, expected, "Loadouts should be in alphabetical order");
        }
    }
}
