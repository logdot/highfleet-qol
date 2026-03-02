//! Lightweight xorshift64* pseudo-random number generator.
//!
//! Uses an [`AtomicU64`] for lock-free state, seeded once from the system clock.
//! Not cryptographically secure — perfectly fine for gameplay RNG such as shop
//! part rolls.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Global xorshift64 RNG state, seeded once at init time.
static RNG_STATE: AtomicU64 = AtomicU64::new(0);

/// Seeds the global RNG from the current system time.
pub fn seed() {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xdeadbeefcafe1234);
    // xorshift doesn't like an all-zero state
    let seed = if seed == 0 { 0xdeadbeefcafe1234 } else { seed };
    RNG_STATE.store(seed, Ordering::Relaxed);
}

/// Returns the next pseudo-random `u64` using xorshift64*.
///
/// Uses a compare-exchange loop on the atomic state so concurrent calls
/// (unlikely but possible) never reuse the same state.
pub fn next_u64() -> u64 {
    loop {
        let old = RNG_STATE.load(Ordering::Relaxed);
        let mut s = old;
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        // xorshift64* mixes with a constant for better low-bit quality
        let result = s.wrapping_mul(0x2545F4914F6CDD1D);
        if RNG_STATE
            .compare_exchange(old, s, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            return result;
        }
    }
}

/// Returns a random `f32` in `[0.0, 1.0)`.
pub fn random_f32() -> f32 {
    // Use the upper 24 bits for a uniform float in [0, 1).
    (next_u64() >> 40) as f32 / (1u64 << 24) as f32
}

/// Returns a random `u32` in `[min, max]` (inclusive on both ends).
///
/// If `min >= max`, returns `min`.
pub fn random_range(min: u32, max: u32) -> u32 {
    if min >= max {
        return min;
    }
    let range = (max - min) as u64 + 1;
    min + (next_u64() % range) as u32
}
