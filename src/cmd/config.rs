use std::io::{self, Write};

use cowboy::claude_env::ClaudeEnvStore;

use cowboy::features::profile_editor::{edit_profile_json, EditOutcome};

use super::{CommandMode, ConfigCommand};

const CONFIG_USAGE: &str =
    "Usage: cowboy config <list|create|edit|delete|activate|bind|unbind|copy>";

pub(super) fn parse_config_args<I>(args: I) -> Result<CommandMode, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Err(CONFIG_USAGE.to_string());
    };

    let command = match command.as_str() {
        "list" => {
            reject_extra(&mut args, "config list")?;
            ConfigCommand::List
        }
        "create" => ConfigCommand::Create {
            name: exactly_one(&mut args, "config create <name>")?,
        },
        "edit" => ConfigCommand::Edit {
            name: exactly_one(&mut args, "config edit <name>")?,
        },
        "delete" => ConfigCommand::Delete {
            name: exactly_one(&mut args, "config delete <name>")?,
        },
        "activate" => ConfigCommand::Activate {
            name: exactly_one(&mut args, "config activate <name>")?,
        },
        "bind" => {
            let project_path = args.next().ok_or_else(|| {
                "Usage: cowboy config bind <project-path> <profile-name>".to_string()
            })?;
            let profile_name = args.next().ok_or_else(|| {
                "Usage: cowboy config bind <project-path> <profile-name>".to_string()
            })?;
            reject_extra(&mut args, "config bind <project-path> <profile-name>")?;
            ConfigCommand::Bind {
                project_path,
                profile_name,
            }
        }
        "unbind" => ConfigCommand::Unbind {
            project_path: exactly_one(&mut args, "config unbind <project-path>")?,
        },
        "copy" => {
            let source = args
                .next()
                .ok_or_else(|| "Usage: cowboy config copy <source> <new-name>".to_string())?;
            let new_name = args
                .next()
                .ok_or_else(|| "Usage: cowboy config copy <source> <new-name>".to_string())?;
            reject_extra(&mut args, "config copy <source> <new-name>")?;
            ConfigCommand::Copy { source, new_name }
        }
        unknown => return Err(format!("Unknown config command: {unknown}\n{CONFIG_USAGE}")),
    };

    Ok(CommandMode::Config(command))
}

fn exactly_one<I>(args: &mut I, usage: &str) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    let value = args
        .next()
        .ok_or_else(|| format!("Usage: cowboy {usage}"))?;
    reject_extra(args, usage)?;
    Ok(value)
}

fn reject_extra<I>(args: &mut I, command: &str) -> Result<(), String>
where
    I: Iterator<Item = String>,
{
    if let Some(extra) = args.next() {
        Err(format!("Unexpected argument for {command}: {extra}"))
    } else {
        Ok(())
    }
}

pub(crate) fn handle_config(
    store: &ClaudeEnvStore,
    command: ConfigCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    handle_config_with_writer(store, command, &mut output)
}

fn handle_config_with_writer<W: Write>(
    store: &ClaudeEnvStore,
    command: ConfigCommand,
    output: &mut W,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        ConfigCommand::List => {
            for profile in store
                .list_profiles()
                .map_err(|error| format!("Failed to list profiles: {error}"))?
            {
                writeln!(output, "{}", profile.name)?;
            }
        }
        ConfigCommand::Create { name } => {
            let profile = store
                .create_profile(&name)
                .map_err(|error| format!("Failed to create profile '{name}': {error}"))?;
            writeln!(output, "Created profile: {}", profile.name)?;
        }
        ConfigCommand::Edit { name } => {
            let profile = store
                .profile(&name)
                .map_err(|error| format!("Failed to load profile '{name}': {error}"))?;
            match edit_profile_json(&profile.settings_json)? {
                EditOutcome::Saved(edited) => {
                    let profile = store.update_profile_json(&name, &edited).map_err(|error| {
                        format!("Failed to update profile '{}': {error}", profile.name)
                    })?;
                    writeln!(output, "Updated profile: {}", profile.name)?;
                }
                EditOutcome::NoEditorConfigured => {
                    return Err("$EDITOR is not set or is empty; profile was not changed".into());
                }
                EditOutcome::EditorExitedWithError(message) => return Err(message.into()),
                EditOutcome::ValidationError { error, temp_file } => {
                    return Err(format!(
                        "Invalid profile JSON: {error}. Edits preserved in {}",
                        temp_file.display()
                    )
                    .into());
                }
            }
        }
        ConfigCommand::Delete { name } => {
            store
                .delete_profile(&name)
                .map_err(|error| format!("Failed to delete profile '{name}': {error}"))?;
            writeln!(output, "Deleted profile: {}", name.to_ascii_lowercase())?;
        }
        ConfigCommand::Activate { name } => {
            let path = store
                .activate_profile(&name)
                .map_err(|error| format!("Failed to activate profile '{name}': {error}"))?;
            writeln!(
                output,
                "Activated profile '{}' at {}",
                name.to_ascii_lowercase(),
                path.display()
            )?;
        }
        ConfigCommand::Bind {
            project_path,
            profile_name,
        } => {
            let cwd = std::path::PathBuf::from(&project_path);
            if !cwd.is_dir() {
                return Err(format!("Project path does not exist: {project_path}").into());
            }
            store
                .bind_profile(&cwd, &profile_name)
                .map_err(|error| format!("Failed to bind profile: {error}"))?;
            writeln!(output, "Bound profile '{profile_name}' to {project_path}")?;
        }
        ConfigCommand::Unbind { project_path } => {
            let cwd = std::path::PathBuf::from(&project_path);
            store
                .unbind_profile(&cwd)
                .map_err(|error| format!("Failed to unbind profile: {error}"))?;
            writeln!(output, "Unbound profile from {project_path}")?;
        }
        ConfigCommand::Copy { source, new_name } => {
            let copied = store
                .copy_profile(&source, &new_name)
                .map_err(|error| format!("Failed to copy profile: {error}"))?;
            writeln!(output, "Copied profile '{}' to '{}'", source, copied.name)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{handle_config_with_writer, parse_config_args};
    use crate::cmd::{CommandMode, ConfigCommand};
    use cowboy::claude_env::ClaudeEnvStore;

    fn parse(args: &[&str]) -> Result<CommandMode, String> {
        parse_config_args(args.iter().map(|arg| (*arg).to_string()))
    }

    #[test]
    fn parses_profile_commands() {
        assert_eq!(
            parse(&["list"]).unwrap(),
            CommandMode::Config(ConfigCommand::List)
        );
        assert_eq!(
            parse(&["create", "Work_Profile"]).unwrap(),
            CommandMode::Config(ConfigCommand::Create {
                name: "Work_Profile".to_string()
            })
        );
        assert_eq!(
            parse(&["edit", "work"]).unwrap(),
            CommandMode::Config(ConfigCommand::Edit {
                name: "work".to_string()
            })
        );
        assert_eq!(
            parse(&["delete", "work"]).unwrap(),
            CommandMode::Config(ConfigCommand::Delete {
                name: "work".to_string()
            })
        );
        assert_eq!(
            parse(&["activate", "work"]).unwrap(),
            CommandMode::Config(ConfigCommand::Activate {
                name: "work".to_string()
            })
        );
        assert_eq!(
            parse(&["bind", "/my/project", "work"]).unwrap(),
            CommandMode::Config(ConfigCommand::Bind {
                project_path: "/my/project".to_string(),
                profile_name: "work".to_string()
            })
        );
        assert_eq!(
            parse(&["unbind", "/my/project"]).unwrap(),
            CommandMode::Config(ConfigCommand::Unbind {
                project_path: "/my/project".to_string()
            })
        );
        assert_eq!(
            parse(&["copy", "work", "work-debug"]).unwrap(),
            CommandMode::Config(ConfigCommand::Copy {
                source: "work".to_string(),
                new_name: "work-debug".to_string()
            })
        );
    }

    #[test]
    fn rejects_history_subcommand() {
        for args in [
            vec!["history"],
            vec!["history", "list"],
            vec!["history", "show", "1"],
            vec!["history", "activate", "1"],
        ] {
            assert!(parse(&args).is_err(), "accepted invalid args: {args:?}");
        }
    }

    #[test]
    fn rejects_missing_unknown_and_extra_arguments() {
        for args in [
            vec![],
            vec!["unknown"],
            vec!["list", "extra"],
            vec!["create"],
            vec!["create", "one", "two"],
            vec!["bind"],
            vec!["bind", "/path/to/project"],
            vec!["bind", "/path/to/project", "work", "extra"],
            vec!["unbind"],
            vec!["unbind", "/path/to/project", "extra"],
            vec!["copy"],
            vec!["copy", "work"],
            vec!["copy", "work", "work-debug", "extra"],
        ] {
            assert!(parse(&args).is_err(), "accepted invalid args: {args:?}");
        }
    }

    #[test]
    fn profile_handlers_create_list_and_delete() {
        let (_temp, store) = initialized_store();
        let mut output = Vec::new();

        handle_config_with_writer(
            &store,
            ConfigCommand::Create {
                name: "Work_Profile".to_string(),
            },
            &mut output,
        )
        .unwrap();
        handle_config_with_writer(&store, ConfigCommand::List, &mut output).unwrap();
        handle_config_with_writer(
            &store,
            ConfigCommand::Delete {
                name: "WORK_PROFILE".to_string(),
            },
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Created profile: work_profile"));
        assert!(output.lines().any(|line| line == "work_profile"));
        assert!(output.contains("Deleted profile: work_profile"));
        assert!(store.list_profiles().unwrap().is_empty());
    }

    #[test]
    fn bind_handler_creates_binding_and_reports_success() {
        let (temp, store) = initialized_store();
        store.create_profile("work").unwrap();
        let project_dir = temp.path().join("my-project");
        std::fs::create_dir(&project_dir).unwrap();
        let mut output = Vec::new();

        handle_config_with_writer(
            &store,
            ConfigCommand::Bind {
                project_path: project_dir.display().to_string(),
                profile_name: "work".to_string(),
            },
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Bound profile 'work'"));
        let binding = store.project_binding(&project_dir).unwrap();
        assert_eq!(binding.unwrap().profile_name, "work");
    }

    #[test]
    fn bind_handler_rejects_nonexistent_project_path() {
        let (_temp, store) = initialized_store();
        store.create_profile("work").unwrap();
        let mut output = Vec::new();

        let result = handle_config_with_writer(
            &store,
            ConfigCommand::Bind {
                project_path: "/nonexistent/path".to_string(),
                profile_name: "work".to_string(),
            },
            &mut output,
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Project path does not exist"));
    }

    #[test]
    fn unbind_handler_removes_binding_and_reports_success() {
        let (temp, store) = initialized_store();
        store.create_profile("work").unwrap();
        let project_dir = temp.path().join("my-project");
        std::fs::create_dir(&project_dir).unwrap();
        store.bind_profile(&project_dir, "work").unwrap();
        let mut output = Vec::new();

        handle_config_with_writer(
            &store,
            ConfigCommand::Unbind {
                project_path: project_dir.display().to_string(),
            },
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Unbound profile from"));
        assert!(store.project_binding(&project_dir).unwrap().is_none());
    }

    #[test]
    fn unbind_handler_fails_when_no_binding_exists() {
        let (temp, store) = initialized_store();
        let project_dir = temp.path().join("my-project");
        std::fs::create_dir(&project_dir).unwrap();
        let mut output = Vec::new();

        let result = handle_config_with_writer(
            &store,
            ConfigCommand::Unbind {
                project_path: project_dir.display().to_string(),
            },
            &mut output,
        );

        assert!(result.is_err());
    }

    #[test]
    fn copy_handler_creates_profile_and_rejects_duplicate_name() {
        let (_temp, store) = initialized_store();
        let mut output = Vec::new();

        // Create source profile with custom settings
        handle_config_with_writer(
            &store,
            ConfigCommand::Create {
                name: "work".to_string(),
            },
            &mut output,
        )
        .unwrap();
        store
            .update_profile_json("work", r#"{"env":{"KEY":"value"}}"#)
            .unwrap();

        // Copy to a new name
        handle_config_with_writer(
            &store,
            ConfigCommand::Copy {
                source: "work".to_string(),
                new_name: "work-debug".to_string(),
            },
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Copied profile"));
        assert!(output.contains("work-debug"));

        // Verify the copy has identical JSON
        let copied = store.profile("work-debug").unwrap();
        assert_eq!(copied.settings_json, r#"{"env":{"KEY":"value"}}"#);

        // Copy to same name as existing profile should fail
        let mut fail_output = Vec::new();
        let result = handle_config_with_writer(
            &store,
            ConfigCommand::Copy {
                source: "work".to_string(),
                new_name: "work".to_string(),
            },
            &mut fail_output,
        );
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("profile already exists"));
    }

    fn initialized_store() -> (tempfile::TempDir, ClaudeEnvStore) {
        let temp = tempfile::tempdir().unwrap();
        let store = ClaudeEnvStore::new(temp.path().join("cowboy.db"));
        store.initialize().unwrap();
        (temp, store)
    }
}
