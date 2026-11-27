use std::process::Command;

#[test]
fn profile_commands_are_rejected() {
    let output = Command::new(env!("CARGO_BIN_EXE_cowboy"))
        .args(["config", "sync", "work", "unexpected"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown command: config"));
}

#[test]
fn help_lists_alias_and_omits_profile_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_cowboy"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("cowboy alias <command>"));
    assert!(!help.contains("cowboy config"));
}
