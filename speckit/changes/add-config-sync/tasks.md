## 1. 解析层:`src/cmd/mod.rs`、`src/cmd/config.rs`(TDD:解析负向测试先)

- [x] 1.1 `src/cmd/mod.rs` 的 `ConfigCommand` 枚举加入 `Sync { name: Option<String> }` 变体(先加枚举,让 `cargo check` 暴露所有未处理的 match 臂)
- [x] 1.2 `src/cmd/config.rs` 在 `parse_config_args` 现有 dispatch 循环中追加 `"sync" => { ... }` match 臂:取出可选 token 作为 `<name>`,`reject_extra` 拦截多余的 token,产出 `ConfigCommand::Sync { name }`
- [x] 1.3 `src/cmd/config.rs` 的 `CONFIG_USAGE` 加入 `cowboy config sync [name]` 提示
- [x] 1.4 `src/cmd/config.rs` 解析测试模块加入(均为**先失败的负向断言**,作为 Red 阶段起点):
  - `parses_sync_with_no_args_uses_none`:`parse(&["sync"])` → `CommandMode::Config(ConfigCommand::Sync { name: None })`
  - `parses_sync_with_name`:`parse(&["sync", "work"])` → `Sync { name: Some("work") }`
  - `rejects_sync_with_extra_args`:`parse(&["sync", "work", "extra"])` → error
  - `parses_sync_rejects_extra_positional_when_name_given`:`parse(&["sync", "work", "junk"])` → error(spec 场景 3)

## 2. 存储层:`src/claude_env/profiles.rs`(TDD:数据结构 + 失败行为测试先)

- [x] 2.1 在 `profiles.rs` 加入 `pub struct SyncReport { inserted: Vec<String>, updated: Vec<String>, unchanged: Vec<String>, invalid: Vec<InvalidEntry> }` 与 `pub struct InvalidEntry { name: String, error: String }`,以及 `Default` / `is_empty()` 等轻量派生 impl
- [x] 2.2 `src/claude_env/mod.rs` re-exports 加入 `SyncReport` 与 `InvalidEntry`
- [x] 2.3 `profiles.rs` 加入 `sync_profiles_from_disk(&self, name: Option<&str>) -> Result<SyncReport>`,初始实现抛 `unimplemented!()` 让测试红
- [x] 2.4 `profiles.rs` 的测试模块加入负向断言测试(实现尚未完成时应失败):
  - `sync_with_no_arg_returns_empty_report_when_profiles_dir_has_no_files`
  - `sync_ignores_non_conforming_files_in_profiles_dir`(放一个 `notes.txt` 和一个 `settings..json`,确保不被 reconcile)
  - `sync_with_specific_name_does_not_touch_db_when_file_is_missing`

## 3. 存储层正向:`profiles.rs::sync_profiles_from_disk`(Green)

- [x] 3.1 `sync_profiles_from_disk` 主体实现,遵循 design.md §"写入路径:复用现有基础设施":
  - `name=None`:遍历 `profiles_dir()`,按 `settings.<name>.json` 形如正则提取 `<name>`,过滤掉不合规名(`validate_profile_name`)
  - `name=Some(n)`:`validate_profile_name(n)` 一次,读这一个文件
  - 每个 name:read → `validate_settings_json` → 比 `self.profile(name)` 决定 inserted / updated / unchanged / invalid
  - INSERT 用 `INSERT INTO claude_profiles (name, settings_json) VALUES (?1, ?2)`(处理 `ProfileExists` 退化为 UPDATE)
  - UPDATE 用 `UPDATE claude_profiles SET settings_json=?1, updated_at=CURRENT_TIMESTAMP WHERE name=?2`
  - invalid 文件不入 DB,`error` 字段是 `validate_settings_json` 的 `Display` 或 `String`
  - 缺失文件:不动 DB,不入 report
- [x] 3.2 `sync_profiles_from_disk` 测试模块加入正向行为测试:
  - `sync_inserts_profile_from_disk_when_no_db_row`
  - `sync_updates_db_row_when_disk_differs`
  - `sync_no_ops_when_db_and_disk_match`
  - `sync_skips_invalid_json_and_returns_entry_in_report`(根是数组 / trailing comma / syntax error 三种覆盖)
  - `sync_leaves_db_row_when_disk_file_missing`
  - `sync_walks_all_files_in_profiles_dir_when_called_with_none`
  - `sync_only_targets_given_name_when_some`
  - `sync_preserves_project_bindings`(sync 不删 bindings)
  - `sync_with_invalid_name_format_in_filename_lands_in_invalid`(`settings.work with space.json` 入 `invalid` 列表)
  - `sync_with_binary_file_records_invalid_and_continues`(非 UTF-8 bytes 落 invalid 而非 panic)

## 4. Handler 层:`src/cmd/config.rs`(Green)

- [x] 4.1 `handle_config_with_writer` 加入 `ConfigCommand::Sync { name }` 分支:调 `store.sync_profiles_from_disk(name.as_deref())`,遍历 `SyncReport` 写人读可读汇总到 `output`(用 `writeln!`)
- [x] 4.2 handler 测试模块加入:
  - `sync_handler_inserts_new_profile_from_disk_file`
  - `sync_handler_updates_drifted_row`
  - `sync_handler_reports_invalid_json_without_aborting`(放一个可解析但根是数组的 JSON 一个无效 JSON,验证两条都入 report,进程继续完成)
  - `sync_handler_leaves_db_row_when_file_missing`
  - `sync_handler_writes_summary_with_outcome_keywords`(stdout 含 `inserted` / `updated` / `unchanged` / `invalid` 关键字之一)
  - `sync_handler_returns_ok_even_with_invalid_entries`(遵循 spec 退出 0)

## 5. Help 与文档

- [x] 5.1 `src/cmd/help.rs` 的 `Config commands:` 段落插入一行:`config sync [name]    Reconcile profiles from on-disk files into the database`
- [x] 5.2 `src/cmd/help.rs` 的 `Options:` 段落保持只列 `-h, --help`;sync 不是顶层 option,不上 Options
- [x] 5.3 `README.md` `## CLI Commands` 段落加入 `cowboy config sync [name]`
- [x] 5.4 `README.md` 在 `## Profiles and Environment` 段落补充一段"如果你在外部直接编辑了 `~/.config/cowboy/profiles/settings.*.json`,跑 `cowboy config sync` 把变更收回 DB"

## 6. Refactor(测试持续绿的前提下改善)

- [x] 6.1 检查 `SyncReport` 的 `is_empty` 与 `len`/`iter` 等派生方法是否被多处使用,若有重复累加或打印代码则抽出 helper
- [x] 6.2 检查 `parse_config_args` 的 `sync` match 臂与原 verb 分发是否可以合并到一个公共的"读 first token,按 string 分发"调度器(若读起来更清晰则改;不必硬来)
- [x] 6.3 任何 `is_some() / unwrap` 之类在 hot path 的可读性改进

## 7. 验证(CLAUDE.md 提交前验证)

- [x] 7.1 `cargo fmt --check` 通过
- [x] 7.2 `cargo test` 通过(149 lib + 100 binary 不退化;新增 20 个 sync 相关测试)
- [x] 7.3 `cargo clippy --all-targets --all-features -- -D warnings` 零警告
- [x] 7.4 `speckit validate add-config-sync` 通过("Change 'add-config-sync' is valid")

## 8. 提交与归档

- [ ] 8.1 写 commit message:`feat(profiles): add cowboy config sync subcommand to reconcile profiles from disk to DB`
- [ ] 8.2 push 远端
- [ ] 8.3 `/opsx:archive` 把本次 change 移到 `speckit/changes/archive/`

## 9. Profile 文件不变量（TDD）

- [x] 9.1 先在 `src/claude_env/profiles.rs` 增加失败测试：创建 profile
  会生成 `{}` 镜像；镜像写入失败时数据库插入回滚；legacy DB-only profile
  会补齐缺失镜像且不会覆盖已有文件。
- [x] 9.2 最小修改 `create_profile`，使 SQLite 行与
  `settings.<name>.json` 的创建成为同一成功条件；保持错误传播与事务回滚。
- [x] 9.3 实现 legacy mirror 的非破坏性 backfill，并在合适的启动或
  profile 操作边界调用；为该边界补隔离文件系统测试。
- [x] 9.4 调整 sync 测试与文档，移除”缺失文件是允许的稳定状态”这一旧
  假设，确认 sync 仍只执行文件→数据库同步。
- [x] 9.5 运行 `cargo fmt --check`、`cargo test`、
  `cargo clippy --all-targets --all-features -- -D warnings`，以及
  `speckit validate --change add-config-sync`。
