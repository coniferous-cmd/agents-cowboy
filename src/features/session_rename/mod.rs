use crate::domain::Session;

pub fn rename_initial_buffer(session: &Session) -> String {
    session.title.clone()
}

pub fn rename_prompt_status() -> &'static str {
    "Rename session"
}

pub fn rename_cancel_status() -> &'static str {
    "Rename canceled"
}

pub fn rename_success_status() -> &'static str {
    "Session renamed"
}

pub fn validate_rename_title(input: &str) -> Result<String, &'static str> {
    let title = input.trim();
    if title.is_empty() {
        return Err("Rename requires a non-empty title");
    }

    Ok(title.to_string())
}

#[cfg(test)]
mod tests {
    use super::validate_rename_title;

    #[test]
    fn rejects_empty_titles() {
        let error = validate_rename_title("   ").unwrap_err();
        assert_eq!(error, "Rename requires a non-empty title");
    }
}
