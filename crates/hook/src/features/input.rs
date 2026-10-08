//! Which attack buttons are down right now? Dark Souls III gives no "attack started" event to listen to, so the sound
//! feature tells an attack from a roll by whether an attack button went down just before stamina fell. This only READS
//! the controller (XInput) and the mouse (GetAsyncKeyState); it never sends or blocks any input.
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

pub const VK_LBUTTON: i32 = 0x01;
pub const VK_RBUTTON: i32 = 0x02;
/// Shift is the "strong attack" modifier of the game's default keyboard layout (Shift + left mouse button).
pub const VK_SHIFT: i32 = 0x10;
pub const VK_F5: i32 = 0x74;
pub const VK_F6: i32 = 0x75;
pub const VK_F7: i32 = 0x76;
pub const VK_F8: i32 = 0x77;

// XINPUT_GAMEPAD button bits
const XI_LEFT_SHOULDER: u16 = 0x0100;
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

/// What the player is pressing, as far as attacks are concerned. The default layouts of Dark Souls III: gamepad RB / RT are
/// the right-hand weapon's normal / strong attack and LB the left-hand weapon's; on keyboard and mouse the left button is
/// the right-hand attack (Shift + left button the strong one) and the right button the left-hand weapon's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buttons {
    pub right_light: bool,
    pub right_strong: bool,
    pub left_light: bool,
    /// raw values for the trace log (so a wrong guess about the bindings can be corrected from a real session)
    pub shift: bool,
    pub pad_buttons: u16,
    pub pad_triggers: (u8, u8),
    pub mouse: (bool, bool),
}

impl Buttons {
    /// The gamepad part: shoulder buttons and triggers.
    pub fn from_pad(pad_buttons: u16, left_trigger: u8, right_trigger: u8) -> Buttons {
        Buttons {
            right_light: pad_buttons & XI_RIGHT_SHOULDER != 0,
            right_strong: right_trigger >= TRIGGER_DOWN,
            left_light: pad_buttons & XI_LEFT_SHOULDER != 0,
            pad_buttons,
            pad_triggers: (left_trigger, right_trigger),
            ..Buttons::default()
        }
    }

    /// Add the mouse (only while the game window is in front): left button = right-hand attack, strong with Shift held;
    /// right button = left-hand weapon.
    pub fn with_mouse(mut self, left: bool, right: bool, shift: bool) -> Buttons {
        self.mouse = (left, right);
        self.shift = shift;
        if left {
            if shift {
                self.right_strong = true;
            } else {
                self.right_light = true;
            }
        }
        self.left_light |= right;
        self
    }
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
                    b = Buttons::from_pad(st.gamepad.buttons, st.gamepad.left_trigger, st.gamepad.right_trigger);
                    break;
                }
            }
        }
        if game_has_focus() {
            b = b.with_mouse(key_down(VK_LBUTTON), key_down(VK_RBUTTON), key_down(VK_SHIFT));
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
        assert!(!b.right_light && !b.right_strong && !b.left_light);
        assert!(!key_down(0x7E)); // F15
        let _ = i.has_gamepad_support();
    }

    #[test]
    fn the_default_gamepad_layout_maps_to_hands() {
        assert_eq!(Buttons::from_pad(0, 0, 0), Buttons::default());
        let rb = Buttons::from_pad(XI_RIGHT_SHOULDER, 0, 0);
        assert!(rb.right_light && !rb.right_strong && !rb.left_light);
        let lb = Buttons::from_pad(XI_LEFT_SHOULDER, 0, 0);
        assert!(lb.left_light && !lb.right_light);
        let rt = Buttons::from_pad(0, 0, 200);
        assert!(rt.right_strong && !rt.right_light);
        assert!(!Buttons::from_pad(0, 255, 10).right_strong, "a light touch of the trigger and the left trigger are not attacks");
        assert_eq!(rt.pad_triggers, (0, 200));
    }

    #[test]
    fn the_default_keyboard_layout_maps_to_hands() {
        let none = Buttons::default();
        let l = none.with_mouse(true, false, false);
        assert!(l.right_light && !l.right_strong && !l.left_light);
        let shift_l = none.with_mouse(true, false, true);
        assert!(shift_l.right_strong && !shift_l.right_light, "Shift + left button is the strong attack");
        let r = none.with_mouse(false, true, false);
        assert!(r.left_light && !r.right_light, "the right button is the left-hand weapon");
        let shift_only = none.with_mouse(false, false, true);
        assert!(!shift_only.right_strong && shift_only.shift, "Shift alone is not an attack");
        // a controller and the mouse can both be in use
        let both = Buttons::from_pad(XI_LEFT_SHOULDER, 0, 0).with_mouse(true, false, false);
        assert!(both.left_light && both.right_light);
    }

    #[test]
    fn xinput_structures_have_the_sizes_windows_expects() {
        assert_eq!(core::mem::size_of::<XInputGamepad>(), 12);
        assert_eq!(core::mem::size_of::<XInputState>(), 16);
    }
}
