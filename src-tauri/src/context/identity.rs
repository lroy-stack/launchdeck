//! What an application is called, in a way that survives a rename, an update
//! and another language.
//!
//! The `id` is whatever the operating system itself uses to tell applications
//! apart: a bundle identifier on macOS, the executable's file name on Windows,
//! the class of `WM_CLASS` on X11. The `namespace` says which of those it is,
//! so a rule written on one system is never mistaken for one written on
//! another. The display name is for people and is never compared.

use serde::{Deserialize, Serialize};

use crate::platform::Platform;

/// Bundle identifiers, as `NSRunningApplication` reports them.
pub const MACOS: &str = "macos";
/// Executable file names, lower case.
pub const WINDOWS: &str = "windows";
/// The class part of `WM_CLASS`.
pub const X11: &str = "x11";

const MAX_NAMESPACE: usize = 16;
const MAX_ID: usize = 255;
const MAX_DISPLAY_NAME: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationIdentity {
    pub namespace: String,
    pub id: String,
    /// What a person reads. Never part of a comparison.
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    #[error("the kind of application identity \"{0}\" is not valid")]
    Namespace(String),
    #[error("an application needs an identifier")]
    EmptyId,
    #[error(
        "the application identifier is too long or holds characters that cannot be part of one"
    )]
    BadId,
}

impl ApplicationIdentity {
    /// An identity as a platform reports it. The id is trimmed, and on Windows
    /// lower-cased (file names there do not distinguish case). An empty display
    /// name becomes the id.
    pub fn new(namespace: &str, id: &str, display_name: &str) -> ApplicationIdentity {
        let id = id.trim();
        let id = if namespace == WINDOWS {
            id.to_ascii_lowercase()
        } else {
            id.to_string()
        };
        let shown: String = display_name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_DISPLAY_NAME)
            .collect();
        ApplicationIdentity {
            namespace: namespace.to_string(),
            display_name: if shown.is_empty() { id.clone() } else { shown },
            id,
        }
    }

    /// The same application: the same kind of identity and the same id,
    /// ignoring ASCII case. The display name plays no part.
    pub fn is(&self, other: &ApplicationIdentity) -> bool {
        self.namespace == other.namespace && self.id.eq_ignore_ascii_case(&other.id)
    }

    /// Whether this can be the subject of a rule.
    pub fn validate(&self) -> Result<(), IdentityError> {
        let ns = &self.namespace;
        let namespace_ok = !ns.is_empty()
            && ns.len() <= MAX_NAMESPACE
            && ns.starts_with(|c: char| c.is_ascii_lowercase())
            && ns
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !namespace_ok {
            return Err(IdentityError::Namespace(
                ns.chars().filter(|c| !c.is_control()).take(24).collect(),
            ));
        }
        if self.id.trim().is_empty() {
            return Err(IdentityError::EmptyId);
        }
        if self.id.chars().count() > MAX_ID
            || self.id != self.id.trim()
            || self.id.chars().any(char::is_control)
        {
            return Err(IdentityError::BadId);
        }
        Ok(())
    }

    /// The identity as it is stored: the same application, with a display name
    /// that fits.
    pub fn normalized(&self) -> ApplicationIdentity {
        ApplicationIdentity::new(&self.namespace, &self.id, &self.display_name)
    }
}

// What each system reports, turned into an identity. These are plain text in,
// identity out, so they are tested on every system, not only the one they serve.

/// From a bundle identifier and a localized name, as `NSRunningApplication`
/// gives them. A process without a bundle identifier has no identity.
pub fn from_bundle(bundle_id: &str, localized_name: &str) -> Option<ApplicationIdentity> {
    let id = bundle_id.trim();
    (!id.is_empty()).then(|| ApplicationIdentity::new(MACOS, id, localized_name))
}

/// From the full path of the executable that owns the foreground window. The
/// identity is the file name: the folder changes with every update of many
/// applications, the name does not.
pub fn from_executable_path(path: &str) -> Option<ApplicationIdentity> {
    let file = path.trim().rsplit(['\\', '/']).next().unwrap_or("").trim();
    if file.is_empty() {
        return None;
    }
    let name = match file.len().checked_sub(4) {
        Some(at) if file.is_char_boundary(at) && file[at..].eq_ignore_ascii_case(".exe") => {
            &file[..at]
        }
        _ => file,
    };
    Some(ApplicationIdentity::new(WINDOWS, file, name))
}

/// From the value of a window's `WM_CLASS`: two strings, each ended by a zero
/// byte, the instance and then the class. The class is the identity.
pub fn from_wm_class(value: &[u8]) -> Option<ApplicationIdentity> {
    let mut parts = value.split(|b| *b == 0);
    let instance = parts.next().unwrap_or(&[]);
    let class = parts.next().filter(|c| !c.is_empty()).unwrap_or(instance);
    let class = String::from_utf8_lossy(class);
    let class = class.trim();
    (!class.is_empty()).then(|| ApplicationIdentity::new(X11, class, class))
}

/// Two identities are equal when they name the same application.
impl PartialEq for ApplicationIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.is(other)
    }
}
impl Eq for ApplicationIdentity {}

/// The namespace applications have on `platform`, when it is one LaunchDeck
/// can observe at all.
pub fn namespace_of(platform: Platform) -> Option<&'static str> {
    Some(match platform {
        Platform::Linux => X11,
        Platform::MacOs => MACOS,
        Platform::Windows => WINDOWS,
    })
}

/// The namespace applications have on the system this was built for.
pub fn native_namespace() -> Option<&'static str> {
    namespace_of(Platform::current())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_system_has_its_namespace() {
        assert_eq!(namespace_of(Platform::Linux), Some(X11));
        assert_eq!(namespace_of(Platform::MacOs), Some(MACOS));
        assert_eq!(namespace_of(Platform::Windows), Some(WINDOWS));
        assert_eq!(native_namespace(), namespace_of(Platform::current()));
    }

    #[test]
    fn the_same_application_is_recognised_whatever_it_is_called() {
        let rule = ApplicationIdentity::new(MACOS, "com.microsoft.VSCode", "Visual Studio Code");
        // Renamed, localised, or with no name at all: still the same application.
        for seen in [
            ApplicationIdentity::new(MACOS, "com.microsoft.VSCode", "Code"),
            ApplicationIdentity::new(
                MACOS,
                "com.microsoft.VSCode",
                "Visual Studio Code - Insiders",
            ),
            ApplicationIdentity::new(MACOS, "com.microsoft.vscode", ""),
        ] {
            assert!(rule.is(&seen), "{seen:?}");
            assert_eq!(rule, seen);
        }
        // The same name is not the same application.
        let other = ApplicationIdentity::new(MACOS, "com.vscodium", "Visual Studio Code");
        assert!(!rule.is(&other));
    }

    #[test]
    fn an_identity_belongs_to_one_kind_of_system() {
        let x = ApplicationIdentity::new(X11, "Code", "Code");
        let w = ApplicationIdentity::new(WINDOWS, "Code", "Code");
        assert!(
            !x.is(&w),
            "the same id in another namespace is another application"
        );
    }

    #[test]
    fn what_a_platform_reports_is_normalised_once() {
        let w = ApplicationIdentity::new(WINDOWS, "  Code.EXE ", "");
        assert_eq!(
            (w.id.as_str(), w.display_name.as_str()),
            ("code.exe", "code.exe")
        );
        // Elsewhere the case the system uses is kept as it is, and only ignored when comparing.
        let x = ApplicationIdentity::new(X11, "Gnome-terminal", "Terminal\u{7}");
        assert_eq!(
            (x.id.as_str(), x.display_name.as_str()),
            ("Gnome-terminal", "Terminal")
        );
        let long = ApplicationIdentity::new(X11, "a", &"n".repeat(200));
        assert_eq!(long.display_name.chars().count(), 80);
    }

    #[test]
    fn an_identity_that_cannot_name_an_application_is_refused() {
        let id = |ns: &str, id: &str| ApplicationIdentity {
            namespace: ns.into(),
            id: id.into(),
            display_name: String::new(),
        };
        assert!(id(X11, "Code").validate().is_ok());
        assert!(
            id("wayland", "org.gnome.Nautilus").validate().is_ok(),
            "a kind this version does not observe still loads"
        );
        assert_eq!(id(X11, "").validate(), Err(IdentityError::EmptyId));
        assert_eq!(id(X11, "   ").validate(), Err(IdentityError::EmptyId));
        assert_eq!(id(X11, "a\nb").validate(), Err(IdentityError::BadId));
        assert_eq!(id(X11, " padded ").validate(), Err(IdentityError::BadId));
        assert_eq!(
            id(X11, &"x".repeat(256)).validate(),
            Err(IdentityError::BadId)
        );
        for bad in ["", "X11", "1x", "has space", "way_land", &"n".repeat(17)] {
            assert!(
                matches!(id(bad, "Code").validate(), Err(IdentityError::Namespace(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn each_system_has_its_own_stable_identity() {
        // macOS: the bundle identifier. The name is whatever the system shows.
        let mac = from_bundle("com.microsoft.VSCode", "Visual Studio Code").unwrap();
        assert_eq!(
            (
                mac.namespace.as_str(),
                mac.id.as_str(),
                mac.display_name.as_str()
            ),
            (MACOS, "com.microsoft.VSCode", "Visual Studio Code")
        );
        assert!(from_bundle("", "A tool without a bundle").is_none());

        // Windows: the executable's file name, whatever folder this version lives in.
        let a =
            from_executable_path(r"C:\Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe")
                .unwrap();
        let b = from_executable_path(r"D:\Apps\VSCode\app-1.94.2\CODE.EXE").unwrap();
        assert_eq!(
            (a.namespace.as_str(), a.id.as_str(), a.display_name.as_str()),
            (WINDOWS, "code.exe", "Code")
        );
        assert!(
            a.is(&b),
            "an update that moves the folder is the same application"
        );
        assert_eq!(
            from_executable_path(r"C:\Windows\explorer.exe").unwrap().id,
            "explorer.exe"
        );
        assert_eq!(from_executable_path("tool").unwrap().display_name, "tool");
        assert!(
            from_executable_path("").is_none() && from_executable_path(r"C:\folder\").is_none()
        );

        // X11: the class of WM_CLASS, not the instance.
        let x = from_wm_class(b"code\0Code\0").unwrap();
        assert_eq!((x.namespace.as_str(), x.id.as_str()), (X11, "Code"));
        assert_eq!(
            from_wm_class(b"gnome-terminal-server\0Gnome-terminal\0")
                .unwrap()
                .id,
            "Gnome-terminal"
        );
        // Only one string: it stands for both. Nothing: no identity.
        assert_eq!(from_wm_class(b"xterm\0").unwrap().id, "xterm");
        assert_eq!(from_wm_class(b"xterm").unwrap().id, "xterm");
        assert!(from_wm_class(b"").is_none() && from_wm_class(b"\0\0").is_none());
    }

    #[test]
    fn it_is_stored_with_its_namespace_id_and_name() {
        let app = ApplicationIdentity::new(X11, "Code", "Visual Studio Code");
        let json = serde_json::to_value(&app).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "namespace": "x11", "id": "Code", "displayName": "Visual Studio Code" })
        );
        let back: ApplicationIdentity =
            serde_json::from_value(serde_json::json!({ "namespace": "x11", "id": "Code" }))
                .unwrap();
        assert!(back.is(&app), "a display name is optional in the file");
    }
}
