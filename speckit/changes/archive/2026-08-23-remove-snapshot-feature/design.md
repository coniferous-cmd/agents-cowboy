## Context

`snapshot` 这次移除同时改动了 5 个层次(SQLite schema、领域类型、存储层、UI/CLI、文档),但核心替换逻辑只有一块:**首启备份**。这一节集中描述它。

备份发生的位置、时机、标志位的存储方式、迁移期如何和老数据库共存,是设计里需要明确的几件事。其余改动(删表、删命令、删 TUI 子面板)在 proposal.md 里按文件列出,这里只解释为什么这么做。

## Goals / Non-Goals

**Goals**

- 把"用户被改坏之后想回到最初"这个需求从一个持续增长的 SQLite 表,降级为一个一次性文件备份
- 备份对现有激活路径零侵入 —— `perform_activation` 的事务步骤里不再有 INSERT
- 老用户升级到 v4 时,无需手工操作即可获得同样的"回到最初"能力

**Non-Goals**

- 不提供时间序列历史(用户用 `cp` 备份自己的 profile JSON 文件)
- 不试图恢复老用户已经写过的快照行(直接 DROP)
- 不引入新的 CLI 子命令去管理备份(用户用 `cp settings.json.cowboy-backup settings.json` 即可)
- 不解决 symlink 替换的微秒窗口风险(原子 rename + 进程被杀概率足够低,接受)

## Decisions

### 备份位置:`<claude_config_dir>/settings.json.cowboy-backup`

**原因**:与 Claude 自己的 `settings.json` 同目录,`ls ~/.claude/` 即可发现。回滚命令 `cp` 自然,无需文档检索。

**替代方案**:

- 牛仔自己的数据目录(~/Library/Application Support/cowby/...):不污染 Claude 目录,但用户得读文档才知道在哪 —— 拒绝
- 加版本号后缀:`settings.json.cowboy-v0.bak`:为以后再加备份预留位置,但当前只有一个文件,过度设计 —— 拒绝

### 触发时机:`initialize_env_store` 的早期

```
1. ClaudeEnvStore::from_home
2. initialize        // 建表/迁移
3. **perform_initial_backup**   // 新增:条件性复制 settings.json
4. seed_default_settings
5. seed_default_theme
6. recover_profile_activation
```

**原因**:`initialize` 之后 SQLite 已经 ready,可以读 `settings` 表里的 `initial_backup_done`。在 `seed_default_settings` 之前做备份,避免被默认配置覆盖。`recover_profile_activation` 之前完成,保证任何潜在恢复路径看到的都是备份后的世界。

**`perform_initial_backup` 的步骤**:

1. 读 `claude_config_dir` 设置(或取默认)
2. 计算 `<dir>/settings.json` 和 `<dir>/settings.json.cowboy-backup`
3. 如果 `settings.initial_backup_done == Some("1")` → 直接返回
4. 如果目标文件不是常规文件(包括符号链接或不存在)→ 直接写标志然后返回
5. 读源文件字节 → `AtomicReplace::write(backup_path, bytes)` → 写标志
6. 用现有的 `ensure_private_file` 工具设置 0600

**关键不变量**:写标志与写备份**没有放在一个事务里**(跨文件系统)。极端情况下备份写完但进程被杀,标志没写,下次启动会再备份一次 —— 但备份目标文件已存在,直接 `AtomicReplace::write` 覆盖(原子,安全)。所以可以接受。

### 标志存储:`settings` 表的 `initial_backup_done` 键

**原因**:复用现有的键值表,不新增 schema。值固定为字符串 `"1"`,与其他 `settings` 行的写法保持一致。

**`initial_backup_done` 的语义边界**:

- 标志存在且为 `"1"` → 不再备份,**无论备份文件是否还在磁盘上**
- 标志缺失或不是 `"1"` → 走首次备份路径

如果用户手动 `rm settings.json.cowboy-backup`,下次启动不会重建。这是有意为之 —— 标志保护的是"备份动作",不是"备份文件存在性"。

### Schema 迁移 v3 → v4

`migrate_v3_to_v4(connection)` 的步骤:

1. **备份现有 settings.json**(如果存在)到 `settings.json.cowboy-backup`,逻辑与首启备份相同 —— 复用 `perform_initial_backup`
2. `DROP TABLE claude_settings_snapshots`
3. 删 `profile_activation_journal` 中残留的 `kind='snapshot'` 行(理论上不会存在,journal 在每次成功激活后清)
4. 重建 `profile_activation_journal` 表,新 schema 去掉 `target_kind = 'snapshot'` 这个 CHECK 取值和 `target_name IS NULL` 约束
5. `INSERT OR REPLACE INTO settings(key,value) VALUES ('initial_backup_done','1')`
6. `PRAGMA user_version = 4`

**为什么不导出旧快照**:用户面需求已经被新的 `settings.json.cowboy-backup` 覆盖。导出代码、UI、文档都得加,收益仅对极少数历史重用户成立。release notes 说明即可。

**为什么 DROP TABLE 而不是 RENAME 保留**:SQLite 的 ALTER TABLE 不支持改 CHECK 约束;必须重建 `profile_activation_journal`。既然要重建 journal,顺手把 `claude_settings_snapshots` 也 DROP 掉。

### 移除 `perform_activation` 的 snapshot_source 参数

从签名删除 `snapshot_source: Option<String>`,调用点不再传 `Some(format!("pre-activate:{name}"))`。函数体里"if let (Some(raw), Some(source)) = (current, snapshot_source)" 这段 INSERT 整段删除。

### TUI layout 简化

```rust
// before
profile_rows: [profiles_area, snapshots_area]

// after  
profile_rows: [profiles_area]   // 占满 tab 高度
```

`Layout.snapshots` 字段删除。`state.profile_cursor` 范围回到 `[0, profiles.len())`,跨快照索引的所有判断删除。

### 测试策略

按 CLAUDE.md 的 TDD 要求,**先写失败测试再删代码**:

1. **负向断言测试**(删除前先加):
   - `snapshots_table_does_not_exist_after_initialize`
   - `config_history_subcommand_rejected`
   - `no_snapshot_methods_on_profile_repository`
   - `tui_does_not_render_snapshots_panel`
2. **正向新行为测试**:
   - `first_launch_backup_copies_existing_settings_file`
   - `first_launch_backup_is_idempotent_via_flag`
   - `first_launch_backup_skipped_when_settings_is_symlink`
   - `first_launch_backup_skipped_when_settings_missing`
   - `backup_file_has_private_permissions`
   - `v3_to_v4_migration_drops_snapshot_table_and_backs_up_file`
   - `v3_to_v4_migration_removes_snapshot_journal_kind`
   - `v3_to_v4_migration_writes_initial_backup_done_flag`

## Risks / Trade-offs

### 风险 1:symlink 替换微秒窗口

**描述**:Unix `replace_with_symlink` 是 `fs::remove_file(link)` + `symlink(...)`,中间有一刻 `settings.json` 不存在。如果进程在那窗口被杀,用户失去当前 settings 链接,但文件内容本身在 `<profiles_dir>/settings.<name>.json` 里还是完整的,只是 `~/.claude/settings.json` 这个 symlink 不在了。

**现状**:该窗口只有微秒级。修复方法是 atomic rename(用 `renameat2` 的 `RENAME_EXCHANGE`,Linux 3.15+)或 macOS 的 `renamex_np`,代码复杂度上升很多。

**本变更的处理**:接受这个窗口风险,通过 release notes 让用户知道:"如果 cowboy 崩溃在激活中,你需要从 `settings.json.cowboy-backup` 或某个 profile 的 `settings.<name>.json` 手动恢复 symlink。"

### 风险 2:首次备份覆盖了用户已经手动备份的同名文件

**描述**:如果用户之前已经放了一个 `settings.json.cowboy-backup` 在那里,首次启动会被覆盖。

**现状**:这个文件名是 cowboy 自己选的,公开协议。用户不太可能事先用这个特定名字。

**本变更的处理**:文档里写明。如果未来要更名,改这里一处即可。

### 风险 3:`initial_backup_done` 标志无法跨数据库移动

**描述**:如果用户把 cowboy 的数据库搬到另一台机器、或重装系统后导入旧数据库,标志会带着走 —— 即使新机器的 `~/.claude/settings.json` 是另一份内容,也不会再备份。

**现状**:这是预期行为。用户主动恢复数据库,应该预期所有"已做过的事"都迁移了。

**本变更的处理**:文档里写明。如果想重新触发备份,删 `settings` 表里那行 `initial_backup_done`。

### 风险 4:已有 v3 用户的快照行被静默丢弃

**描述**:v4 migration 不导出历史快照,数据不可逆丢失。

**现状**:对于"回到最初"这个需求,新的 `settings.json.cowboy-backup` 覆盖了;但"回到上周三"这种细粒度历史彻底没了。

**本变更的处理**:靠 release notes 高亮说明,用户主动选择升级时间。如果未来有人真的需要历史,可以写一个小迁移脚本读 v3 DB 导出。

### 权衡:简化 vs 灵活性

整个机制从"100 条历史 + 自动恢复" 简化成"1 个文件 + 手动恢复"。换来的是:
- `perform_activation` 不再需要事务里的额外 INSERT
- `finish_activation` 不再需要 prune 步骤
- `profile_activation_journal` 的 CHECK 约束不再需要 `'snapshot'` 这个分支
- TUI 不再需要双面板,光标索引不需要"翻译"
- 测试套件可以删掉所有 snapshot-related 测试

代码量减少大约 200 行,激活路径更直接。代价是用户在某个时刻只能回到"牛仔第一次看到的那份配置",不能回到"上周三激活的那份"。
