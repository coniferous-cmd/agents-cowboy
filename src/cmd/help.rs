pub(crate) fn print_help() {
    println!(
        "\
cowboy

Usage:
  cowboy
  cowboy config <command>
  cowboy alias <command>
  cowboy install
  cowboy --help

Commands:
  config   Manage Claude settings profiles and activation history
  alias    Set the Claude launcher command (default: claude)
  install  Download and install the latest Claude Code binary

Config commands:
  config list
  config create <name>
  config edit <name>
  config delete <name>
  config activate <name>
  config history list
  config history show <id>        Print snapshot JSON (may contain secrets)
  config history activate <id>
  config history delete <id>
  config history prune --keep <n>

Options:
  -h, --help     Show this help message
"
    );
}
