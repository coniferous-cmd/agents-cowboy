use super::parser::{discover_session_cwd, invalid, parse};
use crate::{
    domain::{validate_rename_title, ProjectCatalog, SessionKey},
    group_sessions_by_project, Project, Result, Session, StetsonError,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, HashSet},
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};
#[derive(Clone)]
pub struct PiProjectsStore {
    root: PathBuf,
}
impl PiProjectsStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn from_home() -> Result<Self> {
        Ok(Self::new(
            dirs::home_dir()
                .ok_or(StetsonError::HomeDirectoryUnavailable)?
                .join(".pi/agent/sessions"),
        ))
    }
    fn files(&self) -> Result<Vec<(PathBuf, Session)>> {
        let mut out = Vec::new();
        self.visit_session_files(|path| {
            if let Some(session) = parse(&path)? {
                out.push((path, session));
            }
            Ok(())
        })?;
        Ok(out)
    }

    pub fn discover_projects(&self) -> Result<ProjectCatalog> {
        let mut catalog = ProjectCatalog::new();
        self.visit_session_files(|path| {
            if let Some(cwd) = discover_session_cwd(&path)? {
                catalog.entry(cwd).or_default().push(path);
            }
            Ok(())
        })?;
        Ok(catalog)
    }

    pub fn load_project(&self, cwd: &Path, sources: &[PathBuf]) -> Result<Project> {
        let mut sessions = Vec::new();
        for path in sources {
            if let Some(session) = parse(path)?.filter(|session| session.cwd == cwd) {
                sessions.push(session);
            }
        }
        Ok(group_sessions_by_project(sessions)
            .pop()
            .unwrap_or_else(|| Project {
                cwd: cwd.to_path_buf(),
                sessions: Vec::new(),
            }))
    }

    fn visit_session_files(&self, mut visit: impl FnMut(PathBuf) -> Result<()>) -> Result<()> {
        let dirs = match fs::read_dir(&self.root) {
            Ok(d) => d,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for dir in dirs {
            let dir = dir?;
            if !dir.file_type()?.is_dir() {
                continue;
            }
            for file in fs::read_dir(dir.path())? {
                let file = file?;
                let path = file.path();
                if file.file_type()?.is_file() && path.extension().is_some_and(|x| x == "jsonl") {
                    visit(path)?;
                }
            }
        }
        Ok(())
    }
    pub fn discover_sessions(&self) -> Result<Vec<Session>> {
        Ok(self.files()?.into_iter().map(|(_, s)| s).collect())
    }
    pub fn load_projects(&self) -> Result<Vec<Project>> {
        Ok(group_sessions_by_project(self.discover_sessions()?))
    }
    fn find(&self, key: &SessionKey) -> Result<PathBuf> {
        self.files()?
            .into_iter()
            .find(|(_, s)| s.key == *key)
            .map(|(p, _)| p)
            .ok_or_else(|| StetsonError::SessionNotFound(key.native_id.clone()))
    }
    pub fn delete_session(&self, key: &SessionKey) -> Result<()> {
        Ok(fs::remove_file(self.find(key)?)?)
    }
    pub fn delete_project(&self, cwd: &Path) -> Result<()> {
        let files: Vec<_> = self
            .files()?
            .into_iter()
            .filter(|(_, s)| s.cwd == cwd)
            .map(|(p, _)| p)
            .collect();
        if files.is_empty() {
            return Err(StetsonError::ProjectNotFound(cwd.display().to_string()));
        }
        let mut parents = BTreeSet::new();
        for file in files {
            fs::remove_file(&file)?;
            if let Some(parent) = file.parent() {
                parents.insert(parent.to_owned());
            }
        }
        for parent in parents {
            match fs::remove_dir(parent) {
                Ok(()) => (),
                Err(e) if e.kind() == ErrorKind::DirectoryNotEmpty => (),
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    pub fn rename_session(&self, key: &SessionKey, title: &str) -> Result<()> {
        let path = self.find(key)?;
        let title = validate_rename_title(title).map_err(|e| invalid(&path, e))?;
        let raw = fs::read_to_string(&path)?;
        let mut lines: Vec<String> = raw.lines().map(str::to_owned).collect();
        let mut latest = None;
        let mut parent = None;
        let mut ids = HashSet::new();
        for (i, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(line)?;
            if v["type"] == "session_info" {
                latest = Some((i, v.clone()));
            }
            parent = v["id"].as_str().map(str::to_owned);
            if let Some(id) = &parent {
                ids.insert(id.clone());
            }
        }
        if let Some((i, mut v)) = latest {
            v["name"] = json!(title);
            lines[i] = v.to_string();
        } else {
            let now = chrono::Utc::now();
            let mut seed = now.timestamp_subsec_nanos();
            let id = loop {
                let candidate = format!("{seed:08x}");
                if !ids.contains(&candidate) {
                    break candidate;
                }
                seed = seed.wrapping_add(1);
            };
            lines.push(json!({"type":"session_info","id":id,"parentId":parent,"timestamp":now.to_rfc3339(),"name":title}).to_string());
        }
        let tmp = path.with_extension("jsonl.tmp");
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)?;
        let result = (|| -> std::io::Result<()> {
            for line in lines {
                writeln!(f, "{line}")?;
            }
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(tmp);
        }
        Ok(result?)
    }
}
