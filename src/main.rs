mod app;
mod application;
mod cmd;
mod ui;

use std::{env, error::Error, io, sync::mpsc, thread, time::Duration};

use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use cowboy::{
    claude_env::ClaudeEnvStore, infrastructure::ClaudeProjectsStore, theme::ThemePalette,
};

use crate::{
    app::{DeleteCompletion, DeleteOutcome, DeleteTarget, InitialLoadCompletion, Stetson},
    application::{
        ClaudeCliLauncher, ProfileRepository, ResumeLauncher, SessionRepository, StetsonApplication,
    },
};

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

fn run() -> std::result::Result<(), Box<dyn Error>> {
    let command = cmd::parse_cli_args(env::args().skip(1))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if command == cmd::CommandMode::Help {
        cmd::print_help();
        return Ok(());
    }
    let env_store = initialize_env_store()?;

    match command {
        cmd::CommandMode::Tui => run_tui_app(env_store),
        cmd::CommandMode::Help => Ok(()),
        cmd::CommandMode::Config(command) => cmd::handle_config(&env_store, command),
        cmd::CommandMode::Alias(command) => cmd::handle_alias(&env_store, command),
        cmd::CommandMode::Install => cmd::handle_install(&env_store),
    }
}

fn initialize_env_store() -> std::result::Result<ClaudeEnvStore, Box<dyn Error>> {
    let env_store = ClaudeEnvStore::from_home()
        .map_err(|error| format!("Failed to initialize Claude env metadata path: {error}"))?;
    env_store
        .initialize()
        .map_err(|error| format!("Failed to initialize Claude env metadata store: {error}"))?;
    env_store
        .seed_default_settings()
        .map_err(|error| format!("Failed to seed cowboy settings: {error}"))?;
    env_store
        .seed_default_theme()
        .map_err(|error| format!("Failed to seed default theme: {error}"))?;
    match env_store
        .recover_profile_activation()
        .map_err(|error| format!("Failed to recover profile activation: {error}"))?
    {
        cowboy::claude_env::RecoveryOutcome::Failed(error)
        | cowboy::claude_env::RecoveryOutcome::PreviouslyFailed(error) => {
            eprintln!("Warning: profile activation recovery failed: {error}");
        }
        _ => {}
    }
    Ok(env_store)
}

fn run_tui_app(env_store: ClaudeEnvStore) -> std::result::Result<(), Box<dyn Error>> {
    let projects_root = env_store
        .claude_projects_dir()
        .map_err(|error| format!("Failed to resolve Claude projects directory: {error}"))?;
    let repository = ClaudeProjectsStore::new(projects_root);
    let worker_repository = repository.clone();
    let launcher = ClaudeCliLauncher::new(env_store.clone());
    let application =
        StetsonApplication::with_profiles(repository, launcher.clone(), env_store.clone());
    let mut app = Stetson::new(application);

    if let Ok(Some(theme)) = env_store.active_theme() {
        app.set_theme(ThemePalette::from(&theme));
    }

    let (initial_load_tx, initial_load_rx) = mpsc::channel::<InitialLoadCompletion>();
    spawn_initial_load_worker(
        worker_repository.clone(),
        env_store.clone(),
        initial_load_tx,
    );

    loop {
        let mut terminal = enter_tui()?;
        let tui_result = run_tui(
            &mut terminal,
            &mut app,
            &worker_repository,
            &initial_load_rx,
        );
        let restore_result = leave_tui(&mut terminal);

        match (tui_result, restore_result) {
            (Err(error), _) => return Err(error),
            (Ok(()), Err(error)) => return Err(error),
            (Ok(()), Ok(())) => {}
        }

        let Some(target) = app.take_pending_resume() else {
            if let Some(cwd) = app.take_pending_new_session() {
                let override_path = app.take_pending_profile_override();
                let launch_result = if let Some(profile_path) = override_path {
                    launcher.launch_new_with_override(&cwd, &profile_path)
                } else {
                    launcher.launch_new(&cwd)
                };
                match launch_result {
                    Ok(()) => {
                        app.resume_finished(format!(
                            "Session exited, launched from {}",
                            cwd.display()
                        ));
                    }
                    Err(error) => {
                        app.resume_finished(format!("Failed to launch session: {error}"));
                    }
                }
                continue;
            }
            if let Some(name) = app.take_pending_profile_edit() {
                let outcome = app.run_new_profile_editor(&name);
                app.profile_edit_finished(outcome);
                continue;
            }
            break;
        };

        match launcher.resume(&target) {
            Ok(()) => {
                app.resume_finished(format!("Claude exited: {}", target.key.native_id));
            }
            Err(error) => {
                app.resume_finished(format!(
                    "Failed to resume session {}: {error}",
                    target.key.native_id
                ));
            }
        }
    }

    Ok(())
}

fn enter_tui() -> std::result::Result<Terminal<CrosstermBackend<io::Stdout>>, Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn run_tui<R, L, P>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut Stetson<R, L, P>,
    worker_repository: &ClaudeProjectsStore,
    initial_load_rx: &mpsc::Receiver<InitialLoadCompletion>,
) -> std::result::Result<(), Box<dyn Error>>
where
    R: crate::application::SessionRepository,
    L: ResumeLauncher,
    P: ProfileRepository,
{
    let (delete_tx, delete_rx) = mpsc::channel::<DeleteCompletion>();

    loop {
        while let Ok(completion) = initial_load_rx.try_recv() {
            app.complete_initial_load(completion);
        }

        // Drain any completed worker results and apply them to the app.
        while let Ok(completion) = delete_rx.try_recv() {
            app.complete_delete(completion);
        }

        // Start a confirmed deletion on a worker thread (at most one at a time;
        // `take_pending_delete` yields each queued task exactly once).
        if let Some(target) = app.take_pending_delete() {
            spawn_delete_worker(worker_repository.clone(), delete_tx.clone(), target);
        }

        terminal.draw(|frame| ui::render(frame, app.state()))?;

        // Check for a permitted exit only after processing worker results, so a
        // completion can unlock the UI before we test `should_quit`.
        if app.should_quit() {
            return Ok(());
        }

        // Timed poll keeps the loop responsive to worker completions without
        // waiting for the next keypress.
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                app.handle_key(key);
                if app.take_terminal_refresh_request() {
                    // An external editor changed the physical screen while
                    // ratatui retained its previous-frame cache. Clear both so
                    // the next draw repaints every cell.
                    terminal.clear()?;
                }
            }
        }
    }
}

fn spawn_initial_load_worker<R, P>(
    repository: R,
    profiles: P,
    sender: mpsc::Sender<InitialLoadCompletion>,
) where
    R: SessionRepository + Send + 'static,
    P: ProfileRepository + Send + 'static,
{
    let error_sender = sender.clone();
    let spawn_result = thread::Builder::new()
        .name("cowboy-initial-load".to_string())
        .spawn(move || {
            let projects = repository.load_projects();
            let profiles = (|| {
                Ok((
                    profiles.list_profiles()?,
                    profiles.list_snapshots()?,
                    profiles.active_profile_name()?,
                ))
            })();
            let _ = sender.send(InitialLoadCompletion { projects, profiles });
        });

    if let Err(error) = spawn_result {
        let message = format!("Failed to spawn initial load worker: {error}");
        let _ = error_sender.send(InitialLoadCompletion {
            projects: Err(message.clone()),
            profiles: Err(message),
        });
    }
}

/// Run a deletion on a named standard-library thread and report the outcome
/// back through `sender`. If the thread cannot be spawned, an error completion
/// is fed immediately so the app never stays locked.
fn spawn_delete_worker(
    repository: ClaudeProjectsStore,
    sender: mpsc::Sender<DeleteCompletion>,
    target: DeleteTarget,
) {
    let error_sender = sender.clone();
    let error_target = target.clone();

    let spawn_result = thread::Builder::new()
        .name("cowboy-delete".to_string())
        .spawn(move || {
            let outcome = run_delete(&repository, &target);
            let _ = sender.send(DeleteCompletion { target, outcome });
        });

    if let Err(error) = spawn_result {
        let _ = error_sender.send(DeleteCompletion {
            target: error_target,
            outcome: DeleteOutcome::Failure(format!("Failed to spawn delete worker: {error}")),
        });
    }
}

fn run_delete(repository: &ClaudeProjectsStore, target: &DeleteTarget) -> DeleteOutcome {
    let result = match target {
        DeleteTarget::Project { cwd, .. } => SessionRepository::delete_project(repository, cwd),
        DeleteTarget::Session { key, .. } => SessionRepository::delete_session(repository, key),
        DeleteTarget::Profile { .. } => Err("Profile deletion must run in the TUI".to_string()),
    };

    match result {
        Ok(()) => match SessionRepository::load_projects(repository) {
            Ok(projects) => DeleteOutcome::Success(projects),
            Err(error) => DeleteOutcome::Failure(error),
        },
        Err(error) => DeleteOutcome::Failure(error),
    }
}

fn leave_tui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> std::result::Result<(), Box<dyn Error>> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    use crate::application::NoProfileRepository;
    use cowboy::domain::{Project, SessionKey};
    use std::path::Path;
    use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
    use std::time::Duration;

    struct BlockingRepository {
        started: SyncSender<()>,
        release: Receiver<()>,
    }

    impl SessionRepository for BlockingRepository {
        fn load_projects(&self) -> crate::application::AppResult<Vec<Project>> {
            self.started.send(()).unwrap();
            self.release.recv().unwrap();
            Ok(Vec::new())
        }

        fn rename_session(&self, _: &SessionKey, _: &str) -> crate::application::AppResult<()> {
            Ok(())
        }

        fn delete_session(&self, _: &SessionKey) -> crate::application::AppResult<()> {
            Ok(())
        }

        fn delete_project(&self, _: &Path) -> crate::application::AppResult<()> {
            Ok(())
        }
    }

    #[test]
    fn initial_load_worker_does_not_block_on_session_scan() {
        let (started_tx, started_rx) = mpsc::sync_channel(0);
        let (release_tx, release_rx) = mpsc::channel();
        let (completion_tx, completion_rx) = mpsc::channel();
        let repository = BlockingRepository {
            started: started_tx,
            release: release_rx,
        };

        spawn_initial_load_worker(repository, NoProfileRepository, completion_tx);

        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(completion_rx.try_recv(), Err(TryRecvError::Empty)));
        release_tx.send(()).unwrap();
        let completion = completion_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(completion.projects.is_ok());
    }
}
