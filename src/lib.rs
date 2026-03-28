//! Simple mod that patches highfleet to prevent gui shacking

#![deny(missing_docs)]

use std::ffi::{c_char, CStr};

use crate::config::Config;

mod config;
mod dumpable;
mod guns;
#[cfg(debug_assertions)]
mod logger;
mod parts;
mod patchy;
mod plane;
mod rng;
mod sell_multiplier;
mod shake;
mod structs;
mod ttl;
mod zoom;

#[no_mangle]
unsafe extern "C" fn init() -> bool {
    let config = Config::load("Modloader/config/qol.json");
    let config = match config {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to load config: {}", e);
            log::error!("Using default config");
            let conf = Config::default();

            // Check if default config exists
            if std::path::Path::new("Modloader/config/qol.json").exists() {
                log::error!(
                    "Config file exists but failed to load. Please check the file for errors."
                );
            } else {
                // Save the default config
                if let Err(e) = conf.save("Modloader/config/qol.json") {
                    log::error!("Failed to save default config: {}", e);
                } else {
                    log::info!("Default config saved to Modloader/config/qol.json");
                }
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
        log::info!(
            "Arcade zoom enabled (min zoom level {}, max zoom level {})",
            config.min_zoom_level,
            config.max_zoom_level
        );

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
        guns::patch_sector_restoration();
        log::info!("Gun blocking enabled");
    }

    if config.enable_reduced_shake {
        shake::patch_shake();
        log::info!("Reduced shake enabled");
    } else {
        log::info!("Reduced shake disabled");
    }

    if config.enable_unblocked_ttl {
        ttl::patch_ttl();
        log::info!("Unblocked TTL enabled");
    } else {
        log::info!("Unblocked TTL disabled");
    }

    plane::patch_planes(&config.planes);

    if config.enable_shop_parts {
        parts::patch_custom_parts(config.shop_parts);
        log::info!("Custom parts enabled");
    } else {
        log::info!("Custom parts disabled");
    }

    sell_multiplier::patch_sell_multiplier(config.sell_multiplier);

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
        log::error!("Your game will crash. QOL only supports steam versions of the game.");
        false
    } else {
        false
    }
}
