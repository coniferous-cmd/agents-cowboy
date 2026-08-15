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
const AUTO_SNAPSHOT_LIMIT: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeProfile {
    pub id: i64,
    pub name: String,
    pub settings_json: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeSettingsSnapshot {
    pub id: i64,
    pub captured_at: String,
    pub source: Option<String>,
    pub settings_json: String,
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
        let connection = self.connection()?;
        match connection.execute(
            "INSERT INTO claude_profiles (name,settings_json) VALUES (?1,'{}')",
            [&name],
        ) {
            Ok(_) => self.profile(&name),
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StetsonError::ProfileExists(name))
            }
            Err(error) => Err(error.into()),
        }
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
            let changed =
                transaction.execute("DELETE FROM claude_profiles WHERE name=?1", [&name])?;
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

    pub fn list_snapshots(&self) -> Result<Vec<ClaudeSettingsSnapshot>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id,captured_at,source,settings_json FROM claude_settings_snapshots \
             ORDER BY captured_at DESC,id DESC",
        )?;
        let rows = statement.query_map([], snapshot_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn create_snapshot(&self, profile_id: i64, settings_json: &str) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO claude_settings_snapshots (source,settings_json) VALUES (?1,?2)",
            params![format!("pre-edit:{profile_id}"), settings_json],
        )?;
        Ok(())
    }

    pub fn snapshot(&self, id: i64) -> Result<ClaudeSettingsSnapshot> {
        let connection = self.connection()?;
        connection.query_row(
            "SELECT id,captured_at,source,settings_json FROM claude_settings_snapshots WHERE id=?1",
            [id], snapshot_from_row,
        ).optional()?.ok_or(StetsonError::SnapshotNotFound(id))
    }

    pub fn delete_snapshot(&self, id: i64) -> Result<()> {
        let connection = self.connection()?;
        if connection.execute("DELETE FROM claude_settings_snapshots WHERE id=?1", [id])? == 0 {
            return Err(StetsonError::SnapshotNotFound(id));
        }
        Ok(())
    }

    pub fn prune_snapshots(&self, keep: usize) -> Result<usize> {
        let connection = self.connection()?;
        prune_snapshots_on(&connection, keep)
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
            Some(&name),
            &target,
            &profile.settings_json,
            Some(format!("pre-activate:{name}")),
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

    pub fn activate_snapshot(&self, id: i64) -> Result<PathBuf> {
        let _lock = self.activation_lock()?;
        self.clear_failed_journal()?;
        let snapshot = self.snapshot(id)?;
        validate_settings_json(&snapshot.settings_json)?;
        let target = self.orphan_file_path()?;
        self.perform_activation(
            "snapshot",
            id.to_string(),
            None,
            &target,
            &snapshot.settings_json,
            None,
        )?;
        self.finish_activation(None)?;
        self.global_settings_path()
    }

    fn perform_activation(
        &self,
        kind: &str,
        target_id: String,
        target_name: Option<&str>,
        target_file: &Path,
        settings_json: &str,
        snapshot_source: Option<String>,
    ) -> Result<()> {
        let link = self.global_settings_path()?;
        let current = self.read_current_settings(&link)?;
        let hash = sha256(settings_json.as_bytes());
        {
            let mut connection = self.connection()?;
            let transaction = connection.transaction()?;
            if let (Some(raw), Some(source)) = (current, snapshot_source) {
                transaction.execute(
                    "INSERT INTO claude_settings_snapshots (source,settings_json) VALUES (?1,?2)",
                    params![source, raw],
                )?;
            }
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

    fn orphan_file_path(&self) -> Result<PathBuf> {
        Ok(self.profiles_dir()?.join("settings._orphan.json"))
    }

    fn read_current_settings(&self, link: &Path) -> Result<Option<String>> {
        match fs::read(link) {
            Ok(bytes) => {
                let raw = std::str::from_utf8(&bytes)
                    .map_err(|error| StetsonError::InvalidSettingsFile(error.to_string()))?;
                validate_settings_json(raw)?;
                Ok(Some(raw.to_string()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
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
                        row.get::<_, Option<String>>(2)?,
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
                self.finish_activation(name.as_deref())?;
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
        prune_snapshots_on(&transaction, AUTO_SNAPSHOT_LIMIT)?;
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
        match connection.execute(
            "INSERT INTO project_profile_bindings (project_cwd, profile_name) VALUES (?1, ?2) \
             ON CONFLICT(project_cwd) DO UPDATE SET profile_name=excluded.profile_name, \
             created_at=CURRENT_TIMESTAMP",
            params![cwd_str.as_ref(), profile_name],
        ) {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StetsonError::ProfileAlreadyBound(profile_name))
            }
            Err(error) => Err(error.into()),
        }
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
    name: Option<&str>,
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

fn prune_snapshots_on(connection: &rusqlite::Connection, keep: usize) -> Result<usize> {
    let changed = connection.execute(
        "DELETE FROM claude_settings_snapshots WHERE id IN (\
           SELECT id FROM claude_settings_snapshots \
           ORDER BY captured_at DESC,id DESC LIMIT -1 OFFSET ?1)",
        [keep as i64],
    )?;
    Ok(changed)
}

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClaudeProfile> {
    Ok(ClaudeProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        settings_json: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

fn snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClaudeSettingsSnapshot> {
    Ok(ClaudeSettingsSnapshot {
        id: row.get(0)?,
        captured_at: row.get(1)?,
        source: row.get(2)?,
        settings_json: row.get(3)?,
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
    fn activation_captures_exact_snapshot_and_snapshot_restore_does_not_recurse() {
        let (store, _temp, config) = store();
        let profiles_dir = store.profiles_dir().unwrap();
        fs::create_dir_all(&config).unwrap();
        // Simulate the post-migration state: default profile already exists,
        // settings.json is a symlink to profiles/settings.default.json.
        let default_json = "{\n  \"old\": true\n}";
        AtomicReplace::write(
            &profiles_dir.join("settings.default.json"),
            default_json.as_bytes(),
        )
        .unwrap();
        store.create_profile("default").unwrap();
        store.update_profile_json("default", default_json).unwrap();
        replace_with_symlink(
            &config.join("settings.json"),
            &profiles_dir.join("settings.default.json"),
        )
        .unwrap();
        store.create_profile("work").unwrap();
        store
            .update_profile_json("work", r#"{"new":true}"#)
            .unwrap();
        assert_eq!(
            store.activate_profile("work").unwrap(),
            config.join("settings.json")
        );
        let meta = fs::symlink_metadata(config.join("settings.json")).unwrap();
        assert!(
            meta.file_type().is_symlink(),
            "settings.json should be a symlink after activation"
        );
        assert_eq!(
            fs::read_link(config.join("settings.json")).unwrap(),
            profiles_dir.join("settings.work.json")
        );
        assert_eq!(
            fs::read_to_string(config.join("settings.json")).unwrap(),
            r#"{"new":true}"#
        );
        let snapshots = store.list_snapshots().unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].source.as_deref(), Some("pre-activate:work"));
        assert_eq!(snapshots[0].settings_json, default_json);
        store.activate_snapshot(snapshots[0].id).unwrap();
        assert!(fs::symlink_metadata(config.join("settings.json"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_link(config.join("settings.json")).unwrap(),
            profiles_dir.join("settings._orphan.json")
        );
        assert_eq!(
            fs::read_to_string(config.join("settings.json")).unwrap(),
            default_json
        );
        assert_eq!(store.list_snapshots().unwrap().len(), 1);
        assert_eq!(store.get_setting(ACTIVE_PROFILE_KEY).unwrap(), None);
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
    fn activate_snapshot_overwrites_prior_orphan_file() {
        let (store, _temp, config) = store();
        let profiles_dir = store.profiles_dir().unwrap();
        fs::create_dir_all(&config).unwrap();
        let orphan = profiles_dir.join("settings._orphan.json");
        let profile = store.create_profile("snap-host").unwrap();
        store
            .create_snapshot(profile.id, r#"{"first":true}"#)
            .unwrap();
        let first_id = store.list_snapshots().unwrap()[0].id;
        store.activate_snapshot(first_id).unwrap();
        assert_eq!(fs::read_link(config.join("settings.json")).unwrap(), orphan);
        assert_eq!(fs::read_to_string(&orphan).unwrap(), r#"{"first":true}"#);
        store
            .create_snapshot(profile.id, r#"{"second":true}"#)
            .unwrap();
        let second_id = store.list_snapshots().unwrap()[0].id;
        store.activate_snapshot(second_id).unwrap();
        assert_eq!(fs::read_link(config.join("settings.json")).unwrap(), orphan);
        assert_eq!(fs::read_to_string(&orphan).unwrap(), r#"{"second":true}"#);
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
        #[cfg(unix)]
        replace_with_symlink(&config.join("settings.json"), &target).unwrap();
        #[cfg(not(unix))]
        fs::write(config.join("settings.json"), profile_json).unwrap();
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
        assert!(store.list_snapshots().unwrap().is_empty());
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

    #[test]
    fn snapshot_order_and_prune_use_id_as_stable_tiebreaker() {
        let (store, _temp, _) = store();
        let connection = store.connection().unwrap();
        for raw in [r#"{"n":1}"#, r#"{"n":2}"#, r#"{"n":3}"#] {
            connection.execute(
                "INSERT INTO claude_settings_snapshots (captured_at,settings_json) VALUES ('2026-01-01 00:00:00',?1)",
                [raw],
            ).unwrap();
        }
        assert_eq!(
            store
                .list_snapshots()
                .unwrap()
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            [3, 2, 1]
        );
        assert_eq!(store.prune_snapshots(1).unwrap(), 2);
        assert_eq!(store.list_snapshots().unwrap()[0].id, 3);
        assert_eq!(store.prune_snapshots(0).unwrap(), 1);
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
}
