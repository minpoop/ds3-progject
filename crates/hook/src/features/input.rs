//! Which attack buttons are down right now? Dark Souls III gives no "attack started" event to listen to, so the sound
//! feature tells an attack from a roll by whether an attack button went down just before stamina fell. This only READS
//! the controller (XInput) and the mouse (GetAsyncKeyState); it never sends or blocks any input.
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

pub const VK_LBUTTON: i32 = 0x01;
pub const VK_RBUTTON: i32 = 0x02;
pub const VK_F7: i32 = 0x76;
pub const VK_F8: i32 = 0x77;

// XINPUT_GAMEPAD button bits
const XI_RIGHT_SHOULDER: u16 = 0x0200;
/// Trigger values run 0..=255; the game's own dead zone is around 30.
const TRIGGER_DOWN: u8 = 40;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct XInputGamepad {
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    thumb_lx: i16,
    thumb_ly: i16,
    thumb_rx: i16,
    thumb_ry: i16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct XInputState {
    packet: u32,
    gamepad: XInputGamepad,
}

type XInputGetState = unsafe extern "system" fn(u32, *mut XInputState) -> u32;

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(vkey: i32) -> i16;
}

/// What the player is pressing, as far as swings are concerned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buttons {
    /// right shoulder button, or the left mouse button
    pub light: bool,
    /// right trigger, or the right mouse button
    pub strong: bool,
    /// raw values for the trace log (so a wrong guess about the bindings can be corrected from a real session)
    pub pad_buttons: u16,
    pub pad_triggers: (u8, u8),
    pub mouse: (bool, bool),
}

pub struct Input {
    xinput: Option<XInputGetState>,
}

impl Input {
    pub fn new() -> Input {
        Input { xinput: load_xinput() }
    }

    pub fn has_gamepad_support(&self) -> bool {
        self.xinput.is_some()
    }

    /// Poll the first connected controller and the mouse. The mouse only counts while the game window has the focus.
    pub fn buttons(&self) -> Buttons {
        let mut b = Buttons::default();
        if let Some(get) = self.xinput {
            for pad in 0..4u32 {
                let mut st = XInputState::default();
                if unsafe { get(pad, &mut st) } == 0 {
                    b.pad_buttons = st.gamepad.buttons;
                    b.pad_triggers = (st.gamepad.left_trigger, st.gamepad.right_trigger);
                    b.light = st.gamepad.buttons & XI_RIGHT_SHOULDER != 0;
                    b.strong = st.gamepad.right_trigger >= TRIGGER_DOWN;
                    break;
                }
            }
        }
        if game_has_focus() {
            let (l, r) = (key_down(VK_LBUTTON), key_down(VK_RBUTTON));
            b.mouse = (l, r);
            b.light |= l;
            b.strong |= r;
        }
        b
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

/// Is the key (or mouse button) down right now? (The "was pressed since the last call" bit is not used.)
pub fn key_down(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16) & 0x8000 != 0 }
}

/// Does the window in front belong to this process? Keys pressed in another program must not count.
pub fn game_has_focus() -> bool {
    unsafe {
        let w = GetForegroundWindow();
        if w.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(w, &mut pid);
        pid == GetCurrentProcessId()
    }
}

fn load_xinput() -> Option<XInputGetState> {
    // newest first; xinput9_1_0 ships with every Windows since Vista
    for dll in [&b"xinput1_4.dll\0"[..], b"xinput1_3.dll\0", b"xinput9_1_0.dll\0"] {
        unsafe {
            let m: HMODULE = LoadLibraryA(dll.as_ptr());
            if m.is_null() {
                continue;
            }
            if let Some(f) = GetProcAddress(m, b"XInputGetState\0".as_ptr()) {
                return Some(core::mem::transmute::<unsafe extern "system" fn() -> isize, XInputGetState>(f));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_without_any_device_is_quiet_and_never_crashes() {
        let i = Input::new();
        // no controller, no focus: nothing is pressed
        let b = i.buttons();
        assert!(!b.light && !b.strong);
        assert!(!key_down(0x7E)); // F15
        let _ = i.has_gamepad_support();
    }

    #[test]
    fn xinput_structures_have_the_sizes_windows_expects() {
        assert_eq!(core::mem::size_of::<XInputGamepad>(), 12);
        assert_eq!(core::mem::size_of::<XInputState>(), 16);
    }
}
