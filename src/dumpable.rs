#[cfg(feature = "1_151")]
use highfleet::v1_151::Body;
#[cfg(not(feature = "1_151"))]
use highfleet::v1_163::Body;
use patchy::{CallBridge, FlagPolicy, KeepReturn, PatchSession, Result, StackAlignment};

use crate::replay_call::replay_then_call;

/// `MOV RCX, RSI`.
const PASS_RSI: [u8; 3] = [0x48, 0x89, 0xf1];

pub unsafe fn dumpable(session: &mut PatchSession) -> Result {
    let address;
    if cfg!(feature = "1_151") {
        address = 0x1400240c0;
    } else if cfg!(feature = "1_163") {
        address = 0x1400256e0;
    } else {
        // Default to 1.163
        address = 0x1400256e0;
    }

    replay_then_call(
        session,
        address,
        6,
        CallBridge {
            callback: set_dumpable as *const (),
            incoming_stack: StackAlignment::Aligned16,
            flags: FlagPolicy::Preserve,
            keep_return: KeepReturn::Neither,
            before_call: &PASS_RSI,
            after_call: &[],
        },
    )?;
    Ok(())
}

unsafe extern "C" fn set_dumpable(body: *mut Body) {
    #[cfg(feature = "1_151")]
    let flag = &raw mut (*body).unknown_08e6h;
    #[cfg(not(feature = "1_151"))]
    let flag = &raw mut (*body).unknown_091eh;
    flag.write([0]);
}
