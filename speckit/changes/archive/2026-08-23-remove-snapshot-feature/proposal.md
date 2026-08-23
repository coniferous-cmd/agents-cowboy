## Why

当前 cowboy 每次激活 profile 都会把当前 `~/.claude/settings.json` 的内容写一条 SQLite 快照保留下来,最多 100 条。代码里这套机制同时承担两个角色:

1. **公开的历史回滚** — TUI 的 snapshots 子面板 + `cowboy config history *` 命令
2. **隐式的事前备份** — `perform_activation` 在每个 symlink 替换前自动写入一条,作为激活失败时的恢复点

经验上发现:

- 公开历史的实际使用频率很低,绝大多数用户只在"刚改坏了"那一次想回滚,不需要 100 条
- 隐式备份的恢复价值被 symlink 替换的原子性覆盖得差不多了 —— 只有进程在 symlink 替换的微秒窗口被杀才需要它
- 同时维护两张逻辑(用户面 UI/CLI + 内部 SQLite 表)和 100 条上限,反而让激活路径变得复杂

把整套机制删掉,换成一个**首次启动一次性文件备份**:`cowboy` 第一次看到 `settings.json` 时把它复制为同目录的 `settings.json.cowboy-backup`,之后不再写任何快照。覆盖了"回到最初状态"的恢复需求,代码面也清爽很多。

## What Changes

- **删除 `claude_settings_snapshots` 表** —— 包括 `list_snapshots` / `create_snapshot` / `snapshot(id)` / `delete_snapshot` / `prune_snapshots` / `activate_snapshot(id)` 这 6 个 CRUD 方法,以及 `ClaudeSettingsSnapshot` 结构体、`StetsonError::SnapshotNotFound` 错误变体、`AUTO_SNAPSHOT_LIMIT` 常量和 `prune_snapshots_on` helper
- **简化 profile 激活路径** —— `perform_activation` 不再写 snapshot,`snapshot_source` 参数被移除;`finish_activation` 不再调用 `prune_snapshots_on`
- **简化 journal schema** —— `profile_activation_journal.target_kind` 的 CHECK 约束改为只允许 `'profile'`;`target_name IS NULL` 这条约束删除;snapshot 激活产生的 `kind='snapshot'` journal 行不再可能出现
- **简化 TUI** —— 删除 Profiles 面板下半部分的 Snapshots 子面板;光标只在 profile 列表里移动;layout 简化为单个 `profile_rows`
- **简化 keymap** —— 删除针对 snapshot 的特殊分支:`Enter` 在 snapshot 上回滚的逻辑;`c`/`d` 在 snapshot 上禁用复制/删除的注释
- **删除 `create_snapshot` 在 profile 编辑器入口的调用** —— 不再为"打开编辑器前先备份当前"专门存一条
- **删除 CLI 子命令** —— `cowboy config history` 整个删除;`HistoryCommand` 枚举、`parse_history_args`、`parse_snapshot_id` 全部移除
- **更新 help 文本** —— `print_help` 移除 history 相关行
- **新增首次启动备份行为** —— `Cowboy` 初始化时,如果 `claude_config_dir/settings.json` 存在且 `settings` 表里 `initial_backup_done = '1'` 尚未设置,就把它复制为同目录的 `settings.json.cowboy-backup`,然后写入标志
- **数据库迁移 v3 → v4** —— 升级老用户数据库时 DROP TABLE `claude_settings_snapshots`,更新 journal 的 CHECK 约束;如果当前 `settings.json` 存在,执行同样的首启备份;迁移完成即翻 `initial_backup_done`

## Capabilities

### New Capabilities

- `first-launch-backup`: 一次性把 `settings.json` 复制为 `settings.json.cowboy-backup`,由 `initial_backup_done` 标志保护,后续运行不再重复执行

### Modified Capabilities

- `profiles`: 删除 `claude_settings_snapshots` 表、`Snapshot Table` 章节、所有快照相关测试,激活流程步骤 4 改为"不捕获快照,继续"
- `profile-activation`: 删除 `Snapshot activation writes an orphan file` 需求及其场景,`perform_activation` 不再写 snapshot,journal 的 `target_kind` CHECK 简化

## Impact

### 删除/修改的代码

- `src/claude_env/schema.rs`:从 `PROFILES_SCHEMA` 删除 `claude_settings_snapshots` 表;更新 `profile_activation_journal` 的 CHECK 约束;新增 `migrate_v3_to_v4` 函数;schema version 升到 4
- `src/claude_env/profiles.rs`:删除 `ClaudeSettingsSnapshot`、`list_snapshots`、`create_snapshot`、`snapshot`、`delete_snapshot`、`prune_snapshots`、`activate_snapshot`、`prune_snapshots_on`、`AUTO_SNAPSHOT_LIMIT`;`perform_activation` 拿掉 `snapshot_source` 参数和写入逻辑;`finish_activation` 删除 `prune_snapshots_on` 调用;`activate_profile` 不再传 snapshot 标签;新增 `perform_initial_backup(settings_path)` 函数
- `src/domain.rs`:删除 `StetsonError::SnapshotNotFound`
- `src/claude_env/mod.rs`:从 re-exports 移除 `ClaudeSettingsSnapshot`
- `src/lib.rs`:同步 re-exports
- `src/application.rs`:`ProfileRepository` trait 移除 `list_snapshots`/`activate_snapshot`/`create_snapshot` 三个方法及其在 `NoProfileRepository` / `ClaudeEnvStore` 实现里的对应代码;`StetsonApplication` 同步删除包装方法;`load_profile_data` 返回类型从 `(Vec<ClaudeProfile>, Vec<ClaudeSettingsSnapshot>, Option<String>)` 简化为 `(Vec<ClaudeProfile>, Option<String>)`
- `src/app/mod.rs`:`AppState.snapshots` 字段删除;`profile_cursor` 索引范围回到 `[0, profiles.len())`;`reload_profiles` 不再加载 snapshots;`complete_initial_load` 解构简化;删除 keymap 里针对 snapshot 的特殊分支(包括 `c_key_on_snapshot_list_does_not_open_copy_modal` 等测试);`create_snapshot` 调用点删除
- `src/ui/layout.rs`:`Layout.snapshots` 字段删除,`profile_rows` 数组从 2 项简化为 1 项,相关测试更新
- `src/ui/mod.rs`:删除 snapshots 子列表的渲染代码,`state.profile_cursor >= state.profiles.len()` 跨快照判断删除
- `src/cmd/mod.rs`:`HistoryCommand` 枚举删除;`ConfigCommand::History` 变体删除
- `src/cmd/config.rs`:`parse_history_args`、`parse_snapshot_id`、`handle_history` 函数全删;`parse_config_args` 不再分发 `history` 子命令;help 文本更新;`parses_all_history_commands`、`rejects_invalid_snapshot_ids_and_keep_counts`、`history_show_prints_snapshot_json_and_list_uses_byte_count` 三个测试删除
- `src/cmd/help.rs`:删除 5 行 history 子命令的 help 文本
- `src/main.rs`:`spawn_initial_load_worker` 里 `list_snapshots` 调用删除;`initialize_env_store` 在 `seed_default_settings` 之前调用新加的 `perform_initial_backup`
- `tests/` 目录:任何引用 snapshot 的集成测试需要更新

### 数据库迁移

升级到 v4 的迁移逻辑(`migrate_v3_to_v4`)需要:
1. 备份当前 `~/.claude/settings.json` 到 `settings.json.cowboy-backup`(如果存在)
2. DROP TABLE `claude_settings_snapshots`
3. 删除 `profile_activation_journal` 中残留的 `kind='snapshot'` 行(理论上不会存在,journal 在每次成功激活后被清)
4. 重建 `profile_activation_journal` 表去掉 `snapshot` 这个 CHECK 取值和 `target_name` 的 NULL 约束
5. 写入 `initial_backup_done = '1'`
6. 把 schema version 写到 4

老用户的快照数据会被静默丢弃(无导出、无提示),靠 release notes 说明。

### 用户体验

- TUI 的 Profiles 面板不再分两半,profile 列表占满高度
- CLI 不再支持 `cowboy config history *`
- 如果用户在某个时刻想要"回到最初",命令是 `cp ~/.claude/settings.json.cowboy-backup ~/.claude/settings.json`,需要自己手动(文档会写)
- `~/.claude/settings.json.cowboy-backup` 是 cowboy 在 Claude 自己的目录里写入的第一个也是唯一一个文件,ls 即可发现

### 依赖

无外部依赖变更。
