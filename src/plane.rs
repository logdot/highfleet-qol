use std::{collections::HashMap, hash::Hash};

use highfleet::general::EscadraString;
use serde::Serialize;

use crate::{
    config::ConfigPlane,
    structs::{cvec::CVec, loadout, plane::Plane, tll::TllContainer},
};

fn get_plane_tll_addr() -> u64 {
    if cfg!(feature = "1_151") {
        0x143942740
    } else if cfg!(feature = "1_163") {
        0x143a13c50
    } else {
        // Default to 1.163
        0x143a13c50
    }
}

fn get_loadout_tll_addr() -> u64 {
    if cfg!(feature = "1_151") {
        0x143944528
    } else if cfg!(feature = "1_163") {
        0x143a15a60
    } else {
        // Default to 1.163
        0x143a15a60
    }
}

pub fn get_planes() -> HashMap<EscadraString, ConfigPlane> {
    let loadout_tll_addr = get_plane_tll_addr();
    let tll_container_ptr = loadout_tll_addr as *const TllContainer<EscadraString, Plane>
        as *mut TllContainer<EscadraString, Plane>;

    unsafe {
        let tll_container = &mut *tll_container_ptr;

        tll_container
            .get_map()
            .into_iter()
            .map(|(k, v)| {
                let loadouts = v
                    .loadouts
                    .items()
                    .into_iter()
                    .map(|&ptr| loadout::ConfigLoadout::from(&*ptr))
                    .collect::<Vec<_>>();
                (k.clone(), ConfigPlane::from(loadouts))
            })
            .collect()
    }
}

pub unsafe fn patch_planes(planes: &HashMap<EscadraString, ConfigPlane>) {
    crate::plane_health::install_plane_health(
        planes
            .iter()
            .map(|(plane_name, plane)| (plane_name.get_string().to_owned(), plane.health))
            .collect(),
    );

    // Load all loadouts from config and keep the custom gun selection outside the game ABI.
    let mut new_loadouts = TllContainer::<EscadraString, loadout::GameLoadout>::new();
    let mut gun_ammo_by_oid = HashMap::<String, Option<String>>::new();

    for plane in planes.values() {
        for loadout in &plane.loadouts {
            let oid = loadout.oid.get_string().to_owned();
            let gun_ammo = loadout
                .gun_ammo
                .as_ref()
                .map(|ammo| ammo.get_string().to_owned());

            if let Some(previous) = gun_ammo_by_oid.insert(oid.clone(), gun_ammo.clone()) {
                if previous != gun_ammo {
                    log::warn!(
                        "Loadout '{oid}' has conflicting gun_ammo definitions; the last definition will be used"
                    );
                }
            }

            new_loadouts.insert(loadout.oid.clone(), loadout::GameLoadout::from(loadout));
        }
    }

    crate::plane_guns::install_loadout_guns(
        gun_ammo_by_oid
            .into_iter()
            .filter_map(|(oid, ammo)| ammo.map(|ammo| (oid, ammo)))
            .collect(),
    );

    // Load planes and set loadouts
    let mut new_planes = TllContainer::<EscadraString, Plane>::new();
    for (plane_name, plane_config) in planes.iter() {
        let mut plane = Plane {
            _padding: [0; 8],
            loadouts: CVec::empty(),
        };

        let new_loadout_map = new_loadouts.get_map();

        for loadout in &plane_config.loadouts {
            plane
                .loadouts
                .insert(*new_loadout_map.get(&loadout.oid).unwrap() as *const loadout::GameLoadout);
        }

        new_planes.insert(plane_name.clone(), plane);
    }

    // Write loadouts to game's loadout TLL
    let loadout_tll_ptr: *mut TllContainer<EscadraString, loadout::GameLoadout> =
        get_loadout_tll_addr() as *mut TllContainer<EscadraString, loadout::GameLoadout>;
    std::ptr::write(loadout_tll_ptr, new_loadouts);

    // Write planes to game's plane TLL
    let plane_tll_ptr: *mut TllContainer<EscadraString, Plane> =
        get_plane_tll_addr() as *mut TllContainer<EscadraString, Plane>;
    std::ptr::write(plane_tll_ptr, new_planes);

    read_tll(loadout_tll_ptr);
    read_tll(plane_tll_ptr);
}

unsafe fn read_tll<T: Eq + Hash + Serialize, U: Serialize>(tll_ptr: *const TllContainer<T, U>) {
    let tll_container = &*tll_ptr;
    if tll_container.size == 0 {
        log::warn!("Loadout TLL container is empty.");
        return;
    }

    let sentinel_ptr = tll_container.sentinel;
    if sentinel_ptr.is_null() {
        log::warn!("Loadout TLL sentinel is null.");
        return;
    }

    let sentinel = &mut *sentinel_ptr;

    let items = HashMap::from(sentinel);

    // let items = Vec::from(sentinel);
    let items_str = serde_json::to_string_pretty(&items).unwrap();
    log::info!("{}", items_str);
}
