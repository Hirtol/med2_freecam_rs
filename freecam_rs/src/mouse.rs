use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PeekMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, MSG, MSLLHOOKSTRUCT, PM_REMOVE,
    WM_MOUSEWHEEL,
};

pub struct ScrollTracker {
    scroll_pos: Arc<Mutex<i32>>,
    old_scroll_pos: i32,
    shutdown: std::sync::mpsc::SyncSender<()>,
}

impl ScrollTracker {
    /// Initialises a new Windows hook for low level mouse events and tracks the mouse's scroll.
    pub fn new() -> anyhow::Result<Self> {
        if STATE.with_borrow(|r| r.is_some()) {
            anyhow::bail!("Can't initialise multiple ScrollTrackers!");
        }

        let (send_shutdown, recv_shutdown) = std::sync::mpsc::sync_channel(1);
        let scroll_pos = Arc::new(Mutex::new(0));

        // Initialise listener
        let other_scroll = scroll_pos.clone();
        std::thread::spawn(move || {
            let hook = unsafe {
                SetWindowsHookExW(
                    windows::Win32::UI::WindowsAndMessaging::WH_MOUSE_LL,
                    Some(mouse),
                    None,
                    0,
                )
                .expect("Failed to set hook")
            };

            STATE.set(Some(MouseState { hook, last_delta: 0 }));

            let mut message: MSG = MSG::default();

            loop {
                unsafe {
                    PeekMessageW(&mut message, None, 0, 0, PM_REMOVE);
                }

                STATE.with_borrow_mut(|data| {
                    if let Some(data) = data {
                        if data.last_delta != 0 {
                            *other_scroll.lock().unwrap() += data.last_delta;
                            data.last_delta = 0;
                        }
                    }
                });

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
}

impl Drop for ScrollTracker {
    fn drop(&mut self) {
        let _ = self.shutdown.send(());
        // Block to wait for the receiver to shutdown
        let _ = self.shutdown.send(());

        unsafe {
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_ref() {
                    UnhookWindowsHookEx(state.hook).expect("Failed to unhook");
                }
                let _ = state.replace(None);
            });
        }
    }
}

thread_local! {
    static STATE: RefCell<Option<MouseState>> = RefCell::new(None);
}

pub struct MouseState {
    hook: HHOOK,
    last_delta: i32,
}

unsafe extern "system" fn mouse(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code >= 0 && w_param.0 == WM_MOUSEWHEEL as usize {
        let p_mouse = l_param.0 as *mut MSLLHOOKSTRUCT;
        let to_store = if (*p_mouse).mouseData >> 16 == 120 { 1 } else { -1 };

        STATE.with_borrow_mut(|state| {
            if let Some(state) = state {
                state.last_delta = to_store
            }
        });
    }

    CallNextHookEx(
        STATE
            .with_borrow(|state| state.as_ref().map(|s| s.hook))
            .expect("No STATE!"),
        n_code,
        w_param,
        l_param,
    )
}
