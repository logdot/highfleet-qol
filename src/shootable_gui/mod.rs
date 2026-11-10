//! Direct collision and hit points on native Steam 1.163 GUI objects.

mod damage;
mod geometry;
mod hooks;
mod native;

pub(crate) use hooks::patch_shootable_gui;
