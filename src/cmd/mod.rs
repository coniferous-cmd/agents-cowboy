mod alias;
mod config;
mod help;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandMode {
    Tui,
    Help,
    Config(ConfigCommand),
    Alias(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigCommand {
    List,
    Create {
        name: String,
    },
    Edit {
        name: String,
    },
    Delete {
        name: String,
    },
    Activate {
        name: String,
    },
    Bind {
        project_path: String,
        profile_name: String,
    },
    Unbind {
        project_path: String,
    },
    Copy {
        source: String,
        new_name: String,
    },
    Sync {
        name: Option<String>,
    },
}

pub(crate) fn parse_cli_args<I>(args: I) -> Result<CommandMode, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(CommandMode::Tui);
    };

    match command.as_str() {
        "-h" | "--help" | "help" => Ok(CommandMode::Help),
        "config" => config::parse_config_args(args),
        "alias" => alias::parse_alias_args(args),
        unknown => Err(format!("Unknown command: {unknown}")),
    }
}

pub(crate) use alias::handle_alias;
pub(crate) use config::handle_config;
pub(crate) use help::print_help;

#[cfg(test)]
mod tests {
    use super::{parse_cli_args, CommandMode, ConfigCommand};

    #[test]
    fn parses_config_command_group() {
        let mode = parse_cli_args(["config".to_string(), "list".to_string()]).unwrap();

        assert_eq!(mode, CommandMode::Config(ConfigCommand::List));
    }

    #[test]
    fn export_is_no_longer_a_command() {
        let result = parse_cli_args(["export".to_string()]);

        assert!(result.is_err());
    }

    #[test]
    fn parses_help_flag() {
        let mode = parse_cli_args(["--help".to_string()]).unwrap();

        assert_eq!(mode, CommandMode::Help);
    }
}
