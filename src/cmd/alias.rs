use std::error::Error;

use cowboy::claude_env::{ClaudeEnvStore, Setting};

use super::CommandMode;

pub(crate) fn handle_alias(
    env_store: &ClaudeEnvStore,
    command: String,
) -> Result<(), Box<dyn Error>> {
    let setting = Setting {
        key: "claude_command_alias".to_string(),
        value: command.clone(),
    };
    env_store
        .upsert_setting(&setting)
        .map_err(|error| format!("Failed to save claude command alias: {error}"))?;
    println!("Claude command alias set to: {command}");
    Ok(())
}

pub(super) fn parse_alias_args<I>(args: I) -> Result<CommandMode, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Err("Usage: cowboy alias <command>".to_string());
    };

    if args.next().is_some() {
        return Err("Alias accepts exactly one argument: the command name".to_string());
    }

    Ok(CommandMode::Alias(command))
}

#[cfg(test)]
mod tests {
    use super::parse_alias_args;
    use crate::cmd::CommandMode;

    #[test]
    fn accepts_alias_command() {
        let mode = parse_alias_args(["my-claude".to_string()]).unwrap();
        assert_eq!(mode, CommandMode::Alias("my-claude".to_string()));
    }

    #[test]
    fn rejects_missing_command() {
        let result = parse_alias_args([]);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("Usage:"),
            "expected usage error"
        );
    }

    #[test]
    fn rejects_extra_args() {
        let result = parse_alias_args(["my-claude".to_string(), "extra".to_string()]);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("exactly one argument"),
            "expected 'exactly one argument' error"
        );
    }
}
