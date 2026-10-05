//! A portable application: how a Pack names an application on every system
//! it may be imported on.
//!
//! An application has another stable id on each system: a bundle identifier
//! on macOS, an executable's file name on Windows, the class of `WM_CLASS` on
//! X11. A portable application carries the ids it is known by, one per
//! namespace, and keeps any valid namespace as it was written, also one this
//! version does not observe. On import, only the id in the namespace the
//! engine reports is taken, normalised as an observed identity is; without
//! one there, the application is unmapped on this system. The name is for
//! people and never matches anything.
//!
//! Pure: no observer, no workspace, no file.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::identity::ApplicationIdentity;

/// Characters in an application's name.
pub const MAX_NAME: usize = 80;
/// Identities of one application, each for another system.
pub const MAX_IDENTITIES: usize = 8;

/// An application's id in one namespace (`macos`, `windows`, `x11`, or any
/// other that is valid).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableIdentity {
    pub namespace: String,
    pub id: String,
}

/// An application as a Pack names it: once for people, once per system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableApplication {
    /// What a person reads. Never compared.
    pub name: String,
    pub identities: Vec<PortableIdentity>,
}

/// What a portable application is on one system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// The identity a rule names on this system.
    Identity(ApplicationIdentity),
    /// It has no id in this system's namespace.
    Unmapped,
}

impl PortableApplication {
    /// The application a local rule names, for export: its name and its one
    /// identity.
    pub fn from_identity(identity: &ApplicationIdentity) -> PortableApplication {
        let identity = identity.normalized();
        let name: String = identity.display_name.chars().take(MAX_NAME).collect();
        PortableApplication {
            name: name.trim_end().to_string(),
            identities: vec![PortableIdentity {
                namespace: identity.namespace,
                id: identity.id,
            }],
        }
    }

    /// The same application, also known by `id` on the system of `namespace`
    /// (the person adds it on export). The id is normalised as an observed one
    /// is, and the result must still pass [`PortableApplication::validate`].
    pub fn with_identity(self, namespace: &str, id: &str) -> Result<PortableApplication, String> {
        let added = ApplicationIdentity::new(namespace, id, "");
        let mut identities = self.identities;
        identities.push(PortableIdentity {
            namespace: added.namespace,
            id: added.id,
        });
        let app = PortableApplication {
            name: self.name,
            identities,
        };
        app.validate()?;
        Ok(app)
    }

    /// Whether this can be imported: a name of 1 to 80 characters with no
    /// space around it and no control character, and 1 to 8 identities in
    /// distinct namespaces, each one that could be the subject of a rule.
    pub fn validate(&self) -> Result<(), String> {
        let name = &self.name;
        if name.trim().is_empty() {
            return Err("an application needs a name".into());
        }
        if name.chars().count() > MAX_NAME
            || name != name.trim()
            || name.chars().any(char::is_control)
        {
            return Err(format!(
                "an application's name is at most {MAX_NAME} characters, with no space around it and no control character"
            ));
        }
        if self.identities.is_empty() {
            return Err("an application needs its identity on at least one system".into());
        }
        if self.identities.len() > MAX_IDENTITIES {
            return Err(format!(
                "an application has at most {MAX_IDENTITIES} identities"
            ));
        }
        let mut namespaces = HashSet::new();
        for (i, known) in self.identities.iter().enumerate() {
            let identity = ApplicationIdentity {
                namespace: known.namespace.clone(),
                id: known.id.clone(),
                display_name: String::new(),
            };
            identity
                .validate()
                .map_err(|e| format!("identity {}: {e}", i + 1))?;
            if !namespaces.insert(known.namespace.as_str()) {
                return Err(format!(
                    "an application has one identity of each kind, and this one has two of the kind \"{}\"",
                    known.namespace
                ));
            }
        }
        Ok(())
    }

    /// What this application is on the system whose observer reports
    /// `namespace` (`Engine::namespace()`): the identity a rule names there,
    /// normalised as an observed one is, or `Unmapped`. Namespaces are
    /// compared exactly; the name plays no part. For an application that
    /// passed [`PortableApplication::validate`].
    pub fn resolve(&self, namespace: &str) -> Resolved {
        self.identities
            .iter()
            .find(|known| known.namespace == namespace)
            .map_or(Resolved::Unmapped, |known| {
                Resolved::Identity(ApplicationIdentity::new(
                    &known.namespace,
                    &known.id,
                    &self.name,
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::context::identity::{from_executable_path, namespace_of, MACOS, WINDOWS, X11};
    use crate::platform::Platform;

    fn known_as(namespace: &str, id: &str) -> PortableIdentity {
        PortableIdentity {
            namespace: namespace.into(),
            id: id.into(),
        }
    }

    fn code() -> PortableApplication {
        PortableApplication {
            name: "Visual Studio Code".into(),
            identities: vec![
                known_as(MACOS, "com.microsoft.VSCode"),
                known_as(WINDOWS, "Code.EXE"),
                known_as(X11, "Code"),
            ],
        }
    }

    /// (namespace, id, display name) of what `app` is on `namespace`.
    fn on(app: &PortableApplication, namespace: &str) -> Option<(String, String, String)> {
        match app.resolve(namespace) {
            Resolved::Identity(i) => Some((i.namespace, i.id, i.display_name)),
            Resolved::Unmapped => None,
        }
    }

    fn triple(namespace: &str, id: &str, name: &str) -> Option<(String, String, String)> {
        Some((namespace.into(), id.into(), name.into()))
    }

    #[test]
    fn each_system_takes_its_own_identity_normalised_as_observed() {
        let app = code();
        assert_eq!(app.validate(), Ok(()));
        assert_eq!(
            on(&app, MACOS),
            triple(MACOS, "com.microsoft.VSCode", "Visual Studio Code")
        );
        // File names on Windows do not distinguish case: lower case, as observed.
        assert_eq!(
            on(&app, WINDOWS),
            triple(WINDOWS, "code.exe", "Visual Studio Code")
        );
        assert_eq!(on(&app, X11), triple(X11, "Code", "Visual Studio Code"));
        // It is the application the observer reports there.
        let Resolved::Identity(rule) = app.resolve(WINDOWS) else {
            panic!("Windows is mapped");
        };
        let seen = from_executable_path(r"C:\Program Files\Microsoft VS Code\Code.exe").unwrap();
        assert!(rule.is(&seen));
    }

    #[test]
    fn a_system_it_has_no_identity_for_is_unmapped() {
        let xcode = PortableApplication {
            name: "Xcode".into(),
            identities: vec![known_as(MACOS, "com.apple.dt.Xcode")],
        };
        assert_eq!(xcode.validate(), Ok(()));
        for namespace in [WINDOWS, X11, "", "MACOS", "macos "] {
            assert_eq!(
                xcode.resolve(namespace),
                Resolved::Unmapped,
                "{namespace:?}"
            );
        }
    }

    #[test]
    fn an_unknown_but_valid_namespace_is_carried_and_never_matched() {
        let files = PortableApplication {
            name: "Files".into(),
            identities: vec![
                known_as("wayland", "org.gnome.Nautilus"),
                known_as(X11, "Org.gnome.Nautilus"),
            ],
        };
        assert_eq!(files.validate(), Ok(()));
        // Written and read back as it was.
        let written = serde_json::to_value(&files).unwrap();
        assert_eq!(
            written,
            json!({
                "name": "Files",
                "identities": [
                    { "namespace": "wayland", "id": "org.gnome.Nautilus" },
                    { "namespace": "x11", "id": "Org.gnome.Nautilus" }
                ]
            })
        );
        let read: PortableApplication = serde_json::from_value(written).unwrap();
        assert_eq!(read, files);
        // No system LaunchDeck observes reports that namespace.
        for platform in [Platform::Linux, Platform::MacOs, Platform::Windows] {
            let namespace = namespace_of(platform).unwrap();
            if let Resolved::Identity(identity) = files.resolve(namespace) {
                assert_eq!(identity.namespace, namespace);
                assert_ne!(identity.id, "org.gnome.Nautilus");
            }
        }
        assert_eq!(on(&files, X11), triple(X11, "Org.gnome.Nautilus", "Files"));
    }

    #[test]
    fn identities_that_cannot_name_an_application_are_refused() {
        let with = |namespace: &str, id: &str| PortableApplication {
            name: "App".into(),
            identities: vec![known_as(namespace, id)],
        };
        assert_eq!(with(MACOS, "com.example.App").validate(), Ok(()));
        let long = "x".repeat(256);
        for (namespace, id) in [
            (MACOS, ""),
            (MACOS, "   "),
            (MACOS, " com.example.App"),
            (X11, "a\nb"),
            (X11, long.as_str()),
            ("", "Code"),
            ("Mac OS", "com.example.App"),
            ("x_11", "Code"),
            ("1x", "Code"),
        ] {
            assert!(
                with(namespace, id).validate().is_err(),
                "{namespace:?} {id:?}"
            );
        }
    }

    #[test]
    fn one_to_eight_identities_each_in_its_own_namespace() {
        let namespaces = [
            "macos", "windows", "x11", "wayland", "haiku", "redox", "fuchsia", "genode", "plan9",
        ];
        let app = |count: usize| PortableApplication {
            name: "App".into(),
            identities: namespaces[..count]
                .iter()
                .map(|namespace| known_as(namespace, "app"))
                .collect(),
        };
        assert_eq!(app(1).validate(), Ok(()));
        assert_eq!(app(8).validate(), Ok(()));
        assert!(app(0).validate().is_err(), "no identity at all");
        assert!(app(9).validate().is_err(), "nine identities");

        let mut twice = code();
        twice
            .identities
            .push(known_as(MACOS, "com.vscodium.codium"));
        let refused = twice.validate().unwrap_err();
        assert!(refused.contains("macos"), "{refused}");
    }

    #[test]
    fn a_name_is_for_people_and_must_be_one() {
        let named = |name: &str| PortableApplication {
            name: name.into(),
            ..code()
        };
        assert_eq!(named(&"n".repeat(80)).validate(), Ok(()));
        assert_eq!(
            named(&"é".repeat(80)).validate(),
            Ok(()),
            "characters, not bytes"
        );
        for bad in ["", "   ", " Code", "Code ", "Co\nde", "Code\u{7}"] {
            assert!(named(bad).validate().is_err(), "{bad:?}");
        }
        assert!(named(&"n".repeat(81)).validate().is_err());
    }

    #[test]
    fn the_name_never_stands_for_an_identity() {
        // Named like another application's bundle id, it is still only Firefox on X11.
        let firefox = PortableApplication {
            name: "com.apple.Safari".into(),
            identities: vec![known_as(X11, "firefox")],
        };
        assert_eq!(firefox.resolve(MACOS), Resolved::Unmapped);
        let Resolved::Identity(rule) = firefox.resolve(X11) else {
            panic!("X11 is mapped");
        };
        assert!(!rule.is(&ApplicationIdentity::new(
            X11,
            "com.apple.Safari",
            "com.apple.Safari"
        )));
        // Another name is the same application.
        let renamed = PortableApplication {
            name: "Web browser".into(),
            ..firefox.clone()
        };
        assert_eq!(renamed.resolve(X11), firefox.resolve(X11));
    }

    #[test]
    fn members_it_does_not_know_are_refused() {
        assert!(serde_json::from_value::<PortableApplication>(json!({
            "name": "App",
            "identities": [{ "namespace": "x11", "id": "App" }],
            "displayName": "App"
        }))
        .is_err());
        assert!(serde_json::from_value::<PortableIdentity>(json!({
            "namespace": "x11",
            "id": "App",
            "displayName": "App"
        }))
        .is_err());
    }

    #[test]
    fn a_local_rule_exports_with_its_identity_and_more_can_be_added() {
        let rule = ApplicationIdentity::new(MACOS, "com.microsoft.VSCode", "Visual Studio Code");
        let app = PortableApplication::from_identity(&rule);
        assert_eq!(
            app,
            PortableApplication {
                name: "Visual Studio Code".into(),
                identities: vec![known_as(MACOS, "com.microsoft.VSCode")],
            }
        );
        assert_eq!(app.resolve(MACOS), Resolved::Identity(rule));

        let app = app
            .with_identity(WINDOWS, " Code.exe ")
            .unwrap()
            .with_identity(X11, "Code")
            .unwrap();
        assert_eq!(app.identities[1], known_as(WINDOWS, "code.exe"));
        assert_eq!(app.validate(), Ok(()));
        assert!(
            app.clone()
                .with_identity(MACOS, "com.vscodium.codium")
                .is_err(),
            "one identity per system"
        );
        assert!(app.clone().with_identity("Bad Kind", "x").is_err());
        assert!(app.with_identity("wayland", "  ").is_err());

        // A rule read from a file may have no display name: its id names it.
        let bare = ApplicationIdentity {
            namespace: X11.into(),
            id: "Gnome-terminal".into(),
            display_name: String::new(),
        };
        assert_eq!(
            PortableApplication::from_identity(&bare).name,
            "Gnome-terminal"
        );
        // A name made from a long id is cut to fit.
        let long = ApplicationIdentity::new(X11, &format!("{} tail", "x".repeat(79)), "");
        let exported = PortableApplication::from_identity(&long);
        assert_eq!(exported.name, "x".repeat(79));
        assert_eq!(exported.validate(), Ok(()));
    }
}
