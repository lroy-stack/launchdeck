//! Windows: a WinEvent hook says when the foreground window changes.
//!
//! `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` out of context delivers its
//! callback through the message loop of the thread that installed it, so this
//! observer is one thread sitting in `GetMessageW`. On each event it asks
//! which process owns the foreground window and takes the file name of its
//! executable. The window's title is never read.
//!
//! A thread timer every two seconds stands behind the hook: restoring a
//! minimised window does not always raise the event.
//!
//! What it cannot tell apart: packaged (Store) applications, which all run
//! inside `ApplicationFrameHost.exe`. What it cannot see: a process running
//! elevated while LaunchDeck is not (it is reported as nothing in front).
//!
//! Written; never compiled (no Windows machine; CI blocked by account
//! billing). It is written against windows-sys 0.61.2.

use std::ptr::null_mut;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, SetTimer, EVENT_SYSTEM_FOREGROUND,
    MSG, WINEVENT_OUTOFCONTEXT, WM_TIMER,
};

use super::{ForegroundObserver, ObserverStatus, Seen, Sink};
use crate::context::identity::from_executable_path;

const SAFETY_NET_MS: u32 = 2000;

/// The hook's callback has no argument of its own to carry this.
static SINK: OnceLock<Sink> = OnceLock::new();

#[derive(Default)]
pub struct WindowsObserver;

impl WindowsObserver {
    pub fn new() -> WindowsObserver {
        WindowsObserver
    }
}

/// The application that owns the foreground window, by its executable.
fn foreground() -> Seen {
    // SAFETY: plain Win32 calls with buffers of the sizes they are told, and
    // the process handle is closed on every path that opened it.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return Seen::Nothing;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, &mut pid);
        if pid == 0 {
            return Seen::Nothing;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            // Elevated or protected: it cannot be asked what it is.
            return Seen::Nothing;
        }
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(process);
        if ok == 0 {
            return Seen::Nothing;
        }
        let path = String::from_utf16_lossy(&buffer[..(length as usize).min(buffer.len())]);
        match from_executable_path(&path) {
            Some(identity) => Seen::Application {
                identity,
                own: pid == std::process::id(),
            },
            None => Seen::Nothing,
        }
    }
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _window: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if let Some(sink) = SINK.get() {
        sink.seen(foreground());
    }
}

impl ForegroundObserver for WindowsObserver {
    fn status(&self) -> ObserverStatus {
        ObserverStatus::available("Windows foreground events")
    }

    fn run(self: Box<Self>, sink: Sink) {
        let sink = SINK.get_or_init(|| sink);
        sink.seen(foreground());
        // SAFETY: the callback is a plain function that lives as long as the
        // program; the hook and the timer belong to this thread, which then
        // runs the message loop that delivers both.
        unsafe {
            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                null_mut(),
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            if hook.is_null() {
                sink.status(ObserverStatus::Degraded {
                    mechanism: "Windows foreground events".into(),
                    reason: "Windows did not accept the foreground hook; the application in front is checked every two seconds instead.".into(),
                });
            }
            SetTimer(null_mut(), 0, SAFETY_NET_MS, None);
            let mut message = MSG::default();
            while !sink.stopped() && GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                if message.message == WM_TIMER {
                    sink.seen(foreground());
                }
            }
        }
    }
}
