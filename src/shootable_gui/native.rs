use std::{
    collections::HashMap,
    ffi::c_void,
    mem::{offset_of, size_of},
    ptr::{addr_of, addr_of_mut},
    sync::{Mutex, OnceLock},
};

use highfleet::v1_163::{Body, Gui};

use super::{
    damage::{apply_impact, INITIAL_HEALTH},
    geometry::{Collider, Point, Projection},
};

type Object = *mut c_void;
const GUI_CODE: u64 = 0x4000f;
const BODY_CODE: u64 = 0xf;
const B2_BODY: usize = 0x178;
const B2_FIXTURE: usize = 0x168;

static NATIVE: OnceLock<Native> = OnceLock::new();
static ELEMENTS: Mutex<Option<HashMap<usize, Element>>> = Mutex::new(None);

struct Native {
    battle: usize,
    zoom: usize,
    centre: usize,
    camera: usize,
    fast_shake: usize,
    slow_shake: usize,
    draw: unsafe extern "C" fn(*mut Gui),
    step: unsafe extern "C" fn(Object, f32, i32, i32),
    contact: unsafe extern "C" fn(Object),
    filter: unsafe extern "C" fn(Object, Object, Object) -> u8,
    project: unsafe extern "C" fn(*mut Body, *mut Point, u8, f32, u8),
    create_body: unsafe extern "C" fn(*mut Body),
    game_fixture: unsafe extern "C" fn(*mut Body, *mut Body),
    create_fixture: unsafe extern "C" fn(Object, *const FixtureDef) -> Object,
    destroy_fixture: unsafe extern "C" fn(Object, Object),
    destroy_body: unsafe extern "C" fn(Object, Object),
    set_transform: unsafe extern "C" fn(Object, *const Point, f32),
    set_enabled: unsafe extern "C" fn(Object, u8),
    detonate: unsafe extern "C" fn(Object, Object),
    polygon_set: unsafe extern "C" fn(*mut Polygon, *const Point, i32),
    polygon_vtable: usize,
}

#[derive(Clone)]
struct Element {
    id: i64,
    saved: Saved,
    seen: bool,
    body: usize,
    fixture: usize,
    vertices: Vec<Point>,
}

#[derive(Clone)]
struct Saved {
    state: i32,
    health: f32,
    health_max: f32,
    health_lock: u8,
    physics_update: u8,
    density: f32,
    layer: i32,
    clickable: u8,
    body: usize,
    had_fixture: bool,
}

#[repr(C)]
struct Polygon {
    vtable: usize,
    kind: i32,
    radius: f32,
    centroid: Point,
    vertices: [Point; 8],
    normals: [Point; 8],
    count: i32,
    padding: u32,
}

#[repr(C)]
struct FixtureDef {
    shape: *const Polygon,
    user_data: Object,
    friction: f32,
    restitution: f32,
    density: f32,
    sensor: u8,
    padding: u8,
    category: u16,
    mask: u16,
    group: i16,
}

const _: () = {
    assert!(size_of::<Polygon>() == 0xa0);
    assert!(offset_of!(Polygon, count) == 0x98);
    assert!(offset_of!(FixtureDef, user_data) == 8);
    assert!(offset_of!(FixtureDef, category) == 0x1e);
    assert!(offset_of!(FixtureDef, group) == 0x22);
    assert!(offset_of!(Body, health) == 0x2c8);
    assert!(offset_of!(Body, is_physic_update) == 0x208);
    assert!(offset_of!(Gui, clickable) == 0xca0);
};

pub(super) unsafe fn configure<E>(
    mut resolve: impl FnMut(usize) -> std::result::Result<usize, E>,
) -> std::result::Result<bool, E> {
    let native = Native {
        battle: resolve(0x7fc6fb7)?,
        zoom: resolve(0x3c11c4)?,
        centre: resolve(0x801fd10)?,
        camera: resolve(0x8075ce8)?,
        fast_shake: resolve(0x8075cf0)?,
        slow_shake: resolve(0x8075cf8)?,
        draw: std::mem::transmute(resolve(0x14f370)?),
        step: std::mem::transmute(resolve(0x31bc90)?),
        contact: std::mem::transmute(resolve(0x2849a0)?),
        filter: std::mem::transmute(resolve(0x2858c0)?),
        project: std::mem::transmute(resolve(0x4ec80)?),
        create_body: std::mem::transmute(resolve(0x4b330)?),
        game_fixture: std::mem::transmute(resolve(0x4b530)?),
        create_fixture: std::mem::transmute(resolve(0x319db0)?),
        destroy_fixture: std::mem::transmute(resolve(0x319e80)?),
        destroy_body: std::mem::transmute(resolve(0x31a910)?),
        set_transform: std::mem::transmute(resolve(0x31a290)?),
        set_enabled: std::mem::transmute(resolve(0x31a190)?),
        detonate: std::mem::transmute(resolve(0x1d1e0)?),
        polygon_set: std::mem::transmute(resolve(0x319aa0)?),
        polygon_vtable: resolve(0x3529d8)?,
    };
    Ok(NATIVE.set(native).is_ok())
}

fn elements<T>(f: impl FnOnce(&mut HashMap<usize, Element>) -> T) -> T {
    let mut guard = ELEMENTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

unsafe fn read_at<T: Copy>(object: Object, offset: usize) -> T {
    object.cast::<u8>().add(offset).cast::<T>().read_unaligned()
}

unsafe fn write_at<T>(object: Object, offset: usize, value: T) {
    object
        .cast::<u8>()
        .add(offset)
        .cast::<T>()
        .write_unaligned(value);
}

unsafe fn is_gui(object: Object) -> bool {
    !object.is_null()
        && addr_of!((*object.cast::<Body>()).base.base.base.code).read() & GUI_CODE == GUI_CODE
}

unsafe fn in_battle() -> bool {
    (NATIVE.get().unwrap().battle as *const u8).read() != 0
}

unsafe fn has_broken_parent(gui: *mut Gui) -> bool {
    let mut parent = addr_of!((*gui).base.base.master.target).read();
    // Bound malformed/cyclic master chains without dereferencing arbitrary
    // side-table pointers. Each parent is a native live Link target.
    for _ in 0..128 {
        if parent.is_null() || (*parent).base.base.code & BODY_CODE != BODY_CODE {
            return false;
        }
        if is_gui(parent.cast())
            && elements(|all| all.contains_key(&(parent as usize)))
            && (*parent.cast::<Body>()).health <= 0.0
        {
            return true;
        }
        parent = (*parent).master.target;
    }
    true
}

pub(super) unsafe extern "C" fn draw_element(gui: *mut Gui) {
    if may_draw(gui) {
        (NATIVE.get().unwrap().draw)(gui);
    }
}

pub(super) unsafe extern "C" fn may_draw(gui: *mut Gui) -> bool {
    if !in_battle() {
        restore_all();
        return true;
    }
    if gui.is_null() || !is_gui(gui.cast()) {
        return true;
    }
    if (*gui).base.base.state == 0 {
        return false;
    }
    let id = addr_of!((*gui).base.base.id).read();
    elements(|all| {
        all.entry(gui as usize)
            .and_modify(|element| element.id = id)
            .or_insert_with(|| {
                let saved = Saved {
                    state: (*gui).base.base.state,
                    health: (*gui).base.health,
                    health_max: (*gui).base.health_max,
                    health_lock: (*gui).base.health_lock,
                    physics_update: (*gui).base.is_physic_update,
                    density: (*gui).base.density,
                    layer: (*gui).base.layer,
                    clickable: (*gui).clickable,
                    body: read_at::<Object>(gui.cast(), B2_BODY) as usize,
                    had_fixture: !read_at::<Object>(gui.cast(), B2_FIXTURE).is_null(),
                };
                (*gui).base.health = INITIAL_HEALTH;
                (*gui).base.health_max = INITIAL_HEALTH;
                // Only our post-solver round contacts may damage GUI. Native blast/
                // ship-part damage assumes world-space Body fields and Creature owners.
                (*gui).base.health_lock = 1;
                (*gui).base.is_physic_update = 0;
                Element {
                    id,
                    saved,
                    seen: false,
                    body: 0,
                    fixture: 0,
                    vertices: Vec::new(),
                }
            })
            .seen = true;
    });
    if (*gui).base.health <= 0.0 || has_broken_parent(gui) {
        (*gui).base.base.state = 1; // inactive, but NOT queued for deletion
        (*gui).clickable = 0;
        return false;
    }
    true
}

pub(super) unsafe extern "C" fn forget_element(body: Object) {
    // The native Body destructor immediately frees its attached b2Body itself.
    // Remove the borrowed pointer before that destructor can free/reuse the GUI.
    elements(|all| {
        all.remove(&(body as usize));
    });
}

unsafe fn projection() -> Projection {
    let native = NATIVE.get().unwrap();
    let fast = (native.fast_shake as *const Point).read_unaligned();
    let slow = (native.slow_shake as *const Point).read_unaligned();
    Projection {
        centre: (native.centre as *const Point).read_unaligned(),
        camera: (native.camera as *const Point).read_unaligned(),
        // Invert the WORLD projectile projection, not the GUI projection.
        // Body defaults set +0x91e = 1; the GUI's independent shake flag is
        // already respected by screen_point's native forward mapping.
        shake: Point {
            x: fast.x + slow.x,
            y: fast.y + slow.y,
        },
        zoom: (native.zoom as *const f32).read(),
    }
}

unsafe fn screen_point(gui: *mut Gui, mut point: Point) -> Point {
    // +0xe8 is the forward local-point transform. Body_TransformPointFromWorld
    // at +0xf8 is the inverse and must not be used for projection.
    let vtable: *const usize = read_at(gui.cast(), 0);
    let local_to_world: unsafe extern "C" fn(*mut Gui, *mut Point) =
        std::mem::transmute(vtable.add(0xe8 / 8).read());
    local_to_world(gui, &mut point);
    (NATIVE.get().unwrap().project)(gui.cast(), &mut point, 1, 1.0, 1);
    point
}

unsafe fn collider(gui: *mut Gui, camera: Projection) -> Option<Collider> {
    let mesh = addr_of!((*gui).base.mesh).read().cast::<c_void>();
    if mesh.is_null() {
        return None;
    }
    let count: i32 = read_at(mesh, 0x50);
    if !(3..=8).contains(&count) {
        return None;
    }
    let points: *const Point = read_at(mesh, 0x48);
    if points.is_null() {
        return None;
    }
    let origin = screen_point(gui, Point::default());
    let pixels: Vec<Point> = std::slice::from_raw_parts(points, count as usize)
        .iter()
        .map(|&point| screen_point(gui, point))
        .collect();
    Collider::from_screen(camera, origin, &pixels)
}

unsafe fn release_physics(gui: *mut Gui, expected_body: usize) {
    let object = gui.cast();
    let body: Object = read_at(object, B2_BODY);
    if !body.is_null() && body as usize == expected_body {
        // Use the body's actual world, not the current SuperBig global.
        let world: Object = read_at(body, 0x58);
        (NATIVE.get().unwrap().destroy_body)(world, body);
        write_at(object, B2_BODY, std::ptr::null_mut::<c_void>());
        write_at(object, B2_FIXTURE, std::ptr::null_mut::<c_void>());
    }
}

unsafe fn restore_all() {
    let previous = elements(std::mem::take);
    for (address, element) in previous {
        let gui = address as *mut Gui;
        if (*gui).base.base.state == 0 {
            continue;
        }
        let restore_native_body = element.body != 0
            && element.body == element.saved.body
            && read_at::<Object>(gui.cast(), B2_BODY) as usize == element.body;
        let fixture: Object = read_at(gui.cast(), B2_FIXTURE);
        let restore_native_fixture =
            restore_native_body && (fixture.is_null() || fixture as usize == element.fixture);
        if !restore_native_body {
            release_physics(gui, element.body);
        } else if restore_native_fixture {
            if !fixture.is_null() {
                (NATIVE.get().unwrap().destroy_fixture)(element.body as Object, fixture);
                write_at(gui.cast(), B2_FIXTURE, std::ptr::null_mut::<c_void>());
            }
        }
        (*gui).base.base.state = element.saved.state;
        (*gui).base.health = element.saved.health;
        (*gui).base.health_max = element.saved.health_max;
        (*gui).base.health_lock = element.saved.health_lock;
        (*gui).base.is_physic_update = element.saved.physics_update;
        (*gui).base.density = element.saved.density;
        (*gui).base.layer = element.saved.layer;
        (*gui).clickable = element.saved.clickable;
        if restore_native_body {
            let native = NATIVE.get().unwrap();
            let position = Point {
                x: (*gui).base.position_x * 0.05,
                y: (*gui).base.position_y * 0.05,
            };
            (native.set_enabled)(element.body as Object, 1);
            (native.set_transform)(element.body as Object, &position, (*gui).base.angle);
            if element.saved.had_fixture && restore_native_fixture {
                (native.game_fixture)(gui.cast(), gui.cast());
            }
        }
    }
}

pub(super) unsafe extern "C" fn step_world(
    world: Object,
    dt: f32,
    velocity_steps: i32,
    position_steps: i32,
) {
    let native = NATIVE.get().unwrap();
    if !in_battle() {
        restore_all();
        (native.step)(world, dt, velocity_steps, position_steps);
        return;
    }
    let camera = projection();
    let snapshot = elements(|all| {
        all.iter()
            .map(|(&key, value)| (key, value.clone()))
            .collect::<Vec<_>>()
    });
    for (address, mut element) in snapshot {
        let gui = address as *mut Gui;
        // Node.id is serialized and can change during native reload. Allocation
        // lifetime is tracked by the destructor hook, not by this mutable value.
        element.id = addr_of!((*gui).base.base.id).read();
        let current_body: Object = read_at(gui.cast(), B2_BODY);
        if element.body != 0 && current_body as usize != element.body {
            // Unreflect can release physics even when the GUI itself survives.
            // Invalidate before BOTH the visible and hidden branches use it.
            element.body = 0;
            element.fixture = 0;
            element.vertices.clear();
            element.saved.body = current_body as usize;
            element.saved.had_fixture = !read_at::<Object>(gui.cast(), B2_FIXTURE).is_null();
        }
        (*gui).base.is_physic_update = 0;
        let visible = element.seen
            && (*gui).base.base.state >= 2
            && (*gui).base.opacity > 0.0
            && (*gui).base.health > 0.0
            && !has_broken_parent(gui);
        let geometry = if visible { collider(gui, camera) } else { None };
        if let Some(geometry) = geometry {
            attach(gui, &mut element, &geometry);
        } else if element.body != 0 {
            (native.set_enabled)(element.body as Object, 0);
        }
        element.seen = false;
        elements(|all| {
            all.insert(address, element);
        });
    }
    (native.step)(world, dt, velocity_steps, position_steps);
}

unsafe fn attach(gui: *mut Gui, element: &mut Element, geometry: &Collider) {
    let native = NATIVE.get().unwrap();
    let object = gui.cast();
    let mut body: Object = read_at(object, B2_BODY);
    if element.body == 0 {
        // A root GUI may already have its own static body. Reuse that actual
        // GUI body and rebuild its native fixture on reset. Frame-owned fixtures
        // cannot be adopted without changing another object's physics ownership.
        if body.is_null() && !read_at::<Object>(object, B2_FIXTURE).is_null() {
            return;
        }
        (*gui).base.density = 0.0;
        (*gui).base.layer = 0xf;
        if body.is_null() {
            (native.create_body)(gui.cast());
            body = read_at(object, B2_BODY);
        } else if read_at::<i32>(body, 0) != 0 || read_at::<Object>(body, 0xb0) != object {
            return;
        }
        if body.is_null() {
            return;
        }
        element.body = body as usize;
        log::debug!(
            "shootable_gui: attached real GUI {gui:p}, id {}",
            element.id
        );
    } else if body as usize != element.body {
        // Native unreflect/load can tear down the physics without freeing GUI.
        element.body = 0;
        element.fixture = 0;
        element.vertices.clear();
        return;
    }
    (native.set_enabled)(body, 1);
    let origin = Point {
        x: geometry.origin.x * 0.05,
        y: geometry.origin.y * 0.05,
    };
    (native.set_transform)(body, &origin, 0.0);
    // Pixel-space subtraction can change a vertex by a few ULPs when a panel
    // moves. Keep its contacts intact for changes far below Box2D's 0.01m margin.
    let same_shape = element.vertices.len() == geometry.vertices.len()
        && element
            .vertices
            .iter()
            .zip(&geometry.vertices)
            .all(|(a, b)| (a.x - b.x).abs() <= 0.0001 && (a.y - b.y).abs() <= 0.0001);
    let fixture: Object = read_at(object, B2_FIXTURE);
    if fixture.is_null() || fixture as usize != element.fixture || !same_shape {
        if !fixture.is_null() {
            (native.destroy_fixture)(body, fixture);
            write_at(object, B2_FIXTURE, std::ptr::null_mut::<c_void>());
        }
        let mut shape = Polygon {
            vtable: native.polygon_vtable,
            kind: 2,
            radius: 0.01,
            centroid: Point::default(),
            vertices: [Point::default(); 8],
            normals: [Point::default(); 8],
            count: 0,
            padding: 0,
        };
        (native.polygon_set)(
            &mut shape,
            geometry.vertices.as_ptr(),
            geometry.vertices.len() as i32,
        );
        let definition = FixtureDef {
            shape: &shape,
            user_data: object,
            friction: 0.6,
            restitution: 0.001,
            density: 0.0,
            sensor: 0,
            padding: 0,
            category: 1,
            mask: 0xffff,
            group: 0,
        };
        let fixture = (native.create_fixture)(body, &definition);
        write_at(object, B2_FIXTURE, fixture);
        element.fixture = fixture as usize;
        if !fixture.is_null() {
            element.vertices = geometry.vertices.clone();
            log::debug!("shootable_gui: GUI={gui:p}, b2Body={body:p}, fixture={fixture:p}, userData={gui:p}");
        }
    }
}

fn registered(object: Object) -> bool {
    !object.is_null() && elements(|all| all.contains_key(&(object as usize)))
}

unsafe fn projectile(object: Object) -> bool {
    !object.is_null()
        && addr_of!((*object.cast::<Body>()).base.base.base.code).read() & BODY_CODE == BODY_CODE
        && !is_gui(object)
        && addr_of!((*object.cast::<Body>()).base.state).read() != 0
        && (addr_of!((*object.cast::<Body>()).meta).read() & 0xd == 0xd
            || addr_of!((*object.cast::<Body>()).bullet).read() != 0)
}

pub(super) unsafe extern "C" fn should_collide(filter: Object, a: Object, b: Object) -> u8 {
    if a.is_null() || b.is_null() {
        return 0;
    }
    let first: Object = read_at(a, 0x40);
    let second: Object = read_at(b, 0x40);
    let gui_a = registered(first);
    let gui_b = registered(second);
    if gui_a || gui_b {
        if !in_battle() || gui_a == gui_b {
            return 0;
        }
        let (gui, round) = if gui_a {
            (first, second)
        } else {
            (second, first)
        };
        return (addr_of!((*gui.cast::<Body>()).base.state).read() >= 2
            && addr_of!((*gui.cast::<Body>()).health).read() > 0.0
            && projectile(round)) as u8;
    }
    (NATIVE.get().unwrap().filter)(filter, a, b)
}

pub(super) unsafe extern "C" fn process_contact(record: Object) {
    let a: Object = read_at(record, 8);
    let b: Object = read_at(record, 0x20);
    let gui_a = registered(a);
    let gui_b = registered(b);
    if !gui_a && !gui_b {
        (NATIVE.get().unwrap().contact)(record);
        return;
    }
    if !in_battle() || gui_a == gui_b {
        return;
    }
    let (gui, round) = if gui_a {
        (a.cast::<Gui>(), b)
    } else {
        (b.cast::<Gui>(), a)
    };
    if (*gui).base.base.state < 2 || !projectile(round) {
        return;
    }
    let impulse: f32 = read_at(record, 0x40);
    let health = addr_of_mut!((*gui).base.health);
    let mut remaining = health.read();
    if apply_impact(&mut remaining, impulse) {
        health.write(remaining);
        if remaining == 0.0 {
            (*gui).base.base.state = 1;
            (*gui).clickable = 0;
            log::info!(
                "shootable_gui: destroyed actual GUI {gui:p}, id {}",
                (*gui).base.base.id
            );
        }
        let threshold = addr_of!((*round.cast::<Body>()).collision_sensor).read();
        if threshold >= 0.0 && threshold < impulse / 0.05_f32.powi(4) {
            // The native callee reads only the projectile and target arguments.
            // Its caller normally gates it with the world-unit impulse threshold.
            (NATIVE.get().unwrap().detonate)(round, gui.cast());
        }
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
