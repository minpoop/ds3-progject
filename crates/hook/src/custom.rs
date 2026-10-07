//! Hand-written hook bodies named in the hooks sheet (`custom_fn`).
//!
//! Winsock hands out some functions (ConnectEx, WSASendMsg) through `WSAIoctl(SIO_GET_EXTENSION_FUNCTION_POINTER)`
//! instead of exporting them, so hooking the exports would miss them. `wsaioctl_guard` swaps the pointer it
//! returns for a guarded wrapper that refuses non-loopback destinations.

use crate::generated::{FnNetWsaioctl, ORIG_NET_WSAIOCTL};
use crate::guard;
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Guid {
    d1: u32,
    d2: u16,
    d3: u16,
    d4: [u8; 8],
}

const WSAID_CONNECTEX: Guid = Guid { d1: 0x25a2_07b9, d2: 0xddf3, d3: 0x4660, d4: [0x8e, 0xe9, 0x76, 0xe5, 0x8c, 0x74, 0x06, 0x3e] };
const WSAID_WSASENDMSG: Guid = Guid { d1: 0xa441_e712, d2: 0x754f, d3: 0x43ca, d4: [0x84, 0xa7, 0x0d, 0xee, 0x44, 0xcf, 0x60, 0x6d] };
const SIO_GET_EXTENSION_FUNCTION_POINTER: u32 = 0xC800_0006;

static REAL_CONNECTEX: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static REAL_WSASENDMSG: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

type ConnectExFn = unsafe extern "system" fn(usize, *const c_void, i32, *mut c_void, u32, *mut u32, *mut c_void) -> i32;
type WsaSendMsgFn = unsafe extern "system" fn(usize, *mut c_void, u32, *mut u32, *mut c_void, *mut c_void) -> i32;

unsafe extern "system" fn connectex_guard(s: usize, name: *const c_void, namelen: i32, send_buf: *mut c_void, send_len: u32, sent: *mut u32, ov: *mut c_void) -> i32 {
    if guard::addr_denied("connectex", name as *const u8) {
        guard::set_last_error_wsa(10061); // WSAECONNREFUSED
        return 0; // FALSE
    }
    let real: ConnectExFn = core::mem::transmute(REAL_CONNECTEX.load(Ordering::Relaxed));
    real(s, name, namelen, send_buf, send_len, sent, ov)
}

unsafe extern "system" fn sendmsg_guard(s: usize, msg: *mut c_void, flags: u32, sent: *mut u32, ov: *mut c_void, completion: *mut c_void) -> i32 {
    // WSAMSG begins with `LPSOCKADDR name`.
    let name = if msg.is_null() { core::ptr::null() } else { *(msg as *const *const u8) };
    if guard::addr_denied("wsasendmsg", name) {
        guard::set_last_error_wsa(10051); // WSAENETUNREACH
        return -1;
    }
    let real: WsaSendMsgFn = core::mem::transmute(REAL_WSASENDMSG.load(Ordering::Relaxed));
    real(s, msg, flags, sent, ov, completion)
}

/// Hook for `WSAIoctl` (hooks sheet: net_wsaioctl).
pub unsafe extern "system" fn wsaioctl_guard(
    s: usize,
    code: u32,
    in_buf: *mut c_void,
    cb_in: u32,
    out_buf: *mut c_void,
    cb_out: u32,
    bytes: *mut u32,
    ov: *mut c_void,
    completion: *mut c_void,
) -> i32 {
    let orig: FnNetWsaioctl = core::mem::transmute(ORIG_NET_WSAIOCTL.load(Ordering::Relaxed));
    let r = orig(s, code, in_buf, cb_in, out_buf, cb_out, bytes, ov, completion);
    if r == 0
        && code == SIO_GET_EXTENSION_FUNCTION_POINTER
        && !in_buf.is_null()
        && cb_in as usize == core::mem::size_of::<Guid>()
        && !out_buf.is_null()
        && cb_out as usize >= core::mem::size_of::<usize>()
    {
        let guid = core::ptr::read_unaligned(in_buf as *const Guid);
        let slot = out_buf as *mut *mut c_void;
        if guid == WSAID_CONNECTEX {
            REAL_CONNECTEX.store(*slot, Ordering::Relaxed);
            *slot = connectex_guard as *const () as usize as *mut c_void;
        } else if guid == WSAID_WSASENDMSG {
            REAL_WSASENDMSG.store(*slot, Ordering::Relaxed);
            *slot = sendmsg_guard as *const () as usize as *mut c_void;
        }
    }
    r
}
