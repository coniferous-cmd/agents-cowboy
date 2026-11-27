mod alias;
mod help;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandMode {
    Tui,
    Help,
    Alias(String),
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
        "alias" => alias::parse_alias_args(args),
        unknown => Err(format!("Unknown command: {unknown}")),
    }
}

pub(crate) use alias::handle_alias;
pub(crate) use help::print_help;

#[cfg(test)]
mod tests {
    use super::{parse_cli_args, CommandMode};

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
