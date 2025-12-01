use std::{collections::HashMap, hash::Hash};

use highfleet::general::EscadraString;
use serde::Serialize;

use crate::structs::{cvec::CVec, loadout, plane::Plane, tll::TllContainer};

pub unsafe fn patch_planes() {
    let loadout_tll_addr: u64;
    if cfg!(feature = "1_151") {
        loadout_tll_addr = 0x143944528;
    } else if cfg!(feature = "1_163") {
        loadout_tll_addr = 0x143a15a60;
    } else {
        // Default to 1.163
        loadout_tll_addr = 0x143a15a60;
    }

    let tll_container_ptr = loadout_tll_addr as *const TllContainer<EscadraString, loadout::Loadout>
        as *mut TllContainer<EscadraString, loadout::Loadout>;

    log::info!("Inserting custom loadout LOADOUT_LA29_GUN40");
    let tll_container = &mut *tll_container_ptr;
    tll_container.insert(
        EscadraString::from("LOADOUT_LA29_GUN40"),
        loadout::Loadout {
            plane_loadout: EscadraString::from("LOADOUT_LA29_GUN40"),
            generic_loadout: EscadraString::from("LOADOUT_GUN37"),
            items: CVec::empty(),
            launch_loadout_weight: 9999,
            has_gun37mm: true,
            _padding: [0; 3],
        },
    );

    tll_container.insert(
        EscadraString::from("LOADOUT_MB210_GUN37"),
        loadout::Loadout {
            plane_loadout: EscadraString::from("LOADOUT_MB210_GUN37"),
            generic_loadout: EscadraString::from("LOADOUT_GUN37"),
            items: CVec::empty(),
            launch_loadout_weight: 9999,
            has_gun37mm: true,
            _padding: [0; 3],
        },
    );
    log::info!("Custom loadout inserted.");

    let loadouts = tll_container.get_map();
    let la29_loadout = loadouts
        .get(&EscadraString::from("LOADOUT_LA29_GUN40"))
        .expect("Custom loadout should have been inserted");

    let mb210_loadout = loadouts
        .get(&EscadraString::from("LOADOUT_MB210_GUN37"))
        .expect("Custom loadout should have been inserted");

    let plane_tll_addr: u64;
    if cfg!(feature = "1_151") {
        plane_tll_addr = 0x143942740;
    } else if cfg!(feature = "1_163") {
        plane_tll_addr = 0x143a13c50;
    } else {
        // Default to 1.163
        plane_tll_addr = 0x143a13c50;
    }

    let plane_tll_ptr = plane_tll_addr as *const TllContainer<EscadraString, Plane>
        as *mut TllContainer<EscadraString, Plane>;
    let plane_container = &mut *plane_tll_ptr;
    plane_container.insert(
        EscadraString::from("CRAFT_MB210"),
        Plane {
            _padding: [0; 8],
            loadouts: CVec::empty(),
        },
    );

    let mut planes = plane_container.get_map();
    let la29 = planes
        .get_mut(&EscadraString::from("CRAFT_LA29"))
        .expect("LA29 should always exist");
    la29.loadouts
        .insert(*la29_loadout as *const loadout::Loadout);
    log::info!("Custom loadout added to LA29 plane.");

    let mb210 = planes
        .get_mut(&EscadraString::from("CRAFT_MB210"))
        .expect("MB210 should always exist");
    mb210
        .loadouts
        .insert(*mb210_loadout as *const loadout::Loadout);
    log::info!("Custom loadout added to MB210 plane.");

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
