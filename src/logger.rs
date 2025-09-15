use log::{LevelFilter, Log};

#[no_mangle]
pub fn setup_logger(logger: &'static dyn Log, level: LevelFilter) {
    log::set_logger(logger).unwrap();
    log::set_max_level(level);
}
