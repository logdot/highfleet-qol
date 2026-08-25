//! Simple mod that patches highfleet to prevent gui shacking

#![deny(missing_docs)]

use std::ffi::{c_char, CStr};

use crate::config::Config;

pub mod config;
mod dumpable;
mod flare_crash;
mod guns;
#[cfg(debug_assertions)]
mod logger;
mod parts;
mod plane;
mod plane_guns;
mod plane_health;
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

    let mut patch_session = patchy::PatchSession::new();

    macro_rules! prepare_patch {
        ($operation:expr) => {
            if let Err(error) = $operation {
                log::error!("Unable to prepare patches: {error}");
                return false;
            }
        };
    }

    if config.enable_flare_crash_fix {
        prepare_patch!(flare_crash::patch_flare_crash(&mut patch_session));
    } else {
        log::info!("Flare crash fix disabled");
    }

    if config.enable_anti_wobble {
        prepare_patch!(dumpable::dumpable(&mut patch_session));
        log::info!("Anti-wobble enabled");
    } else {
        log::info!("Anti-wobble disabled");
    }

    if config.enable_arcade_zoom {
        prepare_patch!(zoom::patch_zoom(
            &mut patch_session,
            config.min_zoom_level as u32,
            config.max_zoom_level as u32
        ));
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

        prepare_patch!(zoom::patch_levels(&mut patch_session, config.zoom_levels));
    } else {
        log::info!("Arcade zoom disabled");
    }

    if config.enable_unblocked_guns {
        prepare_patch!(guns::patch_sector_blocking(&mut patch_session));
        log::info!("Unblocked guns enabled");
    } else {
        prepare_patch!(guns::patch_sector_restoration(&mut patch_session));
        log::info!("Gun blocking enabled");
    }

    if config.enable_reduced_shake {
        prepare_patch!(shake::patch_shake(&mut patch_session));
        log::info!("Reduced shake enabled");
    } else {
        log::info!("Reduced shake disabled");
    }

    if config.enable_unblocked_ttl {
        prepare_patch!(ttl::patch_ttl(&mut patch_session));
        log::info!("Unblocked TTL enabled");
    } else {
        log::info!("Unblocked TTL disabled");
    }

    plane::patch_planes(&config.planes);
    prepare_patch!(plane_guns::patch_plane_guns(&mut patch_session));
    prepare_patch!(plane_health::patch_plane_health(&mut patch_session));

    if config.enable_shop_parts {
        prepare_patch!(parts::patch_custom_parts(
            &mut patch_session,
            config.shop_parts
        ));
        log::info!("Custom parts enabled");
    } else {
        log::info!("Custom parts disabled");
    }

    prepare_patch!(sell_multiplier::patch_sell_multiplier(
        &mut patch_session,
        config.sell_multiplier
    ));

    if let Err(error) = patch_session.install_permanently() {
        log::error!("Unable to install prepared patches: {error}");
        return false;
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
        log::error!("Your game will crash. QOL only supports steam versions of the game.");
        false
    } else {
        false
    }
}
