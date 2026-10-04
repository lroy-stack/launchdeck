//! X11: the window manager says which window is active.
//!
//! A window manager that follows the EWMH convention keeps the active window
//! in the `_NET_ACTIVE_WINDOW` property of the root window, and the X server
//! sends a `PropertyNotify` whenever that changes. So nothing is polled: the
//! thread sleeps in the X connection until the active window changes, then
//! reads two properties of that window: `WM_CLASS` (which application) and
//! `_NET_WM_PID` (to recognise LaunchDeck itself). The title is never read.
//!
//! Wayland is not this: see [`native`].

use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, Window,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use super::{ForegroundObserver, NoObserver, ObserverStatus, Seen, Sink};
use crate::context::identity::from_wm_class;

const MECHANISM: &str = "the X11 window manager";

pub struct X11Observer {
    conn: RustConnection,
    root: Window,
    active: Atom,
    pid: Atom,
}

/// Whether this session is a Wayland one. There, an X connection (XWayland)
/// would only ever see other X clients and report every native Wayland window
/// as "nothing in front", which is a wrong answer, not a limited one.
pub fn wayland_session(session_type: Option<&str>, wayland_display: Option<&str>) -> bool {
    session_type.is_some_and(|t| t.eq_ignore_ascii_case("wayland"))
        || wayland_display.is_some_and(|d| !d.is_empty())
}

pub fn wayland_reason(desktop: Option<&str>) -> String {
    let which = match desktop.map(str::trim).filter(|d| !d.is_empty()) {
        Some(desktop) => format!("This is a Wayland session ({desktop})."),
        None => "This is a Wayland session.".to_string(),
    };
    format!("{which} Wayland does not let an application see which application is in front, and LaunchDeck has no compositor-specific support yet. Auto Context needs an X11 session; switching surfaces by hand works as always.")
}

/// The observer for this Linux (or other Unix) session.
pub fn native() -> Box<dyn ForegroundObserver> {
    let var = |name: &str| std::env::var(name).ok();
    if wayland_session(
        var("XDG_SESSION_TYPE").as_deref(),
        var("WAYLAND_DISPLAY").as_deref(),
    ) {
        return Box::new(NoObserver(wayland_reason(
            var("XDG_CURRENT_DESKTOP").as_deref(),
        )));
    }
    match X11Observer::connect() {
        Ok(observer) => Box::new(observer),
        Err(reason) => Box::new(NoObserver(reason)),
    }
}

impl X11Observer {
    /// Connect to the display in `DISPLAY` and check that its window manager
    /// reports the active window. The error is a sentence for a person.
    pub fn connect() -> Result<X11Observer, String> {
        let (conn, screen) = x11rb::connect(None)
            .map_err(|e| format!("LaunchDeck could not connect to an X display ({e}), so it cannot tell which application is in front."))?;
        let root = conn
            .setup()
            .roots
            .get(screen)
            .map(|s| s.root)
            .ok_or("The X display has no screen.")?;
        let atom = |name: &[u8]| -> Result<Atom, String> {
            Ok(conn
                .intern_atom(false, name)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?
                .atom)
        };
        let (active, supported, pid) = (
            atom(b"_NET_ACTIVE_WINDOW")?,
            atom(b"_NET_SUPPORTED")?,
            atom(b"_NET_WM_PID")?,
        );
        let offered = conn
            .get_property(false, root, supported, AtomEnum::ATOM, 0, 4096)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let reports_active = offered
            .value32()
            .is_some_and(|mut atoms| atoms.any(|a| a == active));
        if !reports_active {
            return Err("This X session has no window manager that reports the active window (EWMH `_NET_ACTIVE_WINDOW`), so LaunchDeck cannot tell which application is in front.".into());
        }
        Ok(X11Observer {
            conn,
            root,
            active,
            pid,
        })
    }

    fn property(
        &self,
        window: Window,
        property: impl Into<Atom>,
        long_length: u32,
    ) -> Result<Option<Vec<u8>>, ReplyError> {
        match self
            .conn
            .get_property(false, window, property, AtomEnum::ANY, 0, long_length)?
            .reply()
        {
            Ok(reply) => Ok(Some(reply.value)),
            // The window went away between the notification and the question.
            Err(ReplyError::X11Error(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn cardinal(value: Option<Vec<u8>>) -> Option<u32> {
        let bytes: [u8; 4] = value?.get(..4)?.try_into().ok()?;
        Some(u32::from_ne_bytes(bytes))
    }

    /// The application that owns the active window.
    fn foreground(&self) -> Result<Seen, ReplyError> {
        let Some(window) =
            Self::cardinal(self.property(self.root, self.active, 1)?).filter(|w| *w != 0)
        else {
            return Ok(Seen::Nothing);
        };
        let Some(identity) = self
            .property(window, AtomEnum::WM_CLASS, 256)?
            .and_then(|class| from_wm_class(&class))
        else {
            return Ok(Seen::Nothing);
        };
        let own = Self::cardinal(self.property(window, self.pid, 1)?) == Some(std::process::id());
        Ok(Seen::Application { identity, own })
    }

    fn watch(&self, sink: &Sink) -> Result<(), ReplyError> {
        self.conn
            .change_window_attributes(
                self.root,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )?
            .check()?;
        sink.seen(self.foreground()?);
        while !sink.stopped() {
            let event = self.conn.wait_for_event()?;
            if matches!(event, Event::PropertyNotify(e) if e.window == self.root && e.atom == self.active)
            {
                sink.seen(self.foreground()?);
            }
        }
        Ok(())
    }
}

impl ForegroundObserver for X11Observer {
    fn status(&self) -> ObserverStatus {
        ObserverStatus::available(MECHANISM)
    }

    fn run(self: Box<Self>, sink: Sink) {
        if let Err(e) = self.watch(&sink) {
            // The display went away (the session ended, the server restarted).
            sink.status(ObserverStatus::Degraded {
                mechanism: MECHANISM.to_string(),
                reason: format!("The connection to the X display was lost ({e}). The last application seen stands until LaunchDeck is restarted."),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wayland_session_is_recognised_however_it_says_so() {
        assert!(wayland_session(Some("wayland"), None));
        assert!(wayland_session(Some("Wayland"), Some("wayland-0")));
        // XWayland is there, but the session is still Wayland.
        assert!(wayland_session(None, Some("wayland-1")));
        assert!(!wayland_session(Some("x11"), None));
        assert!(!wayland_session(None, None));
        assert!(!wayland_session(Some("tty"), Some("")));
    }

    #[test]
    fn the_reason_for_wayland_names_the_desktop_and_what_still_works() {
        let reason = wayland_reason(Some("GNOME"));
        assert!(
            reason.starts_with("This is a Wayland session (GNOME)."),
            "{reason}"
        );
        assert!(reason.contains("does not let an application see which application is in front"));
        assert!(reason.contains("needs an X11 session"));
        assert!(reason.contains("by hand works as always"));
        assert!(wayland_reason(None).starts_with("This is a Wayland session. "));
        assert!(wayland_reason(Some("  ")).starts_with("This is a Wayland session. "));
    }

    #[test]
    fn a_property_value_is_read_as_one_number_or_not_at_all() {
        assert_eq!(
            X11Observer::cardinal(Some(77u32.to_ne_bytes().to_vec())),
            Some(77)
        );
        assert_eq!(X11Observer::cardinal(Some(vec![1, 2])), None);
        assert_eq!(X11Observer::cardinal(Some(vec![])), None);
        assert_eq!(X11Observer::cardinal(None), None);
    }
}
