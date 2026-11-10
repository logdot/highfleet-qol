use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Once,
};

static SERIAL: Mutex<()> = Mutex::new(());
static SETUP: Once = Once::new();
static NATIVE_CONTACTS: AtomicUsize = AtomicUsize::new(0);
static NATIVE_DRAWS: AtomicUsize = AtomicUsize::new(0);
static TRANSFORMS: AtomicUsize = AtomicUsize::new(0);
static ENABLE_CHANGES: AtomicUsize = AtomicUsize::new(0);
static FAIL_NEXT_FIXTURE: AtomicBool = AtomicBool::new(false);
static FIXTURE_CREATIONS: AtomicUsize = AtomicUsize::new(0);

#[repr(align(8))]
struct Memory<const N: usize>([u8; N]);

static mut BATTLE: u8 = 1;
static ZOOM: f32 = 0.5;
static CENTRE: Point = Point { x: 960.0, y: 540.0 };
static CAMERA: Point = Point {
    x: 2000.0,
    y: -1000.0,
};
static SHAKE: Point = Point { x: 2.0, y: 3.0 };
static ZERO: Point = Point { x: 0.0, y: 0.0 };

struct Instrument {
    storage: Box<Memory<0xe70>>,
    _mesh: Box<Memory<0x58>>,
    _points: Box<[Point; 4]>,
    _vtable: Box<[usize; 107]>,
}

impl Instrument {
    unsafe fn new(id: i64) -> Self {
        let mut storage = Box::new(Memory([0; 0xe70]));
        let mut mesh = Box::new(Memory([0; 0x58]));
        let points = Box::new([
            Point { x: -20.0, y: -10.0 },
            Point { x: 20.0, y: -10.0 },
            Point { x: 20.0, y: 10.0 },
            Point { x: -20.0, y: 10.0 },
        ]);
        let mut vtable = Box::new([0; 107]);
        vtable[0xe8 / 8] = local_point as *const () as usize;
        let object = storage.0.as_mut_ptr().cast();
        write_at(object, 0, vtable.as_ptr());
        write_at(object, 0x28, GUI_CODE);
        write_at(object, 0x48, 2_i32);
        write_at(object, 0x78, id);
        write_at(object, 0x128, Point { x: 100.0, y: 200.0 });
        write_at(object, 0x1b8, mesh.0.as_mut_ptr());
        write_at(object, 0x208, 1_u8);
        write_at(object, 0x2c8, 42.0_f32);
        write_at(object, 0x2cc, 84.0_f32);
        write_at(object, 0x8f8, 1.0_f32);
        write_at(object, 0x91c, 1_u8);
        write_at(object, 0xca0, 1_u8);
        let mesh_object = mesh.0.as_mut_ptr().cast();
        write_at(mesh_object, 0x48, points.as_ptr());
        write_at(mesh_object, 0x50, 4_i32);
        Self {
            storage,
            _mesh: mesh,
            _points: points,
            _vtable: vtable,
        }
    }

    fn gui(&mut self) -> *mut Gui {
        self.storage.0.as_mut_ptr().cast()
    }
}

impl Drop for Instrument {
    fn drop(&mut self) {
        unsafe {
            let gui = self.gui();
            forget_element(gui.cast());
            let body: Object = read_at(gui.cast(), B2_BODY);
            if !body.is_null() {
                destroy_body(std::ptr::null_mut(), body);
            }
        }
    }
}

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let lock = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    SETUP.call_once(|| {
        let native = Native {
            battle: addr_of!(BATTLE) as usize,
            zoom: addr_of!(ZOOM) as usize,
            centre: addr_of!(CENTRE) as usize,
            camera: addr_of!(CAMERA) as usize,
            fast_shake: addr_of!(SHAKE) as usize,
            slow_shake: addr_of!(ZERO) as usize,
            draw,
            step,
            contact,
            filter,
            project,
            create_body,
            game_fixture,
            create_fixture,
            destroy_fixture,
            destroy_body,
            set_transform,
            set_enabled,
            detonate,
            polygon_set,
            polygon_vtable: 0,
        };
        assert!(NATIVE.set(native).is_ok());
    });
    unsafe {
        restore_all();
        addr_of_mut!(BATTLE).write(1);
    }
    lock
}

unsafe extern "C" fn local_point(gui: *mut Gui, point: *mut Point) {
    (*point).x += (*gui).base.position_x;
    (*point).y += (*gui).base.position_y;
}

unsafe extern "C" fn project(gui: *mut Body, point: *mut Point, _: u8, _: f32, _: u8) {
    if read_at::<u8>(gui.cast(), 0x91e) != 0 {
        (*point).x -= SHAKE.x;
        (*point).y -= SHAKE.y;
    }
    (*point).x += CENTRE.x;
    (*point).y += CENTRE.y;
}

unsafe extern "C" fn draw(_: *mut Gui) {
    NATIVE_DRAWS.fetch_add(1, Ordering::Relaxed);
}
unsafe extern "C" fn step(_: Object, _: f32, _: i32, _: i32) {}
unsafe extern "C" fn contact(_: Object) {
    NATIVE_CONTACTS.fetch_add(1, Ordering::Relaxed);
}
unsafe extern "C" fn filter(_: Object, _: Object, _: Object) -> u8 {
    1
}
unsafe extern "C" fn detonate(_: Object, _: Object) {}

unsafe extern "C" fn create_body(gui: *mut Body) {
    let body = Box::into_raw(Box::new(Memory([0; 0xb8]))).cast();
    write_at(body, 0xb0, gui);
    write_at(gui.cast(), B2_BODY, body);
}

unsafe extern "C" fn create_fixture(_: Object, definition: *const FixtureDef) -> Object {
    FIXTURE_CREATIONS.fetch_add(1, Ordering::Relaxed);
    if FAIL_NEXT_FIXTURE.swap(false, Ordering::Relaxed) {
        return std::ptr::null_mut();
    }
    let fixture = Box::into_raw(Box::new(Memory([0; 0x60]))).cast();
    write_at(fixture, 0x40, (*definition).user_data);
    fixture
}

unsafe extern "C" fn game_fixture(gui: *mut Body, _: *mut Body) {
    let fixture = Box::into_raw(Box::new(Memory([0; 0x60]))).cast::<c_void>();
    write_at(fixture, 0x40, gui);
    write_at(gui.cast(), B2_FIXTURE, fixture);
}

unsafe extern "C" fn destroy_fixture(_: Object, fixture: Object) {
    drop(Box::from_raw(fixture.cast::<Memory<0x60>>()));
}

unsafe extern "C" fn destroy_body(_: Object, body: Object) {
    let gui: Object = read_at(body, 0xb0);
    let fixture: Object = read_at(gui, B2_FIXTURE);
    if !fixture.is_null() {
        destroy_fixture(body, fixture);
    }
    drop(Box::from_raw(body.cast::<Memory<0xb8>>()));
}

unsafe extern "C" fn set_transform(body: Object, point: *const Point, angle: f32) {
    TRANSFORMS.fetch_add(1, Ordering::Relaxed);
    write_at(body, 0xc, point.read());
    write_at(body, 0x38, angle);
}

unsafe extern "C" fn set_enabled(body: Object, enabled: u8) {
    ENABLE_CHANGES.fetch_add(1, Ordering::Relaxed);
    write_at(body, 4, enabled as u16);
}

unsafe extern "C" fn polygon_set(shape: *mut Polygon, points: *const Point, count: i32) {
    (*shape).count = count;
    std::ptr::copy_nonoverlapping(points, (*shape).vertices.as_mut_ptr(), count as usize);
}

#[test]
fn attached_fixture_identifies_the_actual_gui_object() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(1);
        let gui = instrument.gui();
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        let body: Object = read_at(gui.cast(), B2_BODY);
        let fixture: Object = read_at(gui.cast(), B2_FIXTURE);
        assert!(!body.is_null());
        assert!(!fixture.is_null());
        assert_eq!(read_at::<Object>(body, 0xb0), gui.cast());
        assert_eq!(read_at::<Object>(fixture, 0x40), gui.cast());
        assert_eq!((*gui).base.position_x, 100.0);
        assert_eq!((*gui).base.position_y, 200.0);
        assert_eq!((*gui).base.is_physic_update, 0);
        assert_eq!(
            read_at::<Point>(body, 0xc),
            Point {
                x: 110.1,
                y: -29.85
            }
        );
        restore_all();
    }
}

#[test]
fn lethal_impact_deactivates_the_gui_without_freeing_it() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(2);
        let gui = instrument.gui();
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        let mut round = Memory([0; 0xa40]);
        let shell = round.0.as_mut_ptr().cast();
        write_at(shell, 0x28, BODY_CODE);
        write_at(shell, 0x48, 2_i32);
        write_at(shell, 0x1c0, 0xd_u64);
        write_at(shell, 0x38c, -1.0_f32);
        let mut record = Memory([0; 0x48]);
        let record = record.0.as_mut_ptr().cast();
        write_at(record, 8, gui);
        write_at(record, 0x20, shell);
        write_at(record, 0x40, 100.0_f32);
        process_contact(record);
        assert_eq!((*gui).base.health, 0.0);
        assert_eq!((*gui).base.base.state, 1);
        assert_eq!((*gui).clickable, 0);
        assert!(!may_draw(gui));
        assert!(registered(gui.cast()));
        assert!(!read_at::<Object>(gui.cast(), B2_BODY).is_null());
        restore_all();
    }
}

#[test]
fn leaving_battle_restores_the_existing_gui() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(3);
        let gui = instrument.gui();
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        (*gui).base.health = 0.0;
        (*gui).base.base.state = 1;
        (*gui).clickable = 0;
        addr_of_mut!(BATTLE).write(0);
        assert!(may_draw(gui));
        assert_eq!((*gui).base.health, 42.0);
        assert_eq!((*gui).base.health_max, 84.0);
        assert_eq!((*gui).base.base.state, 2);
        assert_eq!((*gui).base.is_physic_update, 1);
        assert_eq!((*gui).base.health_lock, 0);
        assert_eq!((*gui).clickable, 1);
        assert!(read_at::<Object>(gui.cast(), B2_BODY).is_null());
        assert!(!registered(gui.cast()));
    }
}

#[test]
fn battle_exit_does_not_resurrect_a_gui_queued_for_native_deletion() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(4);
        let gui = instrument.gui();
        assert!(may_draw(gui));
        (*gui).base.base.state = 0;
        restore_all();
        assert_eq!((*gui).base.base.state, 0);
    }
}

#[test]
fn ordinary_contacts_keep_the_native_contact_path() {
    let _lock = setup();
    unsafe {
        let before = NATIVE_CONTACTS.load(Ordering::Relaxed);
        let mut record = Memory([0; 0x48]);
        process_contact(record.0.as_mut_ptr().cast());
        assert_eq!(NATIVE_CONTACTS.load(Ordering::Relaxed), before + 1);
    }
}

#[test]
fn broken_instruments_do_not_reappear_when_the_game_draws_them_again() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(6);
        let gui = instrument.gui();
        draw_element(gui);
        let before = NATIVE_DRAWS.load(Ordering::Relaxed);
        (*gui).base.health = 0.0;
        draw_element(gui);
        draw_element(gui);
        assert_eq!(NATIVE_DRAWS.load(Ordering::Relaxed), before);
        assert_eq!((*gui).base.health, 0.0);
        restore_all();
    }
}

#[test]
fn gui_fixtures_hit_rounds_without_colliding_with_other_gui() {
    let _lock = setup();
    unsafe {
        let mut first = Instrument::new(7);
        let mut second = Instrument::new(8);
        assert!(may_draw(first.gui()));
        assert!(may_draw(second.gui()));
        let mut a = Memory([0; 0x60]);
        let mut b = Memory([0; 0x60]);
        let a = a.0.as_mut_ptr().cast();
        let b = b.0.as_mut_ptr().cast();
        write_at(a, 0x40, first.gui());
        write_at(b, 0x40, second.gui());
        assert_eq!(should_collide(std::ptr::null_mut(), a, b), 0);
        let mut round = Memory([0; 0xa40]);
        let shell = round.0.as_mut_ptr().cast();
        write_at(shell, 0x28, BODY_CODE);
        write_at(shell, 0x48, 2_i32);
        write_at(shell, 0x1c0, 0xd_u64);
        write_at(b, 0x40, shell);
        // In particular, owner.id == 0 on both objects must not exclude a round.
        assert_eq!(should_collide(std::ptr::null_mut(), a, b), 1);
        write_at(shell, 0x1c0, 0_u64);
        assert_eq!(should_collide(std::ptr::null_mut(), a, b), 0);
        // A pair without GUI goes through the native filter unchanged.
        write_at(a, 0x40, shell);
        assert_eq!(should_collide(std::ptr::null_mut(), a, b), 1);
        restore_all();
    }
}

#[test]
fn native_destruction_removes_the_borrowed_gui_pointer() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(9);
        let gui = instrument.gui();
        assert!(may_draw(gui));
        assert!(registered(gui.cast()));
        forget_element(gui.cast());
        assert!(!registered(gui.cast()));
        assert!(elements(|all| all.is_empty()));
    }
}

#[test]
fn unavailable_game_addresses_fail_configuration_before_installing_it() {
    let _lock = setup();
    unsafe {
        assert_eq!(configure(|_| Err("unavailable")), Err("unavailable"));
    }
}

#[test]
fn reset_does_not_touch_a_native_body_released_by_unreflect() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(5);
        let gui = instrument.gui();
        create_body(gui.cast());
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        let old_body: Object = read_at(gui.cast(), B2_BODY);
        let old_fixture: Object = read_at(gui.cast(), B2_FIXTURE);
        // Native unreflect clears these slots. Keep the old allocations alive
        // until the assertion so an erroneous call is observable, not a UAF.
        write_at(gui.cast(), B2_BODY, std::ptr::null_mut::<c_void>());
        write_at(gui.cast(), B2_FIXTURE, std::ptr::null_mut::<c_void>());
        let before = TRANSFORMS.load(Ordering::Relaxed);
        restore_all();
        let after = TRANSFORMS.load(Ordering::Relaxed);
        destroy_fixture(old_body, old_fixture);
        drop(Box::from_raw(old_body.cast::<Memory<0xb8>>()));
        assert_eq!(after, before);
    }
}

#[test]
fn hidden_gui_does_not_use_a_body_released_by_native_unreflect() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(10);
        let gui = instrument.gui();
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        let old_body: Object = read_at(gui.cast(), B2_BODY);
        let old_fixture: Object = read_at(gui.cast(), B2_FIXTURE);
        write_at(gui.cast(), B2_BODY, std::ptr::null_mut::<c_void>());
        write_at(gui.cast(), B2_FIXTURE, std::ptr::null_mut::<c_void>());
        let before = ENABLE_CHANGES.load(Ordering::Relaxed);
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        let after = ENABLE_CHANGES.load(Ordering::Relaxed);
        restore_all();
        destroy_fixture(old_body, old_fixture);
        drop(Box::from_raw(old_body.cast::<Memory<0xb8>>()));
        assert_eq!(after, before);
    }
}

#[test]
fn fixture_creation_is_retried_after_a_failed_native_allocation() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(11);
        let gui = instrument.gui();
        assert!(may_draw(gui));
        FAIL_NEXT_FIXTURE.store(true, Ordering::Relaxed);
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        assert!(read_at::<Object>(gui.cast(), B2_FIXTURE).is_null());
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        let recovered = !read_at::<Object>(gui.cast(), B2_FIXTURE).is_null();
        restore_all();
        assert!(recovered);
    }
}

#[test]
fn subpixel_translation_does_not_rebuild_the_fixture() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(12);
        let gui = instrument.gui();
        (*gui).base.position_x = 64.0;
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        let before = FIXTURE_CREATIONS.load(Ordering::Relaxed);
        (*gui).base.position_x -= 0.00005;
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        let after = FIXTURE_CREATIONS.load(Ordering::Relaxed);
        restore_all();
        assert_eq!(after, before);
    }
}

#[test]
fn a_native_gui_owned_static_body_is_reused_then_restored() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(13);
        let gui = instrument.gui();
        create_body(gui.cast());
        game_fixture(gui.cast(), gui.cast());
        let original_body: Object = read_at(gui.cast(), B2_BODY);
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        assert_eq!(read_at::<Object>(gui.cast(), B2_BODY), original_body);
        restore_all();
        assert_eq!(read_at::<Object>(gui.cast(), B2_BODY), original_body);
        assert!(!read_at::<Object>(gui.cast(), B2_FIXTURE).is_null());
        assert_eq!(
            read_at::<Point>(original_body, 0xc),
            Point { x: 5.0, y: 10.0 }
        );
    }
}

#[test]
fn a_cleared_fixture_is_rebuilt_even_when_the_mesh_did_not_change() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(14);
        let gui = instrument.gui();
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        let body: Object = read_at(gui.cast(), B2_BODY);
        let fixture: Object = read_at(gui.cast(), B2_FIXTURE);
        destroy_fixture(body, fixture);
        write_at(gui.cast(), B2_FIXTURE, std::ptr::null_mut::<c_void>());
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        assert!(!read_at::<Object>(gui.cast(), B2_FIXTURE).is_null());
    }
}

#[test]
fn steady_gui_colliders_overlap_rounds_in_the_shaken_world_view() {
    let _lock = setup();
    unsafe {
        for gui_shake in [0_u8, 1] {
            let mut instrument = Instrument::new(15 + i64::from(gui_shake));
            let gui = instrument.gui();
            write_at(gui.cast(), 0x91e, gui_shake);
            assert!(may_draw(gui));
            step_world(std::ptr::null_mut(), 0.02, 10, 10);
            let body: Object = read_at(gui.cast(), B2_BODY);
            let metres: Point = read_at(body, 0xc);
            let round_screen = Point {
                x: (metres.x / 0.05 - CAMERA.x - SHAKE.x) * ZOOM + CENTRE.x,
                y: (metres.y / 0.05 - CAMERA.y - SHAKE.y) * ZOOM + CENTRE.y,
            };
            let gui_screen = Point {
                x: 100.0 + CENTRE.x - f32::from(gui_shake) * SHAKE.x,
                y: 200.0 + CENTRE.y - f32::from(gui_shake) * SHAKE.y,
            };
            assert!((round_screen.x - gui_screen.x).abs() < 0.001);
            assert!((round_screen.y - gui_screen.y).abs() < 0.001);
            restore_all();
        }
    }
}

#[test]
fn changing_the_serialized_node_id_does_not_orphan_gui_physics() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(17);
        let gui = instrument.gui();
        assert!(may_draw(gui));
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        (*gui).base.health = 3.0;
        (*gui).base.base.id = 18;
        step_world(std::ptr::null_mut(), 0.02, 10, 10);
        let managed = registered(gui.cast());
        let remaining = (*gui).base.health;
        restore_all();
        assert!(managed);
        assert_eq!(remaining, 3.0);
        assert!(read_at::<Object>(gui.cast(), B2_BODY).is_null());
    }
}

#[test]
fn deferred_contact_does_not_resurrect_a_gui_queued_for_deletion() {
    let _lock = setup();
    unsafe {
        let mut instrument = Instrument::new(19);
        let gui = instrument.gui();
        assert!(may_draw(gui));
        (*gui).base.base.state = 0;
        let mut round = Memory([0; 0xa40]);
        let shell = round.0.as_mut_ptr().cast();
        write_at(shell, 0x28, BODY_CODE);
        write_at(shell, 0x48, 2_i32);
        write_at(shell, 0x1c0, 0xd_u64);
        write_at(shell, 0x38c, -1.0_f32);
        let mut record = Memory([0; 0x48]);
        let record = record.0.as_mut_ptr().cast();
        write_at(record, 8, gui);
        write_at(record, 0x20, shell);
        write_at(record, 0x40, 100.0_f32);
        process_contact(record);
        assert_eq!((*gui).base.base.state, 0);
        assert_eq!((*gui).base.health, INITIAL_HEALTH);
    }
}
