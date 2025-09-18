//! Simple mod that patches highfleet to prevent gui shacking

#![deny(missing_docs)]

use std::ffi::{c_char, CStr};

use crate::config::Config;

mod patchy;
mod dumpable;
// mod logger;
mod config;
mod zoom;
mod guns;

#[no_mangle]
unsafe extern "C" fn init() -> bool {
    let config = Config::load("Modloader/config/qol.json");
    let config = match config {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to load config: {}", e);
            log::error!("Using default config");
            let conf = Config::default();

            // Save the default config
            if let Err(e) = conf.save("Modloader/config/qol.json") {
                log::error!("Failed to save default config: {}", e);
            } else {
                log::info!("Default config saved to Modloader/config/qol.json");
            }

            conf
        }
    };

    if config.enable_anti_wobble {
        dumpable::dumpable();
        log::info!("Anti-wobble enabled");
    } else {
        log::info!("Anti-wobble disabled");
    }

    if config.enable_arcade_zoom {
        zoom::patch_zoom(config.min_zoom_level as u32, config.max_zoom_level as u32);
        log::info!("Arcade zoom enabled (min zoom level {}, max zoom level {})", config.min_zoom_level, config.max_zoom_level);

        if config.zoom_levels.len() < 5 {
            log::warn!("The game by default specifies 5 zoom levels. If you specify less, the game may be unstable.");
        }

        if config.zoom_levels.len() < config.max_zoom_level as usize {
            log::warn!("You have specified more max zoom levels than you have zoom levels. This may cause instability.");
        }

        zoom::patch_levels(config.zoom_levels);
    } else {
        log::info!("Arcade zoom disabled");
    }

    if config.enable_unblocked_guns {
        guns::patch_sector_blocking();
        log::info!("Unblocked guns enabled");
    } else {
        log::info!("Unblocked guns disabled");
    }

    true
}

#[no_mangle]
unsafe extern "C" fn version(version: *const c_char) -> bool {
    let version = CStr::from_ptr(version).to_str().unwrap();
    if cfg!(feature = "1_151") {
        version == "Steam 1.151"
    } else if cfg!(feature = "1_163") {
        version == "Steam 1.163"
    } else if version == "Gog 1.163" {
        log::error!("Gog 1.163 detected");
        log::error!("Your game will crash. Ammo Extended only supports steam versions of the game.");
        false
    } else {
        false
    }
}
