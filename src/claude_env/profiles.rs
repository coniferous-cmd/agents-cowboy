use crate::domain::{ProjectProfileBinding, Result, StetsonError};
use fs2::FileExt;
use rusqlite::{params, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::settings::SETTING_CLAUDE_CONFIG_DIR;
use super::ClaudeEnvStore;

const ACTIVE_PROFILE_KEY: &str = "active_profile_name";
const INITIAL_BACKUP_DONE_KEY: &str = "initial_backup_done";
const INITIAL_BACKUP_FILENAME: &str = "settings.json.cowboy-backup";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeProfile {
    pub id: i64,
    pub name: String,
    pub settings_json: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    NoPending,
    Recovered {
        target_kind: String,
        target_id: String,
    },
    Failed(String),
    PreviouslyFailed(String),
}

pub fn validate_profile_name(name: &str) -> Result<String> {
    if name.is_empty() || name.len() > 64 {
        return Err(StetsonError::InvalidProfileName(
            "name must contain 1 to 64 ASCII characters".to_string(),
        ));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(StetsonError::InvalidProfileName(
            "only ASCII letters, digits, '-' and '_' are allowed".to_string(),
        ));
    }
    Ok(name.to_ascii_lowercase())
}

pub fn validate_settings_json(raw: &str) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| StetsonError::InvalidSettingsFile(error.to_string()))?;
    if !value.is_object() {
        return Err(StetsonError::InvalidSettingsFile(
            "settings root must be a JSON object".to_string(),
        ));
    }
    Ok(())
}

/// Walk `profiles_dir` and return the `<name>` portion of every file that
/// matches the pattern `settings.<name>.json`. The list is sorted
/// alphabetically so callers can produce deterministic output. Files with
/// names that do not match the pattern are silently ignored; their validation
/// (if any) happens later in `reconcile_one`.
fn discover_profile_names(profiles_dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(profiles_dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(as_str) = file_name.to_str() else {
            continue;
        };
        let Some(stripped) = as_str
            .strip_prefix("settings.")
            .and_then(|s| s.strip_suffix(".json"))
        else {
            continue;
        };
        if stripped.is_empty() {
            continue;
        }
        names.push(stripped.to_string());
    }
    names.sort();
    Ok(names)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub inserted: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    pub invalid: Vec<InvalidEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidEntry {
    pub name: String,
    pub error: String,
}

impl SyncReport {
    pub fn is_empty(&self) -> bool {
        self.inserted.is_empty()
            && self.updated.is_empty()
            && self.unchanged.is_empty()
            && self.invalid.is_empty()
    }
}

pub struct AtomicReplace;

impl AtomicReplace {
    pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
        let parent = path.parent().ok_or_else(|| {
            StetsonError::InvalidSettingsFile("target has no parent directory".to_string())
        })?;
        ensure_private_dir(parent)?;
        let mode = target_mode(path)?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp = parent.join(format!(
            ".{}.tmp.{}.{nonce}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            std::process::id()
        ));

        let result = (|| -> Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(mode.unwrap_or(0o600));
            }
            let mut file = options.open(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            #[cfg(unix)]
            if let Some(mode) = mode {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
            }
            replace_file(&tmp, path)?;
            sync_directory(parent)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

impl ClaudeEnvStore {
    pub fn active_profile_name(&self) -> Result<Option<String>> {
        self.get_setting(ACTIVE_PROFILE_KEY)
    }

    pub fn list_profiles(&self) -> Result<Vec<ClaudeProfile>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id,name,settings_json,updated_at FROM claude_profiles ORDER BY name ASC",
        )?;
        let rows = statement.query_map([], profile_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn create_profile(&self, name: &str) -> Result<ClaudeProfile> {
        let name = validate_profile_name(name)?;
        let target = self.profile_file_path(&name)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        match transaction.execute(
            "INSERT INTO claude_profiles (name,settings_json) VALUES (?1,'{}')",
            [&name],
        ) {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(StetsonError::ProfileExists(name));
            }
            Err(error) => return Err(error.into()),
        }
        // On file-write failure the transaction is dropped without commit,
        // rolling back the INSERT.
        AtomicReplace::write(&target, b"{}")?;
        transaction.commit()?;
        drop(connection);
        self.profile(&name)
    }

    pub fn profile(&self, name: &str) -> Result<ClaudeProfile> {
        let name = validate_profile_name(name)?;
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT id,name,settings_json,updated_at FROM claude_profiles WHERE name=?1",
                [&name],
                profile_from_row,
            )
            .optional()?
            .ok_or(StetsonError::ProfileNotFound(name))
    }

    pub fn update_profile_json(&self, name: &str, settings_json: &str) -> Result<ClaudeProfile> {
        let name = validate_profile_name(name)?;
        validate_settings_json(settings_json)?;
        let target = self.profile_file_path(&name)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE claude_profiles SET settings_json=?1,updated_at=CURRENT_TIMESTAMP WHERE name=?2",
            params![settings_json, name],
        )?;
        if changed == 0 {
            return Err(StetsonError::ProfileNotFound(name));
        }
        // On file-write failure the transaction is dropped without commit,
        // rolling back the UPDATE.
        AtomicReplace::write(&target, settings_json.as_bytes())?;
        transaction.commit()?;
        drop(connection);
        self.profile(&name)
    }

    pub fn delete_profile(&self, name: &str) -> Result<()> {
        let name = validate_profile_name(name)?;
        let _lock = self.activation_lock()?;
        let target = self.profile_file_path(&name)?;
        let link = self.global_settings_path()?;
        let was_active;
        {
            let mut connection = self.connection()?;
            let transaction = connection.transaction()?;
            let changed = transaction
                .execute("DELETE FROM claude_profiles WHERE name=?1", [&name])
                .map_err(|error| match error {
                    rusqlite::Error::SqliteFailure(error, _)
                        if error.code == rusqlite::ErrorCode::ConstraintViolation =>
                    {
                        StetsonError::ProfileInUse(name.clone())
                    }
                    error => error.into(),
                })?;
            if changed == 0 {
                return Err(StetsonError::ProfileNotFound(name));
            }
            was_active = transaction.execute(
                "DELETE FROM settings WHERE key=?1 AND value=?2",
                params![ACTIVE_PROFILE_KEY, name],
            )? != 0;
            transaction.commit()?;
        }
        match fs::remove_file(&target) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if was_active {
            match fs::remove_file(&link) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    /// Copy an existing profile to a new name, preserving settings JSON.
    /// The new profile is created with identical settings; project bindings
    /// are NOT copied.
    pub fn copy_profile(&self, source: &str, new_name: &str) -> Result<ClaudeProfile> {
        let source = validate_profile_name(source)?;
        let new_name = validate_profile_name(new_name)?;
        let source_profile = self.profile(&source)?;

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        match transaction.execute(
            "INSERT INTO claude_profiles (name,settings_json) VALUES (?1,?2)",
            params![&new_name, &source_profile.settings_json],
        ) {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(StetsonError::ProfileExists(new_name));
            }
            Err(error) => return Err(error.into()),
        }

        let target = self.profile_file_path(&new_name)?;
        AtomicReplace::write(&target, source_profile.settings_json.as_bytes())?;
        transaction.commit()?;
        drop(connection);
        self.profile(&new_name)
    }

    /// Reconcile profile files on disk into the `claude_profiles` SQLite table.
    ///
    /// When `name` is `None`, every `settings.<name>.json` in `profiles_dir()`
    /// is processed (alphabetical order). When `name` is `Some`, only that one
    /// file is processed (a missing file is a no-op).
    ///
    /// The disk is the source of truth: file content is read, validated, and
    /// used to INSERT or UPDATE the matching database row. Rows that have no
    /// corresponding file are left untouched. Files with non-conforming names
    /// (or invalid JSON, or undecodable bytes) are recorded in
    /// `SyncReport.invalid`; processing continues for the remaining files.
    /// Sync never modifies the on-disk files or the `~/.claude/settings.json`
    /// symlink.
    pub fn sync_profiles_from_disk(&self, name: Option<&str>) -> Result<SyncReport> {
        let mut report = SyncReport::default();
        let candidates: Vec<String> = match name {
            Some(raw) => vec![raw.to_string()],
            None => discover_profile_names(&self.profiles_dir()?)?,
        };
        for candidate in candidates {
            self.reconcile_one(&candidate, &mut report)?;
        }
        Ok(report)
    }

    /// Reconcile a single profile name against its on-disk file.
    ///
    /// The `raw_name` is the literal name as discovered (either from a
    /// `settings.<name>.json` filename or from the explicit `sync` argument).
    /// It is normalized via `validate_profile_name`; a normalization error
    /// populates `report.invalid` and returns without further action.
    fn reconcile_one(&self, raw_name: &str, report: &mut SyncReport) -> Result<()> {
        let name = match validate_profile_name(raw_name) {
            Ok(valid) => valid,
            Err(error) => {
                report.invalid.push(InvalidEntry {
                    name: raw_name.to_string(),
                    error: error.to_string(),
                });
                return Ok(());
            }
        };
        let path = self.profile_file_path(&name)?;
        let raw = match fs::read_to_string(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                report.invalid.push(InvalidEntry {
                    name,
                    error: format!("could not read file: {error}"),
                });
                return Ok(());
            }
        };
        if let Err(error) = validate_settings_json(&raw) {
            report.invalid.push(InvalidEntry {
                name,
                error: error.to_string(),
            });
            return Ok(());
        }
        let existing = self.profile(&name).ok();
        match existing {
            None => {
                self.sync_write_profile_json(&name, &raw)?;
                report.inserted.push(name);
            }
            Some(profile) if profile.settings_json == raw => {
                report.unchanged.push(name);
            }
            Some(_) => {
                self.sync_write_profile_json(&name, &raw)?;
                report.updated.push(name);
            }
        }
        Ok(())
    }

    /// Insert or update a profile row using only the database — without
    /// touching the per-profile mirror file on disk. This is the sync-only
    /// counterpart of `update_profile_json` and is used when the disk is
    /// already authoritative and we just want the row to match.
    fn sync_write_profile_json(&self, name: &str, settings_json: &str) -> Result<()> {
        validate_settings_json(settings_json)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE claude_profiles SET settings_json=?1,updated_at=CURRENT_TIMESTAMP WHERE name=?2",
            params![settings_json, name],
        )?;
        if changed == 0 {
            transaction.execute(
                "INSERT INTO claude_profiles (name,settings_json) VALUES (?1,?2)",
                params![name, settings_json],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Create missing mirror files for legacy database-only profiles.
    ///
    /// This is a non-destructive backfill: it only writes a mirror when none
    /// exists. An existing file is treated as a potential external edit and is
    /// left untouched; only an explicit `config sync` can reconcile it.
    pub fn backfill_missing_mirrors(&self) -> Result<()> {
        for profile in self.list_profiles()? {
            let path = self.profile_file_path(&profile.name)?;
            if !path.exists() {
                AtomicReplace::write(&path, profile.settings_json.as_bytes())?;
            }
        }
        Ok(())
    }

    pub fn perform_initial_backup(&self) -> Result<()> {
        if self.get_setting(INITIAL_BACKUP_DONE_KEY)?.as_deref() == Some("1") {
            return Ok(());
        }
        let dir = self.claude_config_dir()?;
        let source = dir.join("settings.json");
        let backup = dir.join(INITIAL_BACKUP_FILENAME);

        // On any "not a regular file" branch we still mark the flag so the
        // decision is sticky across launches. Backup file mode is enforced by
        // ensure_private_file after the copy.
        let should_copy = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata.file_type().is_file(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };

        if should_copy {
            let bytes = fs::read(&source)?;
            AtomicReplace::write(&backup, &bytes)?;
            ensure_private_file(&backup)?;
        }

        self.upsert_setting(&super::Setting {
            key: INITIAL_BACKUP_DONE_KEY.to_string(),
            value: "1".to_string(),
        })?;
        Ok(())
    }

    pub fn activate_profile(&self, name: &str) -> Result<PathBuf> {
        let name = validate_profile_name(name)?;
        let _lock = self.activation_lock()?;
        self.clear_failed_journal()?;
        self.maybe_migrate_existing_settings(&self.global_settings_path()?)?;
        let profile = self.profile(&name)?;
        validate_settings_json(&profile.settings_json)?;
        let target = self.profile_file_path(&name)?;
        self.perform_activation(
            "profile",
            profile.id.to_string(),
            &name,
            &target,
            &profile.settings_json,
        )?;
        self.finish_activation(Some(&name))?;
        self.global_settings_path()
    }

    fn maybe_migrate_existing_settings(&self, link: &Path) -> Result<()> {
        if self.profile("default").is_ok() {
            return Ok(());
        }
        let metadata = match fs::symlink_metadata(link) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            return Ok(());
        }
        let bytes = fs::read(link)?;
        let raw = std::str::from_utf8(&bytes)
            .map_err(|error| StetsonError::InvalidSettingsFile(error.to_string()))?;
        validate_settings_json(raw)?;
        let target = self.profiles_dir()?.join("settings.default.json");
        AtomicReplace::write(&target, raw.as_bytes())?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO claude_profiles (name,settings_json) VALUES (?1,?2)",
            params!["default", raw],
        )?;
        transaction.commit()?;
        replace_with_symlink(link, &target)?;
        Ok(())
    }

    fn perform_activation(
        &self,
        kind: &str,
        target_id: String,
        target_name: &str,
        target_file: &Path,
        settings_json: &str,
    ) -> Result<()> {
        let link = self.global_settings_path()?;
        let hash = sha256(settings_json.as_bytes());
        {
            let mut connection = self.connection()?;
            let transaction = connection.transaction()?;
            insert_journal(&transaction, kind, target_id, target_name, &hash)?;
            transaction.commit()?;
        }
        if let Err(error) = AtomicReplace::write(target_file, settings_json.as_bytes()) {
            let _ = self.delete_pending_journal();
            return Err(error);
        }
        // On symlink failure, leave the journal in place so recovery can mark
        // the activation failed.
        replace_with_symlink(&link, target_file)
    }

    pub fn profile_file_path(&self, name: &str) -> Result<PathBuf> {
        Ok(self.profiles_dir()?.join(format!("settings.{name}.json")))
    }

    pub fn recover_profile_activation(&self) -> Result<RecoveryOutcome> {
        let _lock = self.activation_lock()?;
        let connection = self.connection()?;
        let journal = connection
            .query_row(
                "SELECT target_kind,target_id,target_name,target_json_hash,phase,error \
             FROM profile_activation_journal WHERE id=1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((kind, id, name, expected, phase, error)) = journal else {
            return Ok(RecoveryOutcome::NoPending);
        };
        if phase == "failed" {
            return Ok(RecoveryOutcome::PreviouslyFailed(error.unwrap_or_default()));
        }
        let link = self.global_settings_path()?;
        let failure_message = match read_symlink_target_hash(&link) {
            Some(actual) if actual == expected.as_str() => {
                drop(connection);
                self.finish_activation(Some(&name))?;
                return Ok(RecoveryOutcome::Recovered {
                    target_kind: kind,
                    target_id: id,
                });
            }
            Some(_) => {
                format!("settings.json symlink target hash does not match pending {kind} {id}")
            }
            None => format!(
                "settings.json is missing or not a symlink; pending {kind} {id} cannot be recovered"
            ),
        };
        connection.execute(
            "UPDATE profile_activation_journal SET phase='failed',error=?1 WHERE id=1",
            [&failure_message],
        )?;
        Ok(RecoveryOutcome::Failed(failure_message))
    }

    pub fn claude_config_dir(&self) -> Result<PathBuf> {
        match self.get_setting(SETTING_CLAUDE_CONFIG_DIR)? {
            Some(path) if !path.trim().is_empty() && Path::new(path.trim()).is_absolute() => {
                Ok(PathBuf::from(path))
            }
            _ => super::settings::default_claude_config_dir(),
        }
    }

    pub fn profiles_dir(&self) -> Result<PathBuf> {
        let parent = self.path().parent().ok_or_else(|| {
            StetsonError::MigrationFailed("database path has no parent".to_string())
        })?;
        let dir = parent.join("profiles");
        ensure_private_dir(&dir)?;
        Ok(dir)
    }

    fn global_settings_path(&self) -> Result<PathBuf> {
        Ok(self.claude_config_dir()?.join("settings.json"))
    }

    fn finish_activation(&self, active_name: Option<&str>) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE profile_activation_journal SET phase='file_replaced' WHERE id=1",
            [],
        )?;
        match active_name {
            Some(name) => {
                transaction.execute(
                    "INSERT INTO settings (key,value) VALUES (?1,?2) \
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![ACTIVE_PROFILE_KEY, name],
                )?;
            }
            None => {
                transaction.execute("DELETE FROM settings WHERE key=?1", [ACTIVE_PROFILE_KEY])?;
            }
        }
        transaction.execute("DELETE FROM profile_activation_journal WHERE id=1", [])?;
        transaction.commit()?;
        Ok(())
    }

    fn clear_failed_journal(&self) -> Result<()> {
        self.connection()?.execute(
            "DELETE FROM profile_activation_journal WHERE id=1 AND phase='failed'",
            [],
        )?;
        Ok(())
    }

    fn delete_pending_journal(&self) -> Result<()> {
        self.connection()?
            .execute("DELETE FROM profile_activation_journal WHERE id=1", [])?;
        Ok(())
    }

    pub fn bind_profile(&self, project_cwd: &Path, profile_name: &str) -> Result<()> {
        let profile_name = validate_profile_name(profile_name)?;
        // Validate profile exists
        let _ = self.profile(&profile_name)?;
        let connection = self.connection()?;
        let cwd_str = project_cwd.to_string_lossy();
        connection.execute(
            "INSERT INTO project_profile_bindings (project_cwd, profile_name) VALUES (?1, ?2) \
             ON CONFLICT(project_cwd) DO UPDATE SET profile_name=excluded.profile_name, \
             created_at=CURRENT_TIMESTAMP",
            params![cwd_str.as_ref(), profile_name],
        )?;
        Ok(())
    }

    pub fn unbind_profile(&self, project_cwd: &Path) -> Result<()> {
        let connection = self.connection()?;
        let cwd_str = project_cwd.to_string_lossy();
        let changed = connection.execute(
            "DELETE FROM project_profile_bindings WHERE project_cwd=?1",
            [cwd_str.as_ref()],
        )?;
        if changed == 0 {
            return Err(StetsonError::BindingNotFound(cwd_str.into_owned()));
        }
        Ok(())
    }

    pub fn project_binding(&self, project_cwd: &Path) -> Result<Option<ProjectProfileBinding>> {
        let connection = self.connection()?;
        let cwd_str = project_cwd.to_string_lossy();
        let mut statement = connection.prepare(
            "SELECT project_cwd, profile_name FROM project_profile_bindings WHERE project_cwd=?1",
        )?;
        let mut rows = statement.query_map([cwd_str.as_ref()], |row| {
            Ok(ProjectProfileBinding {
                project_cwd: PathBuf::from(row.get::<_, String>(0)?),
                profile_name: row.get(1)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn profile_bindings(&self, profile_name: &str) -> Result<Vec<ProjectProfileBinding>> {
        let profile_name = validate_profile_name(profile_name)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT project_cwd, profile_name FROM project_profile_bindings WHERE profile_name=?1",
        )?;
        let rows = statement.query_map([&profile_name], |row| {
            Ok(ProjectProfileBinding {
                project_cwd: PathBuf::from(row.get::<_, String>(0)?),
                profile_name: row.get(1)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn activation_lock(&self) -> Result<ActivationLock> {
        let parent = self.path().parent().ok_or_else(|| {
            StetsonError::MigrationFailed("database path has no parent".to_string())
        })?;
        ensure_private_dir(parent)?;
        let path = parent.join("profile-activation.lock");
        let file = private_open(&path)?;
        file.lock_exclusive()?;
        Ok(ActivationLock(file))
    }
}

fn insert_journal(
    transaction: &Transaction<'_>,
    kind: &str,
    id: String,
    name: &str,
    hash: &str,
) -> Result<()> {
    transaction.execute("DELETE FROM profile_activation_journal WHERE id=1", [])?;
    transaction.execute(
        "INSERT INTO profile_activation_journal \
         (id,target_kind,target_id,target_name,target_json_hash,phase,error) \
         VALUES (1,?1,?2,?3,?4,'prepared',NULL)",
        params![kind, id, name, hash],
    )?;
    Ok(())
}

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClaudeProfile> {
    Ok(ClaudeProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        settings_json: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read_symlink_target_hash(link: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(link).ok()?;
    if !metadata.file_type().is_symlink() {
        return None;
    }
    let bytes = fs::read(link).ok()?;
    Some(sha256(&bytes))
}

struct ActivationLock(File);
impl Drop for ActivationLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

#[cfg(unix)]
pub(super) fn replace_with_symlink(link: &Path, target: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::symlink_metadata(link) {
        Ok(_) => fs::remove_file(link)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    symlink(target, link)?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn replace_with_symlink(_link: &Path, _target: &Path) -> Result<()> {
    Err(StetsonError::MigrationFailed(
        "symlink-based profile activation is only supported on Unix".to_string(),
    ))
}

pub(super) fn ensure_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    restrict_windows_acl(path, true)?;
    Ok(())
}

pub(super) fn ensure_private_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.exists() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
    }
    #[cfg(windows)]
    if path.exists() {
        restrict_windows_acl(path, false)?;
    }
    Ok(())
}

#[cfg(windows)]
fn restrict_windows_acl(path: &Path, directory: bool) -> Result<()> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{SetFileSecurityW, DACL_SECURITY_INFORMATION};

    // A protected DACL granting full access only to the object's owner. Directories
    // propagate the same rule to child files and directories.
    let sddl = if directory {
        "D:P(A;OICI;FA;;;OW)"
    } else {
        "D:P(A;;FA;;;OW)"
    };
    let sddl = std::ffi::OsStr::new(sddl)
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut descriptor: *mut c_void = std::ptr::null_mut();
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let applied =
        unsafe { SetFileSecurityW(path.as_ptr(), DACL_SECURITY_INFORMATION, descriptor.cast()) };
    unsafe { LocalFree(descriptor.cast()) };
    if applied == 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}

pub(super) fn private_open(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    ensure_private_file(path)?;
    Ok(file)
}

#[cfg(unix)]
fn target_mode(path: &Path) -> Result<Option<u32>> {
    use std::os::unix::fs::PermissionsExt;
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions().mode() & 0o777)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(not(unix))]
fn target_mode(_path: &Path) -> Result<Option<u32>> {
    Ok(None)
}

#[cfg(not(windows))]
fn replace_file(tmp: &Path, target: &Path) -> Result<()> {
    fs::rename(tmp, target).map_err(Into::into)
}

#[cfg(windows)]
fn replace_file(tmp: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH, REPLACEFILE_WRITE_THROUGH,
    };
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let tmp_wide = wide(tmp);
    let target_wide = wide(target);
    let success = unsafe {
        if target.exists() {
            ReplaceFileW(
                target_wide.as_ptr(),
                tmp_wide.as_ptr(),
                std::ptr::null(),
                REPLACEFILE_WRITE_THROUGH,
                std::ptr::null(),
                std::ptr::null(),
            )
        } else {
            MoveFileExW(
                tmp_wide.as_ptr(),
                target_wide.as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        }
    };
    if success == 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all().map_err(Into::into)
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::tempdir;

    fn store() -> (ClaudeEnvStore, tempfile::TempDir, PathBuf) {
        let temp = tempdir().unwrap();
        let config = temp.path().join("claude");
        let store = ClaudeEnvStore::new(temp.path().join("data/cowboy.db"));
        store.initialize().unwrap();
        store
            .upsert_setting(&super::super::Setting {
                key: SETTING_CLAUDE_CONFIG_DIR.into(),
                value: config.display().to_string(),
            })
            .unwrap();
        (store, temp, config)
    }

    #[cfg(unix)]
    #[test]
    fn replace_with_symlink_creates_link_for_missing_target() {
        let temp = tempdir().unwrap();
        let link = temp.path().join("settings.json");
        let target = temp.path().join("settings.work.json");
        fs::write(&target, br#"{"work":true}"#).unwrap();
        assert!(!link.exists(), "link should not exist yet");
        replace_with_symlink(&link, &target).unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), target);
        assert_eq!(fs::read(&link).unwrap(), fs::read(&target).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn replace_with_symlink_swaps_when_target_is_regular_file() {
        let temp = tempdir().unwrap();
        let link = temp.path().join("settings.json");
        let target = temp.path().join("settings.work.json");
        fs::write(&link, br#"{"old":true}"#).unwrap();
        fs::write(&target, br#"{"work":true}"#).unwrap();
        assert!(!fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        replace_with_symlink(&link, &target).unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), target);
        assert_eq!(fs::read(&link).unwrap(), br#"{"work":true}"#);
    }

    #[cfg(unix)]
    #[test]
    fn replace_with_symlink_repoints_when_target_is_symlink() {
        let temp = tempdir().unwrap();
        let link = temp.path().join("settings.json");
        let target_a = temp.path().join("settings.work.json");
        let target_b = temp.path().join("settings.home.json");
        fs::write(&target_a, br#"{"work":true}"#).unwrap();
        fs::write(&target_b, br#"{"home":true}"#).unwrap();
        replace_with_symlink(&link, &target_a).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), target_a);
        replace_with_symlink(&link, &target_b).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), target_b);
        assert_eq!(fs::read(&link).unwrap(), br#"{"home":true}"#);
    }

    #[test]
    fn profiles_dir_is_cowboy_data_profiles_subdirectory() {
        let (store, temp, _config) = store();
        assert_eq!(
            store.profiles_dir().unwrap(),
            temp.path().join("data").join("profiles")
        );
    }

    #[cfg(unix)]
    #[test]
    fn profiles_dir_is_created_with_private_mode() {
        use std::os::unix::fs::PermissionsExt;
        let (store, temp, _config) = store();
        let expected = temp.path().join("data").join("profiles");
        assert!(!expected.exists(), "directory should not exist yet");
        let path = store.profiles_dir().unwrap();
        assert_eq!(path, expected);
        assert!(path.exists(), "directory should be created on access");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn validates_and_normalizes_profile_names() {
        assert_eq!(validate_profile_name("Work_A-1").unwrap(), "work_a-1");
        for invalid in ["", "a b", "é", &"a".repeat(65)] {
            assert!(validate_profile_name(invalid).is_err());
        }
        assert!(validate_profile_name(&"a".repeat(64)).is_ok());
        for valid in ["{}", r#"{"_comment":"ok","$schema":"x"}"#] {
            assert!(validate_settings_json(valid).is_ok());
        }
        for invalid in ["", "null", "[]", "1", "true", "broken"] {
            assert!(validate_settings_json(invalid).is_err());
        }
    }

    #[test]
    fn profile_crud_is_case_insensitive_and_sorted() {
        let (store, _temp, _) = store();
        store.create_profile("Zulu").unwrap();
        store.create_profile("alpha").unwrap();
        assert!(matches!(
            store.create_profile("ALPHA"),
            Err(StetsonError::ProfileExists(_))
        ));
        store
            .update_profile_json("ZULU", r#"{"env":{"A":"b"}}"#)
            .unwrap();
        assert_eq!(
            store
                .list_profiles()
                .unwrap()
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zulu"]
        );
        assert!(store.update_profile_json("alpha", "[]").is_err());
        store.delete_profile("Alpha").unwrap();
        assert!(matches!(
            store.profile("alpha"),
            Err(StetsonError::ProfileNotFound(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn first_activation_migrates_existing_real_settings_into_default_profile() {
        let (store, _temp, config) = store();
        let profiles_dir = store.profiles_dir().unwrap();
        fs::create_dir_all(&config).unwrap();
        let original = r#"{"env":{"KEY":"value"}}"#;
        fs::write(config.join("settings.json"), original).unwrap();
        store.create_profile("work").unwrap();
        store
            .update_profile_json("work", r#"{"work":true}"#)
            .unwrap();
        store.activate_profile("work").unwrap();
        let profiles = store.list_profiles().unwrap();
        let names: Vec<_> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"default"));
        let default = store.profile("default").unwrap();
        assert_eq!(default.settings_json, original);
        let default_file = profiles_dir.join("settings.default.json");
        assert_eq!(fs::read_to_string(&default_file).unwrap(), original);
        let link = config.join("settings.json");
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_link(&link).unwrap(),
            profiles_dir.join("settings.work.json")
        );
    }

    #[cfg(unix)]
    #[test]
    fn first_activation_skips_migration_when_default_already_exists() {
        let (store, _temp, config) = store();
        let profiles_dir = store.profiles_dir().unwrap();
        fs::create_dir_all(&config).unwrap();
        let original = r#"{"env":{"KEY":"value"}}"#;
        fs::write(config.join("settings.json"), original).unwrap();
        // Pre-create the default profile + symlink to simulate already-migrated state.
        store.create_profile("default").unwrap();
        store.update_profile_json("default", original).unwrap();
        AtomicReplace::write(
            &profiles_dir.join("settings.default.json"),
            original.as_bytes(),
        )
        .unwrap();
        replace_with_symlink(
            &config.join("settings.json"),
            &profiles_dir.join("settings.default.json"),
        )
        .unwrap();
        store.create_profile("work").unwrap();
        store
            .update_profile_json("work", r#"{"work":true}"#)
            .unwrap();
        store.activate_profile("work").unwrap();
        assert!(store.profile("default").is_ok());
        assert_eq!(
            fs::read_link(config.join("settings.json")).unwrap(),
            profiles_dir.join("settings.work.json")
        );
    }

    #[cfg(unix)]
    #[test]
    fn first_activation_aborts_when_existing_settings_are_invalid_json() {
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("settings.json"), "broken").unwrap();
        store.create_profile("work").unwrap();
        assert!(store.activate_profile("work").is_err());
        assert_eq!(
            fs::read_to_string(config.join("settings.json")).unwrap(),
            "broken"
        );
        let profiles = store.list_profiles().unwrap();
        let names: Vec<_> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert!(!names.contains(&"default"));
    }

    #[cfg(unix)]
    #[test]
    fn first_activation_aborts_when_existing_settings_root_is_not_object() {
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("settings.json"), "[]").unwrap();
        store.create_profile("work").unwrap();
        assert!(store.activate_profile("work").is_err());
        assert_eq!(
            fs::read_to_string(config.join("settings.json")).unwrap(),
            "[]"
        );
    }

    #[cfg(unix)]
    #[test]
    fn activate_profile_writes_target_file_with_private_mode() {
        use std::os::unix::fs::PermissionsExt;
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        store.create_profile("private").unwrap();
        store.update_profile_json("private", r#"{"x":1}"#).unwrap();
        store.activate_profile("private").unwrap();
        let target = store.profiles_dir().unwrap().join("settings.private.json");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn activate_profile_swaps_symlink_when_activating_a_second_profile() {
        let (store, _temp, config) = store();
        let profiles_dir = store.profiles_dir().unwrap();
        fs::create_dir_all(&config).unwrap();
        store.create_profile("work").unwrap();
        store.create_profile("home").unwrap();
        store
            .update_profile_json("work", r#"{"work":true}"#)
            .unwrap();
        store
            .update_profile_json("home", r#"{"home":true}"#)
            .unwrap();
        store.activate_profile("work").unwrap();
        assert_eq!(
            fs::read_link(config.join("settings.json")).unwrap(),
            profiles_dir.join("settings.work.json")
        );
        store.activate_profile("home").unwrap();
        assert_eq!(
            fs::read_link(config.join("settings.json")).unwrap(),
            profiles_dir.join("settings.home.json")
        );
        assert_eq!(
            fs::read_to_string(profiles_dir.join("settings.work.json")).unwrap(),
            r#"{"work":true}"#
        );
        assert_eq!(
            fs::read_to_string(config.join("settings.json")).unwrap(),
            r#"{"home":true}"#
        );
    }

    #[cfg(unix)]
    #[test]
    fn activate_profile_journal_hash_targets_the_profile_file() {
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        let profile_json = r#"{"new":true}"#;
        let profile = store.create_profile("work").unwrap();
        store.update_profile_json("work", profile_json).unwrap();
        let target = store.profiles_dir().unwrap().join("settings.work.json");
        // Simulate the journal row that activate_profile would insert right
        // before swapping the symlink: hash is over the target file content.
        let connection = Connection::open(store.path()).unwrap();
        connection
            .execute(
                "INSERT INTO profile_activation_journal \
                 (id,target_kind,target_id,target_name,target_json_hash,phase) \
                 VALUES (1,'profile',?1,'work',?2,'prepared')",
                params![profile.id.to_string(), sha256(profile_json.as_bytes())],
            )
            .unwrap();
        // Pretend the symlink already points at the target (mid-activation).
        replace_with_symlink(&config.join("settings.json"), &target).unwrap();
        assert!(matches!(
            store.recover_profile_activation().unwrap(),
            RecoveryOutcome::Recovered { .. }
        ));
        assert_eq!(
            store.get_setting(ACTIVE_PROFILE_KEY).unwrap().as_deref(),
            Some("work")
        );
    }

    #[cfg(unix)]
    #[test]
    fn delete_inactive_profile_removes_only_its_file() {
        let (store, _temp, config) = store();
        let profiles_dir = store.profiles_dir().unwrap();
        store.create_profile("work").unwrap();
        store.create_profile("home").unwrap();
        store
            .update_profile_json("home", r#"{"home":true}"#)
            .unwrap();
        store.activate_profile("work").unwrap();
        let work_file = profiles_dir.join("settings.work.json");
        let home_file = profiles_dir.join("settings.home.json");
        assert!(work_file.exists());
        assert!(home_file.exists());
        store.delete_profile("home").unwrap();
        assert!(work_file.exists(), "active profile file should remain");
        assert!(!home_file.exists(), "deleted profile file should be gone");
        let meta = fs::symlink_metadata(config.join("settings.json")).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(
            fs::read_link(config.join("settings.json")).unwrap(),
            work_file
        );
        assert_eq!(
            store.active_profile_name().unwrap().as_deref(),
            Some("work")
        );
    }

    #[cfg(unix)]
    #[test]
    fn delete_active_profile_removes_symlink_and_clears_active() {
        let (store, _temp, config) = store();
        let profiles_dir = store.profiles_dir().unwrap();
        store.create_profile("work").unwrap();
        store.activate_profile("work").unwrap();
        let link = config.join("settings.json");
        let work_file = profiles_dir.join("settings.work.json");
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        store.delete_profile("work").unwrap();
        assert!(!work_file.exists());
        assert!(!link.exists(), "settings.json should be removed");
        assert_eq!(store.active_profile_name().unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn profiles_dir_is_created_with_private_mode_on_first_activation() {
        use std::os::unix::fs::PermissionsExt;
        let (store, temp, _config) = store();
        let profiles_dir = temp.path().join("data").join("profiles");
        assert!(!profiles_dir.exists());
        store.create_profile("work").unwrap();
        store.update_profile_json("work", r#"{"x":1}"#).unwrap();
        store.activate_profile("work").unwrap();
        assert!(profiles_dir.exists());
        assert_eq!(
            fs::metadata(&profiles_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[cfg(unix)]
    #[test]
    fn changing_claude_config_dir_moves_the_symlink_target() {
        use super::super::Setting;
        let (store, temp, config) = store();
        let profiles_dir = store.profiles_dir().unwrap();
        store.create_profile("work").unwrap();
        store
            .update_profile_json("work", r#"{"work":true}"#)
            .unwrap();
        store.activate_profile("work").unwrap();
        let link_a = config.join("settings.json");
        assert_eq!(
            fs::read_link(&link_a).unwrap(),
            profiles_dir.join("settings.work.json")
        );
        // Move the claude_config_dir to a new location.
        let new_config = temp.path().join("claude2");
        store
            .upsert_setting(&Setting {
                key: SETTING_CLAUDE_CONFIG_DIR.into(),
                value: new_config.display().to_string(),
            })
            .unwrap();
        store.activate_profile("work").unwrap();
        let link_b = new_config.join("settings.json");
        assert!(fs::symlink_metadata(&link_b)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_link(&link_b).unwrap(),
            profiles_dir.join("settings.work.json")
        );
        // The old symlink at the previous claude_config_dir location is left
        // alone: cowboy does not own that directory anymore. The profile file
        // itself is untouched.
        assert_eq!(
            fs::read_link(&link_a).unwrap(),
            profiles_dir.join("settings.work.json")
        );
        assert_eq!(
            fs::read_to_string(profiles_dir.join("settings.work.json")).unwrap(),
            r#"{"work":true}"#
        );
    }

    #[cfg(unix)]
    #[test]
    fn delete_active_profile_does_not_auto_activate_another() {
        let (store, _temp, config) = store();
        store.create_profile("work").unwrap();
        store.create_profile("home").unwrap();
        store.activate_profile("work").unwrap();
        store.delete_profile("work").unwrap();
        assert!(!config.join("settings.json").exists());
        assert_eq!(store.active_profile_name().unwrap(), None);
        let profiles = store.list_profiles().unwrap();
        let names: Vec<_> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["home"]);
    }

    #[test]
    fn invalid_current_settings_prevents_all_activation_writes() {
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("settings.json"), "broken").unwrap();
        store.create_profile("work").unwrap();
        assert!(store.activate_profile("work").is_err());
        assert_eq!(
            fs::read_to_string(config.join("settings.json")).unwrap(),
            "broken"
        );
    }

    #[cfg(unix)]
    #[test]
    fn recovery_completes_matching_journal_and_marks_mismatch_failed() {
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        let profiles_dir = store.profiles_dir().unwrap();
        let target = profiles_dir.join("settings.work.json");
        fs::write(&target, r#"{"ok":1}"#).unwrap();
        replace_with_symlink(&config.join("settings.json"), &target).unwrap();
        let connection = Connection::open(store.path()).unwrap();
        connection
            .execute(
                "INSERT INTO profile_activation_journal \
                 (id,target_kind,target_id,target_name,target_json_hash,phase) \
                 VALUES (1,'profile','2','work',?1,'prepared')",
                [sha256(br#"{"ok":1}"#)],
            )
            .unwrap();
        assert!(matches!(
            store.recover_profile_activation().unwrap(),
            RecoveryOutcome::Recovered { .. }
        ));
        assert_eq!(
            store.get_setting(ACTIVE_PROFILE_KEY).unwrap().as_deref(),
            Some("work")
        );
        // Insert a journal whose expected hash doesn't match the symlink target.
        connection
            .execute(
                "INSERT INTO profile_activation_journal \
                 (id,target_kind,target_id,target_name,target_json_hash,phase) \
                 VALUES (1,'profile','2','work','bad','prepared')",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.recover_profile_activation().unwrap(),
            RecoveryOutcome::Failed(_)
        ));
        assert!(matches!(
            store.recover_profile_activation().unwrap(),
            RecoveryOutcome::PreviouslyFailed(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn recovery_marks_broken_symlink_failed() {
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        let link = config.join("settings.json");
        let target = store.profiles_dir().unwrap().join("settings.work.json");
        fs::write(&target, r#"{"work":true}"#).unwrap();
        replace_with_symlink(&link, &target).unwrap();
        // Now delete the target to make the symlink dangle.
        fs::remove_file(&target).unwrap();
        let connection = Connection::open(store.path()).unwrap();
        connection
            .execute(
                "INSERT INTO profile_activation_journal \
                 (id,target_kind,target_id,target_name,target_json_hash,phase) \
                 VALUES (1,'profile','2','work',?1,'prepared')",
                [sha256(br#"{"work":true}"#)],
            )
            .unwrap();
        assert!(matches!(
            store.recover_profile_activation().unwrap(),
            RecoveryOutcome::Failed(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn recovery_marks_regular_file_failed() {
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        // Someone replaced the symlink with a regular file (e.g., external write).
        fs::write(config.join("settings.json"), r#"{"ok":1}"#).unwrap();
        let connection = Connection::open(store.path()).unwrap();
        connection
            .execute(
                "INSERT INTO profile_activation_journal \
                 (id,target_kind,target_id,target_name,target_json_hash,phase) \
                 VALUES (1,'profile','2','work',?1,'prepared')",
                [sha256(br#"{"ok":1}"#)],
            )
            .unwrap();
        assert!(matches!(
            store.recover_profile_activation().unwrap(),
            RecoveryOutcome::Failed(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn update_profile_json_syncs_profile_file() {
        let (store, _temp, _config) = store();
        store.create_profile("work").unwrap();
        let new_json = r#"{"v":2}"#;
        store.update_profile_json("work", new_json).unwrap();
        let profile = store.profile("work").unwrap();
        assert_eq!(profile.settings_json, new_json);
        let file = store.profiles_dir().unwrap().join("settings.work.json");
        assert_eq!(fs::read_to_string(&file).unwrap(), new_json);
    }

    #[cfg(unix)]
    #[test]
    fn update_profile_json_rolls_back_when_file_write_fails() {
        let (store, _temp, _config) = store();
        store.create_profile("work").unwrap();
        store
            .update_profile_json("work", r#"{"before":true}"#)
            .unwrap();
        // Force the next AtomicReplace::write to fail by replacing the target
        // path with a directory: rename(tmp -> dir) returns EISDIR.
        let profiles_dir = store.profiles_dir().unwrap();
        let file = profiles_dir.join("settings.work.json");
        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        let result = store.update_profile_json("work", r#"{"after":true}"#);
        fs::remove_dir(&file).unwrap();
        assert!(result.is_err());
        let profile = store.profile("work").unwrap();
        assert_eq!(profile.settings_json, r#"{"before":true}"#);
    }

    #[cfg(unix)]
    #[test]
    fn update_profile_json_preserves_profile_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let (store, _temp, _config) = store();
        store.create_profile("work").unwrap();
        store
            .update_profile_json("work", r#"{"first":true}"#)
            .unwrap();
        let file = store.profiles_dir().unwrap().join("settings.work.json");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
        store
            .update_profile_json("work", r#"{"second":true}"#)
            .unwrap();
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replace_preserves_existing_unix_mode() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempdir().unwrap();
        let path = temp.path().join("settings.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        AtomicReplace::write(&path, br#"{"changed":true}"#).unwrap();
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[cfg(unix)]
    #[test]
    fn database_and_new_settings_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let (store, _temp, config) = store();
        store.create_profile("private").unwrap();
        store.activate_profile("private").unwrap();
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // settings.json is a symlink; verify the target profile file has mode 0o600.
        let link = config.join("settings.json");
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        let target = fs::read_link(&link).unwrap();
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // ── project_profile_bindings tests ───────────────────────────────────

    #[test]
    fn bind_profile_creates_binding_and_query_returns_it() {
        let (store, _temp, _) = store();
        store.create_profile("work").unwrap();
        let cwd = PathBuf::from("/work/my-project");

        store.bind_profile(&cwd, "work").unwrap();

        let binding = store.project_binding(&cwd).unwrap();
        assert_eq!(binding.as_ref().unwrap().profile_name, "work");
        assert_eq!(binding.unwrap().project_cwd, cwd);
    }

    #[test]
    fn bind_profile_rejects_nonexistent_profile() {
        let (store, _temp, _) = store();
        let cwd = PathBuf::from("/work/my-project");

        let result = store.bind_profile(&cwd, "nonexistent");
        assert!(matches!(result, Err(StetsonError::ProfileNotFound(_))));
    }

    #[test]
    fn bind_profile_upserts_binding_for_same_project() {
        let (store, _temp, _) = store();
        store.create_profile("work").unwrap();
        store.create_profile("home").unwrap();
        let cwd = PathBuf::from("/work/my-project");

        store.bind_profile(&cwd, "work").unwrap();
        store.bind_profile(&cwd, "home").unwrap();

        let binding = store.project_binding(&cwd).unwrap().unwrap();
        assert_eq!(binding.profile_name, "home");
    }

    #[test]
    fn unbind_profile_removes_binding() {
        let (store, _temp, _) = store();
        store.create_profile("work").unwrap();
        let cwd = PathBuf::from("/work/my-project");
        store.bind_profile(&cwd, "work").unwrap();

        store.unbind_profile(&cwd).unwrap();

        assert!(store.project_binding(&cwd).unwrap().is_none());
    }

    #[test]
    fn unbind_profile_errors_when_no_binding() {
        let (store, _temp, _) = store();
        let cwd = PathBuf::from("/work/my-project");

        let result = store.unbind_profile(&cwd);
        assert!(matches!(result, Err(StetsonError::BindingNotFound(_))));
    }

    #[test]
    fn profile_bindings_returns_binding_for_bound_profile() {
        let (store, _temp, _) = store();
        store.create_profile("work").unwrap();
        let cwd = PathBuf::from("/work/project-a");
        store.bind_profile(&cwd, "work").unwrap();

        let bindings = store.profile_bindings("work").unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].project_cwd, cwd);
        assert_eq!(bindings[0].profile_name, "work");
    }

    #[test]
    fn profile_bindings_returns_empty_for_unbound_profile() {
        let (store, _temp, _) = store();
        store.create_profile("work").unwrap();

        let bindings = store.profile_bindings("work").unwrap();
        assert!(bindings.is_empty());
    }

    #[test]
    fn delete_profile_with_binding_fails_due_to_foreign_key() {
        let (store, _temp, _) = store();
        store.create_profile("work").unwrap();
        let cwd = PathBuf::from("/work/my-project");
        store.bind_profile(&cwd, "work").unwrap();

        let result = store.delete_profile("work");
        assert!(result.is_err());
    }

    #[test]
    fn bind_profile_allows_same_profile_bound_to_multiple_projects() {
        let (store, _temp, _) = store();
        store.create_profile("work").unwrap();
        let cwd_a = PathBuf::from("/work/project-a");
        let cwd_b = PathBuf::from("/work/project-b");

        store.bind_profile(&cwd_a, "work").unwrap();
        store.bind_profile(&cwd_b, "work").unwrap();

        // Both bindings should exist
        let binding_a = store.project_binding(&cwd_a).unwrap().unwrap();
        let binding_b = store.project_binding(&cwd_b).unwrap().unwrap();

        assert_eq!(binding_a.profile_name, "work");
        assert_eq!(binding_a.project_cwd, cwd_a);
        assert_eq!(binding_b.profile_name, "work");
        assert_eq!(binding_b.project_cwd, cwd_b);

        // profile_bindings should return both projects
        let bindings = store.profile_bindings("work").unwrap();
        assert_eq!(bindings.len(), 2);
    }

    #[test]
    fn profile_file_path_returns_settings_json_in_profiles_dir() {
        let (store, _temp, _) = store();
        let path = store.profile_file_path("work").unwrap();
        assert_eq!(
            path,
            store.profiles_dir().unwrap().join("settings.work.json")
        );
    }

    #[test]
    fn profile_file_path_preserves_name_as_provided() {
        let (store, _temp, _) = store();
        let path = store.profile_file_path("Work_A-1").unwrap();
        assert_eq!(
            path,
            store.profiles_dir().unwrap().join("settings.Work_A-1.json")
        );
    }

    // ── copy_profile tests ─────────────────────────────────────────────────

    // ── first-launch backup tests ───────────────────────────────────────────

    #[test]
    fn perform_initial_backup_copies_existing_settings_file() {
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        let original = "{\"key\":\"value\"}";
        fs::write(config.join("settings.json"), original).unwrap();

        store.perform_initial_backup().unwrap();

        let backup = config.join("settings.json.cowboy-backup");
        assert!(backup.exists());
        assert_eq!(fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(
            store
                .get_setting(INITIAL_BACKUP_DONE_KEY)
                .unwrap()
                .as_deref(),
            Some("1")
        );
    }

    #[test]
    fn perform_initial_backup_is_idempotent_via_flag() {
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        let original = "{\"v\":1}";
        fs::write(config.join("settings.json"), original).unwrap();

        store.perform_initial_backup().unwrap();
        // Overwrite the source file: a second call must NOT replace the backup.
        let overwritten = "{\"v\":2}";
        fs::write(config.join("settings.json"), overwritten).unwrap();
        store.perform_initial_backup().unwrap();

        let backup = config.join("settings.json.cowboy-backup");
        assert_eq!(fs::read_to_string(&backup).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn perform_initial_backup_skipped_when_settings_is_symlink() {
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        let real_target = config.join("settings.real.json");
        fs::write(&real_target, "{}").unwrap();
        std::os::unix::fs::symlink(&real_target, config.join("settings.json")).unwrap();

        store.perform_initial_backup().unwrap();

        let backup = config.join("settings.json.cowboy-backup");
        assert!(
            !backup.exists(),
            "backup must not be created when settings.json is a symlink"
        );
        assert_eq!(
            store
                .get_setting(INITIAL_BACKUP_DONE_KEY)
                .unwrap()
                .as_deref(),
            Some("1")
        );
    }

    #[test]
    fn perform_initial_backup_skipped_when_settings_missing() {
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        // No settings.json at all.

        store.perform_initial_backup().unwrap();

        let backup = config.join("settings.json.cowboy-backup");
        assert!(!backup.exists());
        // The flag is still set so we never re-check on subsequent launches.
        assert_eq!(
            store
                .get_setting(INITIAL_BACKUP_DONE_KEY)
                .unwrap()
                .as_deref(),
            Some("1")
        );
    }

    #[cfg(unix)]
    #[test]
    fn backup_file_has_private_permissions_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let (store, _temp, config) = store();
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("settings.json"), "{}").unwrap();

        store.perform_initial_backup().unwrap();

        let backup = config.join("settings.json.cowboy-backup");
        assert_eq!(
            fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // ── copy_profile tests ─────────────────────────────────────────────────

    #[test]
    fn copy_profile_creates_exact_duplicate() {
        let (store, _temp, _) = store();
        store.create_profile("work").unwrap();
        store
            .update_profile_json("work", r#"{"env":{"KEY":"value"}}"#)
            .unwrap();

        let copied = store.copy_profile("work", "work-debug").unwrap();

        assert_eq!(copied.name, "work-debug");
        assert_eq!(copied.settings_json, r#"{"env":{"KEY":"value"}}"#);
        // Original unchanged
        assert_eq!(
            store.profile("work").unwrap().settings_json,
            r#"{"env":{"KEY":"value"}}"#
        );
    }

    #[test]
    fn copy_profile_to_existing_name_returns_error() {
        let (store, _temp, _) = store();
        store.create_profile("work").unwrap();
        store.create_profile("home").unwrap();

        let result = store.copy_profile("work", "home");
        assert!(matches!(result, Err(StetsonError::ProfileExists(_))));
    }

    #[test]
    fn copy_profile_from_nonexistent_returns_error() {
        let (store, _temp, _) = store();
        store.create_profile("work").unwrap();

        let result = store.copy_profile("nonexistent", "new-profile");
        assert!(matches!(result, Err(StetsonError::ProfileNotFound(_))));
    }

    #[test]
    fn copy_profile_with_invalid_new_name_returns_error() {
        let (store, _temp, _) = store();
        store.create_profile("work").unwrap();

        let result = store.copy_profile("work", "invalid name");
        assert!(matches!(result, Err(StetsonError::InvalidProfileName(_))));
    }

    // ── sync_profiles_from_disk tests ───────────────────────────────────

    fn write_profile_file(store: &ClaudeEnvStore, name: &str, body: &str) -> PathBuf {
        let path = store.profile_file_path(name).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn sync_with_no_arg_returns_unchanged_when_only_mirror_matches_db() {
        let (store, _temp, _config) = store();
        // create_profile now atomically writes the mirror file, so the
        // profiles dir is never truly empty after a profile exists.
        store.create_profile("work").unwrap();

        let report = store.sync_profiles_from_disk(None).unwrap();
        assert!(report.inserted.is_empty());
        assert!(report.updated.is_empty());
        assert_eq!(report.unchanged, vec!["work".to_string()]);
        assert!(report.invalid.is_empty());
    }

    #[test]
    fn sync_ignores_non_conforming_files_in_profiles_dir() {
        let (store, _temp, _config) = store();
        let dir = store.profiles_dir().unwrap();
        fs::write(dir.join("notes.txt"), "ignore me").unwrap();
        fs::write(dir.join("settings..json"), "{}").unwrap();
        fs::write(dir.join("settings..json.bak"), "{}").unwrap();
        fs::write(dir.join("settings.work.json.bak"), "{}").unwrap();

        let report = store.sync_profiles_from_disk(None).unwrap();
        assert!(report.is_empty());
        assert_eq!(store.list_profiles().unwrap().len(), 0);
    }

    #[test]
    fn sync_with_specific_name_does_not_touch_db_when_file_is_missing() {
        let (store, _temp, _config) = store();
        store.create_profile("work").unwrap();
        store
            .update_profile_json("work", r#"{"before":true}"#)
            .unwrap();
        // Remove the on-disk mirror so sync has nothing to reconcile.
        let file_path = store.profile_file_path("work").unwrap();
        fs::remove_file(&file_path).unwrap();

        let report = store.sync_profiles_from_disk(Some("work")).unwrap();
        assert!(report.is_empty());
        let profile = store.profile("work").unwrap();
        assert_eq!(profile.settings_json, r#"{"before":true}"#);
    }

    #[test]
    fn sync_inserts_profile_from_disk_when_no_db_row() {
        let (store, _temp, _config) = store();
        write_profile_file(&store, "newproj", r#"{"env":{"NEW":"1"}}"#);

        let report = store.sync_profiles_from_disk(None).unwrap();
        assert_eq!(report.inserted, vec!["newproj".to_string()]);
        let profile = store.profile("newproj").unwrap();
        assert_eq!(profile.settings_json, r#"{"env":{"NEW":"1"}}"#);
    }

    #[test]
    fn sync_updates_db_row_when_disk_differs() {
        let (store, _temp, _config) = store();
        store.create_profile("work").unwrap();
        store
            .update_profile_json("work", r#"{"key":"old"}"#)
            .unwrap();
        write_profile_file(&store, "work", r#"{"key":"new"}"#);

        let report = store.sync_profiles_from_disk(None).unwrap();
        assert_eq!(report.updated, vec!["work".to_string()]);
        let profile = store.profile("work").unwrap();
        assert_eq!(profile.settings_json, r#"{"key":"new"}"#);
    }

    #[test]
    fn sync_no_ops_when_db_and_disk_match() {
        let (store, _temp, _config) = store();
        store.create_profile("work").unwrap();
        store
            .update_profile_json("work", r#"{"key":"value"}"#)
            .unwrap();

        let report = store.sync_profiles_from_disk(None).unwrap();
        assert_eq!(report.unchanged, vec!["work".to_string()]);
        assert!(report.inserted.is_empty());
        assert!(report.updated.is_empty());
    }

    #[test]
    fn sync_skips_invalid_json_and_returns_entry_in_report() {
        let (store, _temp, _config) = store();
        write_profile_file(&store, "broken_a", "trailing comma,");
        write_profile_file(&store, "broken_b", "[1,2,3]");
        write_profile_file(&store, "good", r#"{"k":1}"#);

        let report = store.sync_profiles_from_disk(None).unwrap();
        let invalid_names: Vec<&str> = report.invalid.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(invalid_names, vec!["broken_a", "broken_b"]);
        assert!(!report.invalid[0].error.is_empty());
        assert_eq!(report.inserted, vec!["good".to_string()]);
    }

    #[test]
    /// With the profile-file invariant, missing files are no longer a stable
    /// state — `create_profile` and `backfill_missing_mirrors` ensure every
    /// profile has a mirror. Sync still handles this edge case correctly by
    /// leaving the DB untouched (sync is file→DB only, not a file creator).
    fn sync_leaves_db_row_when_disk_file_missing() {
        let (store, _temp, _config) = store();
        store.create_profile("work").unwrap();
        store
            .update_profile_json("work", r#"{"key":"value"}"#)
            .unwrap();
        let file_path = store.profile_file_path("work").unwrap();
        fs::remove_file(&file_path).unwrap();

        let report = store.sync_profiles_from_disk(None).unwrap();
        assert!(report.is_empty());
        let profile = store.profile("work").unwrap();
        assert_eq!(profile.settings_json, r#"{"key":"value"}"#);
    }

    #[test]
    fn sync_walks_all_files_in_profiles_dir_when_called_with_none() {
        let (store, _temp, _config) = store();
        write_profile_file(&store, "alpha", r#"{"a":1}"#);
        write_profile_file(&store, "beta", r#"{"b":2}"#);
        write_profile_file(&store, "gamma", r#"{"g":3}"#);

        let report = store.sync_profiles_from_disk(None).unwrap();
        assert_eq!(
            report.inserted,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
    }

    #[test]
    fn sync_only_targets_given_name_when_some() {
        let (store, _temp, _config) = store();
        write_profile_file(&store, "alpha", r#"{"a":1}"#);
        write_profile_file(&store, "beta", r#"{"b":2}"#);

        let report = store.sync_profiles_from_disk(Some("alpha")).unwrap();
        assert_eq!(report.inserted, vec!["alpha".to_string()]);
        assert!(store.profile("beta").is_err());
    }

    #[test]
    fn sync_preserves_project_bindings() {
        let (store, _temp, _config) = store();
        store.create_profile("work").unwrap();
        let cwd_a = PathBuf::from("/work/project-a");
        let cwd_b = PathBuf::from("/work/project-b");
        store.bind_profile(&cwd_a, "work").unwrap();
        store.bind_profile(&cwd_b, "work").unwrap();
        write_profile_file(&store, "work", r#"{"key":"new"}"#);

        let _report = store.sync_profiles_from_disk(Some("work")).unwrap();
        assert_eq!(store.profile_bindings("work").unwrap().len(), 2);
    }

    #[test]
    fn sync_with_invalid_name_format_in_filename_lands_in_invalid() {
        let (store, _temp, _config) = store();
        let dir = store.profiles_dir().unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("settings.Work With Space.json"), r#"{"ok":true}"#).unwrap();

        let report = store.sync_profiles_from_disk(None).unwrap();
        assert_eq!(report.invalid.len(), 1);
        assert!(report.invalid[0].name.contains("Work With Space"));
        assert!(report.invalid[0].error.to_lowercase().contains("name"));
    }

    #[test]
    fn sync_with_binary_file_records_invalid_and_continues() {
        let (store, _temp, _config) = store();
        let dir = store.profiles_dir().unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("settings.binary.json"), [0xFF, 0xFE, 0xFD]).unwrap();
        write_profile_file(&store, "good", r#"{"k":1}"#);

        let report = store.sync_profiles_from_disk(None).unwrap();
        assert_eq!(report.invalid.len(), 1);
        assert_eq!(report.invalid[0].name, "binary");
        assert_eq!(report.inserted, vec!["good".to_string()]);
    }

    #[test]
    fn sync_handles_json_object_passing_through_validation_then_inserts() {
        let (store, _temp, _config) = store();
        write_profile_file(&store, "ok", r#"{"key":"value"}"#);

        let report = store.sync_profiles_from_disk(Some("ok")).unwrap();
        assert_eq!(report.inserted, vec!["ok".to_string()]);
        assert!(report.invalid.is_empty());
    }

    // ── Profile file invariant tests (section 9) ──────────────────────

    #[test]
    fn create_profile_writes_empty_json_mirror_file() {
        let (store, _temp, _config) = store();
        store.create_profile("work").unwrap();
        let path = store.profile_file_path("work").unwrap();
        assert!(path.exists(), "mirror file should be created");
        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "{}");
    }

    #[test]
    fn create_profile_rolls_back_db_insert_when_mirror_write_fails() {
        let (store, _temp, _config) = store();
        // Create a file at the profiles directory path so that
        // ensure_private_dir (called by AtomicReplace::write) fails.
        let profiles_dir = store.profiles_dir().unwrap();
        fs::remove_dir_all(&profiles_dir).unwrap();
        fs::write(&profiles_dir, b"block").unwrap();

        let result = store.create_profile("work");
        assert!(result.is_err(), "should fail when mirror cannot be written");
        // The DB insert should have been rolled back.
        assert!(
            store.profile("work").is_err(),
            "profile row should not exist after rollback"
        );
    }

    #[test]
    fn backfill_creates_missing_mirror_for_legacy_profile() {
        let (store, _temp, _config) = store();
        // Simulate a legacy DB-only profile by inserting directly into SQLite.
        let conn = store.connection().unwrap();
        conn.execute(
            "INSERT INTO claude_profiles (name, settings_json) VALUES (?1, ?2)",
            params!["legacy", r#"{"legacy":true}"#],
        )
        .unwrap();

        let path = store.profile_file_path("legacy").unwrap();
        assert!(!path.exists(), "no mirror file before backfill");

        store.backfill_missing_mirrors().unwrap();

        assert!(path.exists(), "mirror file should be created by backfill");
        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents, r#"{"legacy":true}"#);
    }

    #[test]
    fn backfill_does_not_overwrite_existing_mirror() {
        let (store, _temp, _config) = store();
        // Simulate a legacy profile whose file was edited externally.
        let conn = store.connection().unwrap();
        conn.execute(
            "INSERT INTO claude_profiles (name, settings_json) VALUES (?1, ?2)",
            params!["legacy", r#"{"db":"value"}"#],
        )
        .unwrap();
        let path = store.profile_file_path("legacy").unwrap();
        fs::write(&path, r#"{"disk":"different"}"#).unwrap();

        store.backfill_missing_mirrors().unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents, r#"{"disk":"different"}"#,
            "existing file must not be overwritten"
        );
    }
}
