pub(crate) fn print_help() {
    println!(
        "\
cowboy

Usage:
  cowboy
  cowboy config <command>
  cowboy alias <command>
  cowboy --help

Commands:
  config   Manage Claude settings profiles
  alias    Set the Claude launcher command (default: claude)

Config commands:
  config list
  config create <name>
  config edit <name>
  config delete <name>
  config activate <name>
  config bind <project-path> <profile-name>
  config unbind <project-path>
  config copy <source> <new-name>
  config sync [name]    Reconcile profiles from on-disk files into the database

Options:
  -h, --help     Show this help message
"
    );
}
