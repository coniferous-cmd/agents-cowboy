## 1. Schema 层

- [x] 1.1 在 `src/claude_env/schema.rs` 把 `PROFILES_SCHEMA` 常量里的 `claude_settings_snapshots` 表 DDL 删除
- [x] 1.2 把 `profile_activation_journal` 的 CHECK 约束改为只允许 `'profile'`,删除 `target_name IS NULL` 的 CHECK
- [x] 1.3 `SCHEMA_VERSION` 从 `3` 升到 `4`
- [x] 1.4 新增 `migrate_v3_to_v4(connection: &mut Connection)` 函数
- [x] 1.5 `initialize_schema` 在 `version == 3` 时调用 `migrate_v3_to_v4`

## 2. 负向断言测试(TDD,先加再删代码)

- [x] 2.1 `src/claude_env/schema.rs` 测试模块:`fresh_database_is_version_four_without_snapshots_table` —— 验证 `claude_settings_snapshots` 不存在(通过重写 `fresh_database_is_version_three_without_legacy_tables` 为 `_four`)
- [x] 2.2 `src/claude_env/schema.rs` 测试模块:`v3_to_v4_migration_drops_snapshot_table_and_writes_flag`
- [x] 2.3 `src/cmd/config.rs` 测试模块:`config_history_subcommand_rejected` → 实现为 `rejects_history_subcommand`
- [x] 2.4 `src/application.rs` 测试模块:`profile_repository_has_no_snapshot_methods` —— 通过 trait 编译失败即可验证
- [x] 2.5 `src/app/mod.rs` 测试模块:`ui_state_has_no_snapshots_field` —— 通过编译失败即可验证

## 3. 存储层:`src/claude_env/profiles.rs`

- [x] 3.1 删除 `ClaudeSettingsSnapshot` 结构体定义
- [x] 3.2 删除 `AUTO_SNAPSHOT_LIMIT` 常量
- [x] 3.3 删除 `list_snapshots`、`create_snapshot`、`snapshot`、`delete_snapshot`、`prune_snapshots`、`activate_snapshot` 方法体
- [x] 3.4 删除 `snapshot_from_row`、`prune_snapshots_on` helper,以及未再使用的 `read_current_settings`、`orphan_file_path`
- [x] 3.5 修改 `perform_activation` 签名,删除 `snapshot_source: Option<String>` 参数,删除函数体内对应 INSERT 段
- [x] 3.6 修改 `activate_profile` 调用点,不再传 `Some(format!("pre-activate:{name}"))`
- [x] 3.7 修改 `finish_activation`,删除 `prune_snapshots_on(&transaction, AUTO_SNAPSHOT_LIMIT)?` 这一行
- [x] 3.8 新增 `perform_initial_backup(&self) -> Result<()>`

## 4. 领域层:`src/domain.rs`

- [x] 4.1 删除 `StetsonError::SnapshotNotFound(i64)` 变体
- [x] 4.2 删除该变体的 `Display` 实现行
- [x] 4.3 删除该变体的 `source()` 行

## 5. 模块导出:`src/claude_env/mod.rs`、`src/lib.rs`

- [x] 5.1 `src/claude_env/mod.rs`:从 re-exports 删除 `ClaudeSettingsSnapshot`
- [x] 5.2 `src/lib.rs`:同步删除 `ClaudeSettingsSnapshot`

## 6. 应用层:`src/application.rs`

- [x] 6.1 `ProfileRepository` trait 删除 `list_snapshots`、`activate_snapshot`、`create_snapshot` 三个方法签名
- [x] 6.2 `NoProfileRepository` 实现删除对应 3 个 stub
- [x] 6.3 `StetsonApplication::load_profile_data` 返回类型从 `(Vec<ClaudeProfile>, Vec<ClaudeSettingsSnapshot>, Option<String>)` 改为 `(Vec<ClaudeProfile>, Option<String>)`,函数体内删除 `list_snapshots()` 调用
- [x] 6.4 删除 `StetsonApplication::activate_snapshot`、`create_snapshot` 包装方法
- [x] 6.5 `ProfileRepository for ClaudeEnvStore` 实现删除对应 3 个 impl 块
- [x] 6.6 删除文件顶部 `use cowboy::claude_env::ClaudeSettingsSnapshot`

## 7. TUI 状态层:`src/app/mod.rs`

- [x] 7.1 删除 `use cowboy::claude_env::ClaudeSettingsSnapshot`
- [x] 7.2 删除 `AppState` 里 `pub snapshots: Vec<ClaudeSettingsSnapshot>` 字段
- [x] 7.3 修改 `AppState::new()` 里 `snapshots: Vec::new()` 字段初始化
- [x] 7.4 `reload_profiles` 调用点解构:删除 `(profiles, snapshots, active_profile_name)` 中 `snapshots`
- [x] 7.5 删除 `complete_initial_load` 里 `snapshots` 字段赋值
- [x] 7.6 删除 keymap 里 Enter/c/d 在 snapshot 上的特殊分支
- [x] 7.7 删除 `c_key_on_snapshot_list_does_not_open_copy_modal` 等 snapshot 相关测试,改为 `c_key_on_profile_list_opens_copy_modal_even_when_cursor_at_boundary`
- [x] 7.8 删除 `create_snapshot` 在 profile 编辑器入口的调用
- [x] 7.9 修改测试 fixture `create_test_app` / `app.state.snapshots = vec![...]` 那段

## 8. UI 布局:`src/ui/layout.rs`、`src/ui/mod.rs`

- [x] 8.1 `src/ui/layout.rs` 删除 `pub snapshots: Rect` 字段;profiles 占满中间列;测试断言改为 `layout.profiles.bottom() == layout.status.top()`
- [x] 8.2 `src/ui/mod.rs` 删除 `let snapshot_items = ...` 整段、`snapshot_state` 与 `ListState::default()` 那段、`List::new(snapshot_items).block(...).render(...)` 调用
- [x] 8.3 删除 `state.profile_cursor >= state.profiles.len() && !state.snapshots.is_empty()` 这类跨区判断

## 9. CLI 层:`src/cmd/mod.rs`、`src/cmd/config.rs`、`src/cmd/help.rs`

- [x] 9.1 `src/cmd/mod.rs` 删除 `HistoryCommand` 枚举;`ConfigCommand::History(HistoryCommand)` 变体删除
- [x] 9.2 `src/cmd/config.rs` 删除 `parse_history_args`、`parse_snapshot_id`、`handle_history` 函数
- [x] 9.3 `parse_config_args` 删除 `history` 分支
- [x] 9.4 `handle_config_with_writer` 删除 `ConfigCommand::History(...)` 分支
- [x] 9.5 `CONFIG_USAGE` 字符串更新
- [x] 9.6 删除测试 `parses_all_history_commands`、`rejects_invalid_snapshot_ids_and_keep_counts`、`history_show_prints_snapshot_json_and_list_uses_byte_count`,新增 `rejects_history_subcommand`
- [x] 9.7 `src/cmd/help.rs` 删除 5 行 `config history *` help 文本

## 10. 启动流程:`src/main.rs`

- [x] 10.1 `initialize_env_store` 在 `initialize()` 之后、`seed_default_settings()` 之前调用 `env_store.perform_initial_backup()`
- [x] 10.2 `spawn_initial_load_worker` 闭包里的 `(profiles.list_profiles()?, profiles.list_snapshots()?, profiles.active_profile_name()?)` 三元组改为 `(profiles.list_profiles()?, profiles.active_profile_name()?)`

## 11. 新功能测试(正向)

- [x] 11.1 `src/claude_env/profiles.rs` 测试:`perform_initial_backup_copies_existing_settings_file`
- [x] 11.2 `src/claude_env/profiles.rs` 测试:`perform_initial_backup_is_idempotent_via_flag`
- [x] 11.3 `src/claude_env/profiles.rs` 测试:`perform_initial_backup_skipped_when_settings_is_symlink`(`#[cfg(unix)]`)
- [x] 11.4 `src/claude_env/profiles.rs` 测试:`perform_initial_backup_skipped_when_settings_missing`
- [x] 11.5 `src/claude_env/profiles.rs` 测试:`backup_file_has_private_permissions_on_unix`(`#[cfg(unix)]`)
- [x] 11.6 `src/claude_env/schema.rs` 测试:`v3_to_v4_migration_drops_snapshot_table_and_writes_flag`
- [x] 11.7 `src/claude_env/schema.rs` 测试:`v3_to_v4_migration_removes_snapshot_journal_kind`

## 12. 文档

- [x] 12.1 `README.md`
- [x] 12.2 `docs/02-architecture.md`
- [x] 12.3 `docs/03-ui.md`
- [x] 12.4 `docs/05-decisions.md`
- [x] 12.5 `docs/06-interfaces.md`
- [x] 12.6 `docs/07-profiles-plan.md`

## 13. 验收

- [x] 13.1 `cargo fmt --check` 通过
- [x] 13.2 `cargo test` 149 lib tests + 100 binary tests pass(仅 1 个 pre-existing failure `p_key_on_open_here_is_ignored` 跟本变更无关)
- [x] 13.3 `cargo clippy --all-targets --all-features -- -D warnings` 零警告
- [x] 13.4 `speckit validate --change remove-snapshot-feature` 通过(只剩 `Why` 段 > 1000 字符的 WARNING)
