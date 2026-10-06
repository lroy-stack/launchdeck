//! What a command sends back when it fails: a sentence for the person and,
//! apart from it, the detail (ids, what the system or a module said) that
//! the UI keeps under Details. Rust is the only author of this wording.

use serde::Serialize;

use crate::artifacts::FileError;
use crate::devices::DeviceError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandError {
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl CommandError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            detail: None,
        }
    }

    pub(crate) fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// A sentence without detail: what every producer that is not converted yet
/// still returns.
impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

/// The device's sentence, and what the system said or the device id under
/// Details.
impl From<DeviceError> for CommandError {
    fn from(e: DeviceError) -> Self {
        let message = Self::new(e.to_string());
        match e.detail() {
            Some(detail) => message.with_detail(detail),
            None => message,
        }
    }
}

/// An export that could not be saved or shown: already a sentence and a detail.
impl From<FileError> for CommandError {
    fn from(e: FileError) -> Self {
        Self {
            message: e.message,
            detail: e.detail,
        }
    }
}

/// Work moved off the UI thread that never came back (it panicked or was
/// cancelled).
impl From<tauri::Error> for CommandError {
    fn from(e: tauri::Error) -> Self {
        Self::new("LaunchDeck could not finish this. Try again.").with_detail(e.to_string())
    }
}

pub(crate) type CmdResult<T> = Result<T, CommandError>;

/// A refusal of the runtime, with the ids the command was given as its
/// detail: the sentence names things as a person knows them, a bug report
/// needs the ids. An absent page is left out.
pub(crate) fn about<'a>(ids: &'a [(&'a str, &'a str)]) -> impl FnOnce(String) -> CommandError + 'a {
    move |message| {
        let detail = ids
            .iter()
            .filter(|(_, id)| !id.is_empty())
            .map(|(what, id)| format!("{what} id: {id}"))
            .collect::<Vec<_>>()
            .join(", ");
        CommandError::new(message).with_detail(detail)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_command_error_is_a_message_and_an_optional_detail() {
        let e = CommandError::new("That surface no longer exists.").with_detail("surface id: x");
        assert_eq!(
            serde_json::to_value(e).unwrap(),
            json!({ "message": "That surface no longer exists.", "detail": "surface id: x" })
        );
        assert_eq!(
            serde_json::to_value(CommandError::new("No.")).unwrap(),
            json!({ "message": "No." })
        );
    }

    #[test]
    fn a_plain_string_keeps_its_text_as_the_message() {
        let from_string = CommandError::from(String::from("name must be 1-40 characters"));
        assert_eq!(
            serde_json::to_value(from_string).unwrap(),
            json!({ "message": "name must be 1-40 characters" })
        );
        assert_eq!(CommandError::from("No."), CommandError::new("No."));
    }

    #[test]
    fn a_device_error_keeps_what_the_system_said_under_detail() {
        assert_eq!(
            CommandError::from(DeviceError::Port("port is in use".into())),
            CommandError::new("The Launchpad could not be reached.").with_detail("port is in use")
        );
        assert_eq!(
            CommandError::from(DeviceError::NotConnected),
            CommandError::new("The device is not connected.")
        );
    }

    #[test]
    fn a_file_error_is_sent_as_it_is() {
        let e = FileError {
            message: "LaunchDeck could not save the file.".into(),
            detail: Some("disk full".into()),
        };
        assert_eq!(
            serde_json::to_value(CommandError::from(e)).unwrap(),
            json!({ "message": "LaunchDeck could not save the file.", "detail": "disk full" })
        );
    }

    #[test]
    fn the_ids_a_command_was_given_are_the_detail() {
        let e = about(&[("surface", "studio"), ("page", ""), ("control", "c1")])(
            "That pad no longer exists.".into(),
        );
        assert_eq!(
            e,
            CommandError::new("That pad no longer exists.")
                .with_detail("surface id: studio, control id: c1")
        );
    }
}
