mod alias;
mod config;
mod help;
mod install;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandMode {
    Tui,
    Help,
    Config(ConfigCommand),
    Alias(String),
    Install,
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
    History(HistoryCommand),
    Bind {
        project_path: String,
        profile_name: String,
    },
    Unbind {
        project_path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryCommand {
    List,
    Show { id: i64 },
    Activate { id: i64 },
    Delete { id: i64 },
    Prune { keep: usize },
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
        "install" => install::parse_install_args(args),
        unknown => Err(format!("Unknown command: {unknown}")),
    }
}

pub(crate) use alias::handle_alias;
pub(crate) use config::handle_config;
pub(crate) use help::print_help;
pub(crate) use install::handle_install;

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

    #[test]
    fn parses_install_command() {
        let mode = parse_cli_args(["install".to_string()]).unwrap();

        assert_eq!(mode, CommandMode::Install);
    }

    #[test]
    fn rejects_install_with_extra_args() {
        let result = parse_cli_args(["install".to_string(), "extra".to_string()]);

        assert!(result.is_err());
    }
}
