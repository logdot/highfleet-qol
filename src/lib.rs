//! Simple mod that patches highfleet to prevent gui shacking

#![deny(missing_docs)]

highfleet_mod_api::export_logger!();
patchy::export_patchy!();

use std::ffi::{c_char, CStr};

use crate::config::Config;

#[cfg(all(feature = "1_151", feature = "1_163"))]
compile_error!("select at most one of the `1_151` and `1_163` features");

mod antitilt;
pub mod config;
mod dumpable;
#[cfg(not(feature = "1_151"))]
mod elevation_rules;
#[cfg(not(feature = "1_151"))]
mod enemy_laser_warning;
mod flare_crash;
mod guns;
#[cfg(not(feature = "1_151"))]
mod intro;
mod parts;
mod plane;
mod plane_guns;
mod plane_health;
#[cfg(not(feature = "1_151"))]
mod refuel;
mod replay_call;
mod rng;
mod sell_multiplier;
mod shake;
#[cfg(not(feature = "1_151"))]
mod speed;
mod structs;
mod ttl;
mod zoom;

const TARGET_BUILD: &str = if cfg!(feature = "1_151") {
    "Steam 1.151"
} else {
    "Steam 1.163"
};

// QoL uses absolute game addresses, regardless of the version reported by the loader.
const IMAGE_BASE: usize = 0x140000000;

#[no_mangle]
unsafe extern "C" fn init() -> bool {
    match patchy::ProcessModule::main() {
        Ok(module) if module.base_address() == IMAGE_BASE => {}
        Ok(module) => {
            log::error!(
                "Refusing to load QoL: the game is mapped at {:#x} instead of {IMAGE_BASE:#x}",
                module.base_address()
            );
            return false;
        }
        Err(error) => {
            log::error!("Refusing to load QoL: unable to locate the game executable: {error}");
            return false;
        }
    }

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

    let mut patch_session = match patchy::PatchSession::new() {
        Ok(session) => session,
        Err(error) => {
            log::error!("Unable to open a Modloader patch session: {error}");
            return false;
        }
    };

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

    if config.enable_antitilt_fps_fix {
        prepare_patch!(antitilt::patch_antitilt(&mut patch_session));
    } else {
        log::info!("Anti-tilt frame-time scaling disabled");
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

    #[cfg(not(feature = "1_151"))]
    {
        if config.enable_intro {
            prepare_patch!(intro::prepare(&mut patch_session));
        }
        if !config.enemy_laser_warning {
            prepare_patch!(enemy_laser_warning::prepare(&mut patch_session));
        }
        if !config.enemy_sound_warning {
            prepare_patch!(enemy_laser_warning::prepare_sound_suppression(
                &mut patch_session
            ));
        }
        prepare_patch!(refuel::patch_refuel(&mut patch_session, &config.refuel));
        prepare_patch!(speed::patch_speed(&mut patch_session, &config.speed));
        prepare_patch!(elevation_rules::patch_elevation_rules(
            &mut patch_session,
            config.elevation_rules
        ));
    }

    if let Err(error) = patch_session.install_permanently() {
        log::error!("Unable to install prepared patches: {error}");
        return false;
    }

    true
}

#[no_mangle]
unsafe extern "C" fn version(version: *const c_char) -> bool {
    let reported = (!version.is_null())
        .then(|| CStr::from_ptr(version).to_str().ok())
        .flatten()
        .unwrap_or("unreported");
    let supported = reported == TARGET_BUILD;
    if !supported {
        log::warn!("QoL targets {TARGET_BUILD}, but Modloader reported {reported}; continuing with the compiled patch addresses");
    }
    // This reports compatibility to the loader; `init` does not depend on it.
    supported
}

#[cfg(test)]
mod version_tests {
    use super::version;

    #[test]
    fn reports_compatibility_for_the_compiled_steam_build() {
        unsafe {
            assert_eq!(version(c"Steam 1.151".as_ptr()), cfg!(feature = "1_151"));
            assert_eq!(version(c"Steam 1.163".as_ptr()), !cfg!(feature = "1_151"));
            assert!(!version(c"Gog 1.163".as_ptr()));
            assert!(!version(c"Unknown".as_ptr()));
            assert!(!version(std::ptr::null()));
        }
    }
}
