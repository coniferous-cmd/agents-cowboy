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
    app::{DeleteCompletion, DeleteOutcome, DeleteTarget, LoadCompletion, LoadRequest, Stetson},
    application::{AgentBackend, ClaudeBackend, ClaudeCliLauncher, PiBackend, SessionRepository},
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
    let store = ClaudeEnvStore::from_home()
        .map_err(|error| format!("Failed to initialize Claude env metadata path: {error}"))?;
    let env_store = initialize_env_store(store)?;

    match command {
        cmd::CommandMode::Tui => run_tui_app(env_store),
        cmd::CommandMode::Help => Ok(()),
        cmd::CommandMode::Alias(command) => cmd::handle_alias(&env_store, command),
    }
}

fn initialize_env_store(
    env_store: ClaudeEnvStore,
) -> std::result::Result<ClaudeEnvStore, Box<dyn Error>> {
    env_store
        .initialize()
        .map_err(|error| format!("Failed to initialize Claude env metadata store: {error}"))?;
    env_store
        .seed_default_settings()
        .map_err(|error| format!("Failed to seed cowboy settings: {error}"))?;
    env_store
        .seed_default_theme()
        .map_err(|error| format!("Failed to seed default theme: {error}"))?;
    Ok(env_store)
}

#[cfg(test)]
use crate::app::InitialLoadCompletion;

type WorkerBackend = std::sync::Arc<dyn AgentBackend + Send + Sync>;
fn registered_backends(claude: ClaudeBackend, pi: PiBackend) -> (Stetson, Vec<WorkerBackend>) {
    let workers: Vec<WorkerBackend> = vec![
        std::sync::Arc::new(claude.clone()),
        std::sync::Arc::new(pi.clone()),
    ];
    (Stetson::new(vec![Box::new(claude), Box::new(pi)]), workers)
}

fn run_tui_app(env_store: ClaudeEnvStore) -> std::result::Result<(), Box<dyn Error>> {
    let projects_root = env_store
        .claude_projects_dir()
        .map_err(|error| format!("Failed to resolve Claude projects directory: {error}"))?;
    let repository = ClaudeProjectsStore::new(projects_root);
    let launcher = ClaudeCliLauncher::new(env_store.clone());
    // Ordered agent-strategy registry: one registered strategy = one tab.
    let pi = cowboy::pi::PiProjectsStore::from_home()
        .map_err(|e| format!("Failed to resolve Pi sessions directory: {e}"))?;
    let (mut app, worker_backends) =
        registered_backends(ClaudeBackend::new(repository, launcher), PiBackend::new(pi));

    if let Ok(Some(theme)) = env_store.active_theme() {
        app.set_theme(ThemePalette::from(&theme));
    }

    app.enable_project_loading();
    let (load_tx, load_rx) = mpsc::channel::<LoadCompletion>();

    loop {
        let mut terminal = enter_tui()?;
        let tui_result = run_tui(
            &mut terminal,
            &mut app,
            &worker_backends,
            &load_tx,
            &load_rx,
        );
        let restore_result = leave_tui(&mut terminal);

        match (tui_result, restore_result) {
            (Err(error), _) => return Err(error),
            (Ok(()), Err(error)) => return Err(error),
            (Ok(()), Ok(())) => {}
        }

        let Some(target) = app.take_pending_resume() else {
            if let Some(cwd) = app.take_pending_new_session() {
                match app.selected_backend().launch_new(&cwd) {
                    Ok(()) => {
                        app.resume_finished_for(
                            &cwd,
                            format!("Session exited, launched from {}", cwd.display()),
                        );
                    }
                    Err(error) => {
                        app.resume_finished_for(&cwd, format!("Failed to launch session: {error}"));
                    }
                }
                continue;
            }
            break;
        };

        match app.selected_backend().resume(&target) {
            Ok(()) => {
                app.resume_finished_for(
                    &target.cwd,
                    format!(
                        "{} exited: {}",
                        app.selected_backend().label(),
                        target.key.native_id
                    ),
                );
            }
            Err(error) => {
                app.resume_finished_for(
                    &target.cwd,
                    format!("Failed to resume session {}: {error}", target.key.native_id),
                );
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

fn run_tui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut Stetson,
    worker_backends: &[WorkerBackend],
    load_tx: &mpsc::Sender<LoadCompletion>,
    load_rx: &mpsc::Receiver<LoadCompletion>,
) -> std::result::Result<(), Box<dyn Error>> {
    let (delete_tx, delete_rx) = mpsc::channel::<DeleteCompletion>();
    let render_clock = std::time::Instant::now();

    loop {
        while let Ok(completion) = load_rx.try_recv() {
            app.complete_project_load(completion);
        }
        if let Some(request) = app.take_load_request() {
            spawn_project_load_worker(
                worker_backends[request.agent].clone(),
                load_tx.clone(),
                request,
            );
        }

        // Drain any completed worker results and apply them to the app.
        while let Ok(completion) = delete_rx.try_recv() {
            app.complete_delete(completion);
        }

        // Start a confirmed deletion on a worker thread (at most one at a time;
        // `take_pending_delete` yields each queued task exactly once).
        if let Some(target) = app.take_pending_delete() {
            spawn_delete_worker(worker_backends.to_vec(), delete_tx.clone(), target);
        }

        let tick = (render_clock.elapsed().as_millis() / 120) as usize;
        terminal.draw(|frame| ui::render(frame, app.state(), app.load_feedback(), tick))?;

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
            }
        }
    }
}

fn spawn_project_load_worker(
    backend: WorkerBackend,
    sender: mpsc::Sender<LoadCompletion>,
    request: LoadRequest,
) {
    let on_failure = request.clone();
    let result_sender = sender.clone();
    if let Err(error) = thread::Builder::new()
        .name("cowboy-project-load".into())
        .spawn(move || {
            let _ = result_sender.send(request.run(backend.as_ref()));
        })
    {
        let _ = sender.send(on_failure.failed(format!("Failed to start project load: {error}")));
    }
}

#[cfg(test)]
fn spawn_initial_load_worker<R>(repository: R, sender: mpsc::Sender<InitialLoadCompletion>)
where
    R: SessionRepository + Send + 'static,
{
    let error_sender = sender.clone();
    let spawn_result = thread::Builder::new()
        .name("cowboy-initial-load".to_string())
        .spawn(move || {
            let projects = repository.load_projects();
            let _ = sender.send(InitialLoadCompletion { projects });
        });

    if let Err(error) = spawn_result {
        let message = format!("Failed to spawn initial load worker: {error}");
        let _ = error_sender.send(InitialLoadCompletion {
            projects: Err(message),
        });
    }
}

/// Run a deletion on a named standard-library thread and report the outcome
/// back through `sender`. If the thread cannot be spawned, an error completion
/// is fed immediately so the app never stays locked.
fn spawn_delete_worker(
    workers: Vec<WorkerBackend>,
    sender: mpsc::Sender<DeleteCompletion>,
    target: DeleteTarget,
) {
    let error_sender = sender.clone();
    let error_target = target.clone();

    let spawn_result = thread::Builder::new()
        .name("cowboy-delete".to_string())
        .spawn(move || {
            let outcome = match workers.iter().find(|b| &b.id() == target.agent_id()) {
                Some(repository) => run_delete(repository, &target),
                None => {
                    DeleteOutcome::Failure(format!("Unregistered agent: {}", target.agent_id()))
                }
            };
            let _ = sender.send(DeleteCompletion { target, outcome });
        });

    if let Err(error) = spawn_result {
        let _ = error_sender.send(DeleteCompletion {
            target: error_target,
            outcome: DeleteOutcome::Failure(format!("Failed to spawn delete worker: {error}")),
        });
    }
}

fn run_delete(repository: &dyn SessionRepository, target: &DeleteTarget) -> DeleteOutcome {
    let result = match target {
        DeleteTarget::Project { cwd, .. } => SessionRepository::delete_project(repository, cwd),
        DeleteTarget::Session { key, .. } => SessionRepository::delete_session(repository, key),
    };

    match result {
        Ok(()) => DeleteOutcome::Applied,
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
mod metadata_tests;

#[cfg(test)]
mod startup_tests {
    use super::*;
    use cowboy::domain::{Project, SessionKey};
    use std::path::Path;
    use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
    use std::time::Duration;

    #[test]
    fn startup_registration_and_deletion_route_by_agent() {
        use cowboy::domain::AgentId;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let claude = ClaudeBackend::new(
            ClaudeProjectsStore::new(root.join("claude")),
            ClaudeCliLauncher::new(ClaudeEnvStore::new(root.join("db"))),
        );
        let dir = root.join("pi/bucket");
        std::fs::create_dir_all(&dir).unwrap();
        for id in ["keep", "delete"] {
            std::fs::write(
                dir.join(format!("{id}.jsonl")),
                serde_json::json!({"type":"session","version":3,"id":id,"cwd":"pi-only"})
                    .to_string(),
            )
            .unwrap();
        }
        let (app, workers) = registered_backends(
            claude,
            PiBackend::new(cowboy::pi::PiProjectsStore::new(root.join("pi"))),
        );
        assert_eq!(
            app.state()
                .agent_tabs
                .iter()
                .map(|t| t.label.as_str())
                .collect::<Vec<_>>(),
            ["Claude", "Pi"]
        );
        for agent in ["unknown", "pi"] {
            let (tx, rx) = mpsc::channel();
            spawn_delete_worker(
                workers.clone(),
                tx,
                DeleteTarget::Session {
                    title: "target".into(),
                    key: SessionKey::new(AgentId::new(agent).unwrap(), "delete").unwrap(),
                },
            );
            let o = rx.recv_timeout(Duration::from_secs(5)).unwrap().outcome;
            match o {
                DeleteOutcome::Failure(e) => {
                    assert_eq!(agent, "unknown");
                    assert!(e.contains("unknown"));
                    assert!(dir.join("delete.jsonl").exists());
                }
                DeleteOutcome::Applied => {
                    assert_eq!(agent, "pi");
                    assert!(dir.join("keep.jsonl").exists());
                    assert!(!dir.join("delete.jsonl").exists());
                }
                DeleteOutcome::Success(_) => panic!("worker must not reload agent snapshots"),
            }
        }
    }

    #[test]
    fn worker_calls_only_the_matching_backend_without_reloading_sessions() {
        use crate::application::{AppResult, ResumeLauncher};
        use cowboy::domain::AgentId;
        use std::sync::{Arc, Mutex};
        struct Probe(&'static str, Mutex<Vec<&'static str>>);
        impl AgentBackend for Probe {
            fn id(&self) -> AgentId {
                AgentId::new(self.0).unwrap()
            }
            fn label(&self) -> &str {
                self.0
            }
        }
        impl ResumeLauncher for Probe {
            fn resume(&self, _: &app::ResumeTarget) -> AppResult<()> {
                unreachable!()
            }
            fn launch_new(&self, _: &Path) -> AppResult<()> {
                unreachable!()
            }
        }
        impl SessionRepository for Probe {
            fn load_projects(&self) -> AppResult<Vec<Project>> {
                self.1.lock().unwrap().push("load");
                Ok(vec![])
            }
            fn delete_project(&self, _: &Path) -> AppResult<()> {
                self.1.lock().unwrap().push("delete");
                Ok(())
            }
            fn rename_session(&self, _: &SessionKey, _: &str) -> AppResult<()> {
                unreachable!()
            }
            fn delete_session(&self, _: &SessionKey) -> AppResult<()> {
                unreachable!()
            }
        }
        let claude = Arc::new(Probe("claude", Mutex::default()));
        let pi = Arc::new(Probe("pi", Mutex::default()));
        let (tx, rx) = mpsc::channel();
        spawn_delete_worker(
            vec![claude.clone(), pi.clone()],
            tx,
            DeleteTarget::Project {
                agent_id: pi.id(),
                name: "p".into(),
                cwd: "p".into(),
            },
        );
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().outcome,
            DeleteOutcome::Applied
        ));
        assert!(claude.1.lock().unwrap().is_empty());
        assert_eq!(*pi.1.lock().unwrap(), ["delete"]);
    }

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

        spawn_initial_load_worker(repository, completion_tx);

        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(completion_rx.try_recv(), Err(TryRecvError::Empty)));
        release_tx.send(()).unwrap();
        let completion = completion_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(completion.projects.is_ok());
    }
}
