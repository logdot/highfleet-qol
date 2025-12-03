use std::{collections::HashMap, hash::Hash};

use highfleet::general::EscadraString;
use serde::Serialize;

use crate::structs::{cvec::CVec, loadout, plane::Plane, tll::TllContainer};

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

pub fn get_planes() -> HashMap<EscadraString, Vec<loadout::Loadout>> {
    let loadout_tll_addr = get_plane_tll_addr();
    let tll_container_ptr = loadout_tll_addr as *const TllContainer<EscadraString, Plane>
        as *mut TllContainer<EscadraString, Plane>;

    unsafe {
        let tll_container = &mut *tll_container_ptr;

        tll_container
            .get_map()
            .into_iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.loadouts
                        .items()
                        .into_iter()
                        .map(|&ptr| (*ptr).clone())
                        .collect::<Vec<_>>(),
                )
            })
            .collect()
    }
}

pub unsafe fn patch_planes(planes: &HashMap<EscadraString, Vec<loadout::Loadout>>) {
    // Load all loadouts from config
    let mut new_loadouts = TllContainer::<EscadraString, loadout::Loadout>::new();
    for plane_loadouts in planes.values() {
        for loadout in plane_loadouts {
            new_loadouts.insert(loadout.plane_loadout.clone(), loadout.clone());
        }
    }

    // Load planes and set loadouts
    let mut new_planes = TllContainer::<EscadraString, Plane>::new();
    for (plane_name, plane_loadouts) in planes.iter() {
        let mut plane = Plane {
            _padding: [0; 8],
            loadouts: CVec::empty(),
        };

        let new_loadout_map = new_loadouts.get_map();

        for loadout in plane_loadouts {
            plane
                .loadouts
                .insert(*new_loadout_map.get(&loadout.plane_loadout).unwrap()
                    as *const loadout::Loadout);
        }

        new_planes.insert(plane_name.clone(), plane);
    }

    // Write loadouts to game's loadout TLL
    let loadout_tll_ptr: *mut TllContainer<EscadraString, loadout::Loadout> =
        get_loadout_tll_addr() as *mut TllContainer<EscadraString, loadout::Loadout>;
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
