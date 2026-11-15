use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::audio::AudioEngine;
use crate::models::SoundItem;
use crate::server::RemoteServer;

#[derive(Clone)]
pub struct HotkeyManager {
    enabled: Arc<AtomicBool>,
    app_focused: Arc<AtomicBool>,
}

enum HotkeyAction {
    Trigger {
        sound_id: String,
        path: String,
        volume: f32,
        play_mode: String,
    },
    ReleaseHold {
        sound_id: String,
    },
}

struct HookState {
    sounds_provider: Arc<Mutex<Vec<SoundItem>>>,
    enabled: Arc<AtomicBool>,
    action_tx: Sender<HotkeyAction>,
    active_holds: Mutex<HashSet<String>>,
}

static HOOK_STATE: OnceLock<Arc<HookState>> = OnceLock::new();
static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);

fn parse_vk(main_part: &str) -> Option<u32> {
    let s = main_part.to_uppercase();
    let vk = match s.as_str() {
        // Letters A-Z
        s if s.len() == 1 && s.chars().next()?.is_ascii_alphabetic() => {
            s.chars().next()?.to_ascii_uppercase() as u32
        }
        // Digits 0-9
        s if s.len() == 1 && s.chars().next()?.is_ascii_digit() => {
            s.chars().next()? as u32
        }
        // Function Keys F1 - F24
        s if s.starts_with('F') && s[1..].parse::<u32>().is_ok() => {
            let num: u32 = s[1..].parse().ok()?;
            if (1..=24).contains(&num) {
                VK_F1 as u32 + (num - 1)
            } else {
                return None;
            }
        }
        // Numpad 0-9
        "NUM0" | "NUMPAD0" => VK_NUMPAD0 as u32,
        "NUM1" | "NUMPAD1" => VK_NUMPAD1 as u32,
        "NUM2" | "NUMPAD2" => VK_NUMPAD2 as u32,
        "NUM3" | "NUMPAD3" => VK_NUMPAD3 as u32,
        "NUM4" | "NUMPAD4" => VK_NUMPAD4 as u32,
        "NUM5" | "NUMPAD5" => VK_NUMPAD5 as u32,
        "NUM6" | "NUMPAD6" => VK_NUMPAD6 as u32,
        "NUM7" | "NUMPAD7" => VK_NUMPAD7 as u32,
        "NUM8" | "NUMPAD8" => VK_NUMPAD8 as u32,
        "NUM9" | "NUMPAD9" => VK_NUMPAD9 as u32,

        // Common keys
        "SPACE" => VK_SPACE as u32,
        "ENTER" | "RETURN" => VK_RETURN as u32,
        "TAB" => VK_TAB as u32,
        "ESCAPE" | "ESC" => VK_ESCAPE as u32,
        "BACKSPACE" | "BACK" => VK_BACK as u32,
        "INSERT" => VK_INSERT as u32,
        "DELETE" | "DEL" => VK_DELETE as u32,
        "HOME" => VK_HOME as u32,
        "END" => VK_END as u32,
        "PAGEUP" | "PGUP" => VK_PRIOR as u32,
        "PAGEDOWN" | "PGDN" => VK_NEXT as u32,
        "UP" => VK_UP as u32,
        "DOWN" => VK_DOWN as u32,
        "LEFT" => VK_LEFT as u32,
        "RIGHT" => VK_RIGHT as u32,
        "+" | "PLUS" | "=" => VK_OEM_PLUS as u32,
        "-" | "MINUS" => VK_OEM_MINUS as u32,
        "[" => VK_OEM_4 as u32,
        "]" => VK_OEM_6 as u32,
        ";" => VK_OEM_1 as u32,
        "'" => VK_OEM_7 as u32,
        "/" => VK_OEM_2 as u32,
        "\\" => VK_OEM_5 as u32,
        "," => VK_OEM_COMMA as u32,
        "." => VK_OEM_PERIOD as u32,
        "`" | "~" | "TILDE" => VK_OEM_3 as u32,
        _ => return None,
    };
    Some(vk)
}

fn parse_hotkey_string(hotkey: &str) -> Option<(u32, bool, bool, bool, bool)> {
    let trimmed = hotkey.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut win = false;

    let parts: Vec<&str> = trimmed
        .split('+')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if parts.is_empty() {
        if trimmed == "+" {
            return Some((parse_vk("+")?, false, false, false, false));
        }
        return None;
    }

    let (mod_parts, main_key) = if trimmed.ends_with('+') && parts.len() >= 1 {
        (&parts[..], "+")
    } else {
        (&parts[..parts.len() - 1], *parts.last()?)
    };

    for m in mod_parts {
        match m.to_uppercase().as_str() {
            "CTRL" | "CONTROL" => ctrl = true,
            "ALT" | "OPTION" => alt = true,
            "SHIFT" => shift = true,
            "WIN" | "SUPER" | "META" => win = true,
            _ => {}
        }
    }

    let vk = parse_vk(main_key)?;
    Some((vk, ctrl, alt, shift, win))
}

fn get_modifier_state(kbd_flags: u32) -> (bool, bool, bool, bool) {
    unsafe {
        // Ctrl: check generic, left, and right
        let ctrl = ((GetAsyncKeyState(VK_CONTROL as i32) as u16 & 0x8000) != 0)
            || ((GetAsyncKeyState(VK_LCONTROL as i32) as u16 & 0x8000) != 0)
            || ((GetAsyncKeyState(VK_RCONTROL as i32) as u16 & 0x8000) != 0);

        // Alt: check hardware LLKHF_ALTDOWN flag (0x20) or GetAsyncKeyState
        let alt = ((kbd_flags & 0x20) != 0)
            || ((GetAsyncKeyState(VK_MENU as i32) as u16 & 0x8000) != 0)
            || ((GetAsyncKeyState(VK_LMENU as i32) as u16 & 0x8000) != 0)
            || ((GetAsyncKeyState(VK_RMENU as i32) as u16 & 0x8000) != 0);

        // Shift: check generic, left, and right
        let shift = ((GetAsyncKeyState(VK_SHIFT as i32) as u16 & 0x8000) != 0)
            || ((GetAsyncKeyState(VK_LSHIFT as i32) as u16 & 0x8000) != 0)
            || ((GetAsyncKeyState(VK_RSHIFT as i32) as u16 & 0x8000) != 0);

        // Win: check left and right Windows keys
        let win = ((GetAsyncKeyState(VK_LWIN as i32) as u16 & 0x8000) != 0)
            || ((GetAsyncKeyState(VK_RWIN as i32) as u16 & 0x8000) != 0);

        (ctrl, alt, shift, win)
    }
}

unsafe extern "system" fn low_level_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        if let Some(state) = HOOK_STATE.get() {
            if state.enabled.load(Ordering::Relaxed) {
                let kbd = unsafe { *(l_param as *const KBDLLHOOKSTRUCT) };
                let vk = kbd.vkCode;
                let msg = w_param as u32;

                if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                    let is_modifier_key = matches!(
                        vk as u16,
                        VK_CONTROL | VK_LCONTROL | VK_RCONTROL
                            | VK_MENU | VK_LMENU | VK_RMENU
                            | VK_SHIFT | VK_LSHIFT | VK_RSHIFT
                            | VK_LWIN | VK_RWIN
                    );

                    if !is_modifier_key {
                        // Check if our app window is the active foreground window
                        let foreground = unsafe { GetForegroundWindow() };
                        let mut foreground_pid: u32 = 0;
                        unsafe { GetWindowThreadProcessId(foreground, &mut foreground_pid) };
                        let is_our_app_focused = foreground_pid == std::process::id();

                        // If outside our app (in games, fullscreen, desktop, other apps), this global hook handles it!
                        // If inside our app, React DOM keydown handler in App.tsx handles it with zero latency.
                        if !is_our_app_focused {
                            let (ctrl, alt, shift, win) = get_modifier_state(kbd.flags);

                            let sounds = {
                                let guard = state.sounds_provider.lock().unwrap();
                                guard.clone()
                            };

                            for s in sounds {
                                if let Some(ref h_str) = s.hotkey {
                                    if let Some((target_vk, req_ctrl, req_alt, req_shift, req_win)) =
                                        parse_hotkey_string(h_str)
                                    {
                                        if vk == target_vk
                                            && ctrl == req_ctrl
                                            && alt == req_alt
                                            && shift == req_shift
                                            && win == req_win
                                        {
                                            if s.play_mode == "hold" {
                                                let mut holds = state.active_holds.lock().unwrap();
                                                if holds.insert(s.id.clone()) {
                                                    let _ = state.action_tx.send(HotkeyAction::Trigger {
                                                        sound_id: s.id.clone(),
                                                        path: s.path.clone(),
                                                        volume: s.volume,
                                                        play_mode: s.play_mode.clone(),
                                                    });
                                                }
                                            } else {
                                                // Zero debounce! Trigger immediately on every press so user can spam keys!
                                                let _ = state.action_tx.send(HotkeyAction::Trigger {
                                                    sound_id: s.id.clone(),
                                                    path: s.path.clone(),
                                                    volume: s.volume,
                                                    play_mode: s.play_mode.clone(),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if msg == WM_KEYUP || msg == WM_SYSKEYUP {
                    let mut holds = state.active_holds.lock().unwrap();
                    if !holds.is_empty() {
                        let sounds = {
                            let guard = state.sounds_provider.lock().unwrap();
                            guard.clone()
                        };

                        for s in sounds {
                            if holds.contains(&s.id) {
                                if let Some(ref h_str) = s.hotkey {
                                    if let Some((target_vk, _, _, _, _)) = parse_hotkey_string(h_str) {
                                        if vk == target_vk {
                                            holds.remove(&s.id);
                                            let _ = state.action_tx.send(HotkeyAction::ReleaseHold {
                                                sound_id: s.id.clone(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Always pass through 100% to game/active window
    unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) }
}

impl HotkeyManager {
    pub fn new(
        sounds: Arc<Mutex<Vec<SoundItem>>>,
        audio_engine: AudioEngine,
        server: RemoteServer,
    ) -> Self {
        let enabled = Arc::new(AtomicBool::new(true));
        let app_focused = Arc::new(AtomicBool::new(false));
        let (action_tx, action_rx) = channel::<HotkeyAction>();

        // Dedicated fast action execution worker thread
        let engine_clone = audio_engine.clone();
        let server_clone = server.clone();
        thread::spawn(move || {
            while let Ok(action) = action_rx.recv() {
                match action {
                    HotkeyAction::Trigger {
                        sound_id,
                        path,
                        volume,
                        play_mode,
                    } => {
                        if play_mode == "toggle" {
                            if engine_clone.is_playing(&sound_id) {
                                engine_clone.stop(&sound_id);
                                server_clone.broadcast_audio_state(&sound_id, false);
                            } else {
                                let _ = engine_clone.play(&sound_id, &path, volume, "toggle");
                                server_clone.broadcast_audio_state(&sound_id, true);
                            }
                        } else if play_mode == "hold" {
                            let _ = engine_clone.play(&sound_id, &path, volume, "hold");
                            server_clone.broadcast_audio_state(&sound_id, true);
                        } else {
                            let _ = engine_clone.play(&sound_id, &path, volume, &play_mode);
                            server_clone.broadcast_audio_state(&sound_id, true);
                        }
                    }
                    HotkeyAction::ReleaseHold { sound_id } => {
                        engine_clone.stop(&sound_id);
                        server_clone.broadcast_audio_state(&sound_id, false);
                    }
                }
            }
        });

        let hook_state = Arc::new(HookState {
            sounds_provider: sounds,
            enabled: enabled.clone(),
            action_tx,
            active_holds: Mutex::new(HashSet::new()),
        });

        let _ = HOOK_STATE.set(hook_state);

        // Spawn dedicated high-priority global hook thread with message loop
        thread::spawn(move || unsafe {
            // Elevate thread priority so game CPU spikes don't starve the hook
            let current_thread = GetCurrentThread();
            SetThreadPriority(current_thread, THREAD_PRIORITY_HIGHEST as i32);

            let h_mod = GetModuleHandleW(std::ptr::null());
            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(low_level_keyboard_proc),
                h_mod,
                0,
            );

            if hook.is_null() {
                eprintln!("[OpenSB Hotkey] Failed to install low-level keyboard hook");
                return;
            }

            HOOK_HANDLE.store(hook as isize, Ordering::SeqCst);
            eprintln!("[OpenSB Hotkey] High-priority global keyboard hook installed successfully");

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let prev_h = HOOK_HANDLE.swap(0, Ordering::SeqCst);
            if prev_h != 0 {
                UnhookWindowsHookEx(prev_h as HHOOK);
            }
            eprintln!("[OpenSB Hotkey] Global keyboard hook thread exiting");
        });

        Self {
            enabled,
            app_focused,
        }
    }

    pub fn set_enabled(&self, val: bool) {
        self.enabled.store(val, Ordering::Relaxed);
    }

    pub fn set_app_focused(&self, val: bool) {
        self.app_focused.store(val, Ordering::Relaxed);
    }
}
