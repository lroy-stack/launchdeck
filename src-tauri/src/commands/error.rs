//! What a command sends back when it fails: a sentence for the person and,
//! apart from it, the detail (ids, what the system or a module said) that
//! the UI keeps under Details. Rust is the only author of this wording.

use serde::Serialize;

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
