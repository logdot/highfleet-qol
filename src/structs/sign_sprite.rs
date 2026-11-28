//! Sign-sprite fields read by the battle aircraft panel.

/// The blend state and frame extents of a loaded sign animation.
///
/// Only the fields the panel touches are modelled; the rest of the record is uninterpreted.
#[repr(C)]
pub struct SignSprite {
    _unknown_000: [u8; 0x168],
    /// Blend state, cleared after each sign load exactly as the game does.
    pub blend_state: i32,
    _unknown_16c: [u8; 0x1c],
    /// Width of the loaded frame, in pixels.
    pub extent_x: f32,
    /// Height of the loaded frame, in pixels.
    pub extent_y: f32,
}

const _: () = {
    assert!(std::mem::offset_of!(SignSprite, blend_state) == 0x168);
    assert!(std::mem::offset_of!(SignSprite, extent_x) == 0x188);
    assert!(std::mem::offset_of!(SignSprite, extent_y) == 0x18c);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_layout() {
        assert_eq!(std::mem::offset_of!(SignSprite, blend_state), 0x168);
        assert_eq!(std::mem::offset_of!(SignSprite, extent_x), 0x188);
        assert_eq!(std::mem::offset_of!(SignSprite, extent_y), 0x18c);
        assert_eq!(std::mem::size_of::<SignSprite>(), 0x190);
    }
}
