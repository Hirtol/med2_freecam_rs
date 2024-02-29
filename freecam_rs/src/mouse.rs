use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rust_hooking_utils::patching::process::Window;
use windows::Win32::Foundation::{HMODULE, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PeekMessageW, SetWindowsHookExW, ShowCursor, UnhookWindowsHookEx, HHOOK, MOUSEHOOKSTRUCTEX, MSG,
    PM_REMOVE, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
};

pub struct MouseManager {
    scroll_pos: Arc<Mutex<i32>>,
    old_scroll_pos: i32,
    shutdown: std::sync::mpsc::SyncSender<()>,
}

impl MouseManager {
    /// Initialises a new Windows hook for low level mouse events and tracks the mouse's scroll.
    pub fn new(main_window: Window, module_handle: HMODULE, block_middle_mouse: bool) -> anyhow::Result<Self> {
        if STATE.get().is_some() {
            anyhow::bail!("Can't initialise multiple ScrollTrackers!");
        }

        let (send_shutdown, recv_shutdown) = std::sync::mpsc::sync_channel(1);
        let scroll_pos = Arc::new(Mutex::new(0));

        // Initialise listener
        let other_scroll = scroll_pos.clone();
        std::thread::spawn(move || {
            let hook = unsafe {
                SetWindowsHookExW(
                    windows::Win32::UI::WindowsAndMessaging::WH_MOUSE,
                    Some(mouse),
                    module_handle,
                    0,
                )
                .expect("Failed to set hook")
            };

            let (scroll_sender, scroll_recv) = std::sync::mpsc::channel();
            let state = MouseState {
                block_middle_mouse,
                main_window,
                scroll_sender,
                hide_cursor: AtomicU32::new(2),
                hook,
            };
            let _ = STATE.set(Box::new(state));

            let mut message: MSG = MSG::default();

            loop {
                unsafe { while PeekMessageW(&mut message, main_window.0, 0, 0, PM_REMOVE).as_bool() {} }

                while let Ok(scroll_delta) = scroll_recv.try_recv() {
                    *other_scroll.lock().unwrap() += scroll_delta;
                }

                if recv_shutdown.try_recv().is_ok() {
                    break;
                }

                // Probably not the best way of avoiding a spinning thread, but I don't know Win32 well enough :)
                // GetMessage seems to just block indefinitely.
                std::thread::sleep(Duration::from_millis(1));
            }
        });

        Ok(Self {
            scroll_pos,
            old_scroll_pos: 0,
            shutdown: send_shutdown,
        })
    }

    /// Return the current scroll position
    #[allow(unused)]
    pub fn get_scroll(&self) -> i32 {
        *self.scroll_pos.lock().unwrap()
    }

    /// Return how much the scrolling occurred since the last time this method was called.
    pub fn get_scroll_delta(&mut self) -> i32 {
        let new_pos = *self.scroll_pos.lock().unwrap();
        let delta = new_pos - self.old_scroll_pos;
        self.old_scroll_pos = new_pos;

        delta
    }

    pub fn reset_scroll(&self) {
        *self.scroll_pos.lock().unwrap() = 0;
    }

    /// Show the current game cursor.
    ///
    /// As `SetCursor` and `ShowCursor` seemingly only work on the thread that created the window the actual method call
    /// will be executed in the context of our MouseHook, so there is a slight delay.
    pub fn show_cursor(&self) {
        if let Some(state) = STATE.get() {
            state.show_cursor();
        }
    }

    /// Hide the current game cursor.
    ///
    /// As `SetCursor` and `ShowCursor` seemingly only work on the thread that created the window the actual method call
    /// will be executed in the context of our MouseHook, so there is a slight delay.
    pub fn hide_cursor(&self) {
        if let Some(state) = STATE.get() {
            state.hide_cursor();
        }
    }
}

impl Drop for MouseManager {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
        // Block to wait for the receiver to shutdown
        let _ = self.shutdown.send(());

        unsafe {
            if let Some(state) = STATE.get() {
                UnhookWindowsHookEx(state.hook).expect("Failed to unhook");
            }
        }
    }
}

static STATE: once_cell::race::OnceBox<MouseState> = once_cell::race::OnceBox::new();

pub struct MouseState {
    block_middle_mouse: bool,
    main_window: Window,
    scroll_sender: std::sync::mpsc::Sender<i32>,
    /// We use a `u32` here to allow us to represent 3 state transitions.
    /// Hide (0), Show (1), and everything else.
    hide_cursor: AtomicU32,
    hook: HHOOK,
}

impl MouseState {
    pub fn show_cursor(&self) {
        self.hide_cursor.store(1, Ordering::Relaxed);
    }

    pub fn hide_cursor(&self) {
        self.hide_cursor.store(0, Ordering::Relaxed);
    }
}

/// Non low-level hooks can be executed from any thread, so we can't use a thread-local.
///
/// This hook is also _extremely_ vulnerable to causing lag/blocking applications, so it should be as cheap as possible to execute.
unsafe extern "system" fn mouse(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code >= 0 {
        let Some(state) = STATE.get() else {
            return CallNextHookEx(None, n_code, w_param, l_param);
        };

        match w_param.0 as u32 {
            WM_MBUTTONDOWN | WM_MBUTTONUP => {
                let p_mouse = l_param.0 as *mut MOUSEHOOKSTRUCTEX;

                if state.block_middle_mouse
                    && (*p_mouse).Base.hwnd == state.main_window.0
                    && crate::battle_cam::data::is_in_battle()
                {
                    return LRESULT(1);
                }
            }
            WM_MOUSEWHEEL => {
                let p_mouse = l_param.0 as *mut MOUSEHOOKSTRUCTEX;
                let to_store = if (*p_mouse).mouseData >> 16 == 120 { 1 } else { -1 };

                if (*p_mouse).Base.hwnd == state.main_window.0 {
                    let _ = state.scroll_sender.send(to_store);
                }
            }
            WM_MOUSEMOVE => {
                // We need to call the `ShowCursor` routines in the context of the right thread (as far as I can tell, documentation
                // is rather sparse on _where_ exactly you're supposed to call these functions. It doesn't work when called from our DLL threads, at least).
                // The mousemove event is incredibly common, so a decent place to ensure the cursor is hidden quickly.
                let cursor_value = state.hide_cursor.load(Ordering::Relaxed);
                match cursor_value {
                    0 => {
                        ShowCursor(false);
                        state.hide_cursor.store(2, Ordering::Relaxed);
                    }
                    1 => {
                        ShowCursor(true);
                        state.hide_cursor.store(2, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}
