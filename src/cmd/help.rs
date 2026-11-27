pub(crate) fn print_help() {
    println!(
        "\
cowboy

Usage:
  cowboy
  cowboy alias <command>
  cowboy --help

Commands:
  alias    Set the Claude launcher command (default: claude)

Options:
  -h, --help     Show this help message
"
    );
}
