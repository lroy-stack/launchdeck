//! The file's inputs (plan 7.8): the person's answers checked, and an
//! assignment as it is written with them.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::actions::definition::Assignment;
use crate::actions::params::hidden_char;
use crate::artifacts::format::{
    echo, plain_message, web_address, AssignmentV1, InputDef, InputType, InputValue,
};
use crate::artifacts::limits::{DEFAULT_STRING_INPUT, MAX_APPLICATION_NAME, MAX_PATH, MAX_URL};
use crate::context::identity::ApplicationIdentity;
use crate::platform::Platform;

/// The answers that may be written, and what is wrong with the others.
pub(super) struct Answers {
    /// Valid answers, by input id.
    pub values: BTreeMap<String, InputValue>,
    /// Problems, by input id; absent when there is none.
    pub issues: BTreeMap<String, Vec<String>>,
}

impl Answers {
    pub fn add_issue(&mut self, input: &str, issue: String) {
        let issues = self.issues.entry(input.to_string()).or_default();
        if !issues.contains(&issue) {
            issues.push(issue);
        }
    }

    /// Answers something refused once they were written into a pad are no
    /// longer answers that may be written.
    pub fn drop_refused(&mut self) {
        let issues = &self.issues;
        self.values.retain(|id, _| !issues.contains_key(id));
    }
}

/// Check the answer given to each declared input. Answers to inputs the
/// file does not declare are ignored.
pub(super) fn answers(
    defs: &[InputDef],
    given: &BTreeMap<String, InputValue>,
    namespace: Option<&str>,
    platform: Platform,
) -> Answers {
    let mut out = Answers {
        values: BTreeMap::new(),
        issues: BTreeMap::new(),
    };
    for def in defs {
        let Some(value) = given.get(&def.id) else {
            continue;
        };
        let issues = check_value(def, value, namespace, platform);
        if issues.is_empty() {
            out.values.insert(def.id.clone(), value.clone());
        } else {
            out.issues.insert(def.id.clone(), issues);
        }
    }
    out
}

/// What is wrong with `value` as an answer to `def` on this system, as
/// sentences; empty when nothing is.
pub(super) fn check_value(
    def: &InputDef,
    value: &InputValue,
    namespace: Option<&str>,
    platform: Platform,
) -> Vec<String> {
    match (def.kind, value) {
        (InputType::Application, InputValue::Application { .. }) => {
            check_application(value, namespace).into_iter().collect()
        }
        (InputType::Application, InputValue::Text { .. }) => {
            vec!["This input takes an application, not a text.".into()]
        }
        (_, InputValue::Application { .. }) => {
            vec!["This input takes a text, not an application.".into()]
        }
        (kind, InputValue::Text { value }) => {
            check_text(def, kind, value, platform).into_iter().collect()
        }
    }
}

fn check_application(value: &InputValue, namespace: Option<&str>) -> Option<String> {
    let InputValue::Application {
        namespace: kind,
        id,
        display_name,
    } = value
    else {
        return None;
    };
    let Some(here) = namespace else {
        return Some(
            "LaunchDeck cannot tell which application is in front on this system, so no \
             application can be chosen here."
                .into(),
        );
    };
    if kind != here {
        return Some(format!(
            "The application is named as on another kind of system (\"{}\"); choose one as \
             this system names it (\"{here}\").",
            echo(kind)
        ));
    }
    let identity = ApplicationIdentity {
        namespace: kind.clone(),
        id: id.clone(),
        display_name: String::new(),
    };
    if let Err(e) = identity.validate() {
        return Some(format!(
            "The application cannot be used: {}.",
            plain_message(&e.to_string())
        ));
    }
    if display_name.chars().count() > MAX_APPLICATION_NAME || has_hidden(display_name) {
        return Some(format!(
            "The application's name is at most {MAX_APPLICATION_NAME} characters, with none a \
             person cannot see."
        ));
    }
    None
}

fn check_text(def: &InputDef, kind: InputType, value: &str, platform: Platform) -> Option<String> {
    if let Some(c) = value.chars().find(|&c| c.is_control() || hidden_char(c)) {
        return Some(format!(
            "The answer holds the character U+{:04X}, which a person cannot see.",
            u32::from(c)
        ));
    }
    let length = value.chars().count();
    match kind {
        InputType::Path if length == 0 || length > MAX_PATH => {
            Some(format!("A path has 1 to {MAX_PATH} characters."))
        }
        InputType::Path if !absolute_on(value, platform) => Some(match platform {
            Platform::Windows => {
                "The path must be a full path, such as C:\\Users\\me\\notes.".into()
            }
            _ => "The path must be a full path, such as /Users/me/notes.".into(),
        }),
        InputType::Url if length > MAX_URL || !web_address(value) => Some(format!(
            "The address must start with http:// or https://, name a host, hold no user name \
             or password, and have at most {MAX_URL} characters."
        )),
        InputType::Enum if !def.options.iter().any(|o| o.value == value) => {
            Some("The answer is not one of the choices.".into())
        }
        InputType::String if length > def.max_length.unwrap_or(DEFAULT_STRING_INPUT) => {
            Some(format!(
                "The answer has at most {} characters.",
                def.max_length.unwrap_or(DEFAULT_STRING_INPUT)
            ))
        }
        _ => None,
    }
}

/// A full path on `platform`: from the root on Linux and macOS; from a drive
/// (`C:\`, `C:/`) or a network share (`\\server\share`) on Windows.
fn absolute_on(path: &str, platform: Platform) -> bool {
    match platform {
        Platform::Windows => {
            let mut chars = path.chars();
            let drive = matches!(
                (chars.next(), chars.next(), chars.next()),
                (Some(letter), Some(':'), Some('\\' | '/')) if letter.is_ascii_alphabetic()
            );
            drive || path.starts_with("\\\\")
        }
        Platform::Linux | Platform::MacOs => path.starts_with('/'),
    }
}

fn has_hidden(text: &str) -> bool {
    text.chars().any(|c| c.is_control() || hidden_char(c))
}

/// An assignment of the file as it is written here: each argument bound to
/// an input replaced by its answer (an optional input without one removes
/// the argument; a required one keeps the file's value until it is
/// answered), and each argument the file marks as naming one of its
/// surfaces renamed to that surface's local id. Every other argument is as
/// the file has it.
pub(in crate::artifacts) fn bound(
    a: &AssignmentV1,
    defs: &[InputDef],
    values: &BTreeMap<String, InputValue>,
    local_ids: &BTreeMap<String, String>,
) -> Assignment {
    let mut args = a.args.clone();
    for (arg, input) in &a.inputs {
        match values.get(input) {
            Some(InputValue::Text { value }) => {
                args.insert(arg.clone(), Value::String(value.clone()));
            }
            Some(InputValue::Application { .. }) => {}
            None => {
                let optional = defs.iter().any(|d| &d.id == input && !d.required);
                if optional {
                    args.remove(arg);
                }
            }
        }
    }
    for arg in &a.surface_refs {
        let renamed = match args.get(arg) {
            Some(Value::String(id)) => local_ids.get(id).cloned(),
            _ => None,
        };
        if let Some(local) = renamed {
            args.insert(arg.clone(), Value::String(local));
        }
    }
    Assignment::new(&a.provider, &a.action, Value::Object(args))
}
