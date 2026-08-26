## Why

cowboy 已经在维护一份**双向绑定**:每个 profile 在 SQLite `claude_profiles` 表里有一行,同时在 `~/.config/cowboy/profiles/settings.<name>.json` 磁盘上有一份镜像文件。每次 `update_profile_json` / `copy_profile` / `perform_activation` 都会在同一个 SQLite 事务里把这两边都写一致(`profiles.rs:155-174`、`profiles.rs:222-247`、`profiles.rs:343`)。

但**只有"DB → 文件"这一侧**:所有现有写入路径都是先把内容落 SQLite,再 `AtomicReplace::write` 到磁盘。如果用户(或外部工具)直接编辑了 `~/.config/cowboy/profiles/settings.work.json`——比如用 `vim` 改了一行 env、加了个新 hook——SQLite 行的 `settings_json` 不会跟着变,`cowboy config list` 看到的 profile 元数据还是旧的,`profile(name).settings_json` 读出来的也是旧的。下次 `update_profile_json` 一跑,用户的手动改动会被覆盖回 DB 当前的内容——数据丢失。

`~/.claude/settings.json`(一个符号链接)指向那个 profile 文件本身,所以 Claude 进程读到的是磁盘的最新版本,**只有 cowboy 自己的 DB view 漂移了**。

需要一种"反向同步"能力:把磁盘上的 `settings.<name>.json` 收回来,写入 SQLite 行。这种场景在牛仔生态里典型但不紧急——DB 是 metadata only(`README.md` 明确写),所以漂移不影响 Claude 运行时,只影响牛仔展示和后续编辑。

## What Changes

- **新增 `cowboy config sync [name]` CLI 子命令**——`sync` 是 `config` 子命令下与 `list`/`create`/`edit`/`delete`/`activate`/`bind`/`unbind`/`copy` 平级的新动词,跟现有 parser dispatch 直接对接,不需要前置 token 抢占
  - `cowboy config sync` 扫描 `profiles/` 目录里所有 `settings.<name>.json`,对每个走"读 → 校验 → 写 DB"流程
  - `cowboy config sync <name>` 只同步指定名字的那个文件
  - `cowboy config sync` 不接受额外的位置参数(`<name>` 后面的 token 一律报错),跟现有 `bind` / `copy` 的限制方式一致
- **新增 `ClaudeEnvStore::sync_profiles_from_disk` 存储层方法**——传入可选 `name: Option<&str>`:
  - `None` → 列出 `profiles_dir()` 里所有 `settings.*.json`,按上述规则逐个 reconcile
  - `Some(name)` → 只 reconcile 那个名字
  - **不做的事**:不动 `~/.claude/settings.json` 符号链接;不删 DB 里"磁盘没对应文件"的行;不碰 bindings
- **复用现有 `update_profile_json` 事务**——sync 路径不另写一套 DB+文件双写逻辑,而是复用 `update_profile_json(..., &new_json)` / `create_profile` (后者只填 `{}`,这里需要补一个 `create_profile_with_json` 变体或扩展 `create_profile` 的签名)。这样"DB+文件一个事务、文件存在/权限/原子替换"的不变量自动继承。
- **JSON 校验失败策略**——`validate_settings_json` 失败的 profile:不抛错中止 sync,跳过那条、累计到错误列表,继续处理其他 profile。CLI exit code:全部成功 → `0`;部分失败 → `0`(在 stderr/stdout 列出失败清单,语义上属于"成功完成")；参数错误(目录读不到等) → 非零
- **CLI 输出格式**——stdout 逐行列出每个被 reconcile 的 profile 与结果(`inserted` / `updated` / `unchanged` / `invalid JSON` / `not found on disk`),便于 `cowboy config sync && git diff` 之类的人工会审
- **不动现有行为**:`create`/`edit`/`copy`/`delete`/`activate`/`bind`/`unbind` 全部不修改;`update_profile_json` 行为不变(还是"先 DB 后文件");`perform_activation` 行为不变(还是写自己的 journal + 写文件 + 替换 symlink)
- **更新 `print_help`**——`config` 子命令的 help 加入 `sync` 子命令说明
- **更新 `README.md`**——CLI 命令章节与"Profile 同步"小节

## Capabilities

### New Capabilities

- `profile-disk-sync`: 让 `cowboy config sync` 把 `~/.config/cowboy/profiles/` 里的 `settings.<name>.json` 收回到 SQLite `claude_profiles` 表里——支持 INSERT / UPDATE / no-op 三种 reconcile 结果,以及 invalid JSON 跳过的错误路径

### Modified Capabilities

无。这是纯增量变更,没有修改任何已有 capability 的 requirement;`profiles` / `profile-activation` / `first-launch-backup` 等已有 capability 的 REQUIREMENTS 不变。

## Impact

### 修改的代码

- `src/cmd/config.rs`:
  - `parse_config_args` 在现有 dispatch 循环里追加 `"sync"` 分支:取出可选的 `<name>`(允许缺失),`reject_extra` 拦截多余 token,产出 `ConfigCommand::Sync { name: Option<String> }` 变体
  - `handle_config_with_writer` 加 `ConfigCommand::Sync { name }` 分支,调用 `store.sync_profiles_from_disk(name.as_deref())`
  - `CONFIG_USAGE` 字符串加入 `sync` 提示
  - 新增解析测试:`parses_sync_with_no_args_uses_none` / `parses_sync_with_name` / `parses_sync_rejects_extra_args`
  - 新增 handler 测试:`sync_handler_inserts_new_profile_from_disk_file` / `sync_handler_updates_drifted_row` / `sync_handler_reports_invalid_json_without_aborting` / `sync_handler_leaves_db_row_when_file_missing`
- `src/cmd/mod.rs`:
  - `ConfigCommand` 枚举新增 `Sync { name: Option<String> }` 变体
  - 之前在文件里看到 `History` / `bind` / `unbind` 都在同一枚举里,加一个变体不破坏兼容性
- `src/cmd/help.rs`:
  - `Config commands:` 段落加入 `config sync [name]` 一行,可选提示一句
- `src/claude_env/profiles.rs`:
  - 新增 `pub fn sync_profiles_from_disk(&self, name: Option<&str>) -> Result<SyncReport>`,内部:
    - 决定待 reconcile 的名字集合:`None` 时扫 `profiles_dir()` 列出 `settings.*.json` 抽取 `<name>`;`Some(n)` 时只用 `n`
    - 调 `validate_profile_name(name)` 标准化(已是现状)
    - 对每个 name:`fs::read + validate_settings_json + (若无行 INSERT,若有行比对后 UPDATE)`
  - INSERT 路径需要新写一行:复用 `validate_profile_name` 然后 `INSERT INTO claude_profiles (name, settings_json) VALUES (?1, ?2)`,处理 `ProfileExists` 错误时降级为 UPDATE(因为 race window 极小)。不引入 `BEGIN IMMEDIATE` 长事务,逐 profile 一笔短事务,与现有风格一致
  - 复用 `profile_file_path(&name)` / `AtomicReplace::write` 这套已经在 `update_profile_json` 里的逻辑;sync 路径**不必再写一遍文件**——sync 的语义是"磁盘是 source of truth,DB 是 mirror";磁盘已经是正确的,不需要再次覆盖
  - 新增 `pub struct SyncReport { inserted: Vec<String>, updated: Vec<String>, unchanged: Vec<String>, invalid: Vec<InvalidEntry> }` 与 `pub struct InvalidEntry { name: String, error: String }`
  - 测试:`sync_inserts_profile_from_disk_when_no_db_row` / `sync_updates_db_row_when_disk_differs` / `sync_no_ops_when_db_and_disk_match` / `sync_skips_invalid_json_and_returns_entry_in_report` / `sync_leaves_db_row_when_disk_file_missing` / `sync_walks_all_files_in_profiles_dir_when_called_with_none` / `sync_only_targets_given_name_when_some`
- `src/claude_env/mod.rs`:
  - re-exports 加入 `SyncReport` / `InvalidEntry`
- `README.md`:
  - "CLI Commands" 段落加入 `cowboy config sync [name]` 一行
  - 新增小节或修订"Profiles and Environment",说明"如何在外部编辑 settings 文件后再把变更读回 DB"

### 不动的部分

- `~/.claude/settings.json` 符号链接——`sync` 不主动修复或重新指向。漂移由 `cowboy config activate <name>` 处理(`activate_profile` 内部走 `replace_with_symlink`)
- `project_profile_bindings` 表——sync 不读、不写 bindings。绑定是逻辑概念,跟 profile 的内容无关
- `first-launch-backup` 流程——`settings.json.cowboy-backup` 与 `~/.claude/settings.json` 是另一对概念,不参与 sync
- `update_profile_json` / `create_profile` / `copy_profile` / `delete_profile` 现有签名与方法体不变(但 sync 路径可能需要 `create_profile_with_json` 之类的小工具,加在 `profiles.rs` 同文件)
- `perform_activation` 流程不变
- SQLite schema 不变——`claude_profiles` 表的列、journal 表、bindings 表都不动;schema version 仍是 4
- 不引入新依赖

### 用户体验影响

- 用户在 `~/.config/cowboy/profiles/settings.work.json` 里加了一行 env,然后跑 `cowboy config sync work`,DB 跟着更新;TUI 显示的 profile 列表点开后也是新内容
- 用户的 DB 被损坏/丢失但文件还在,跑 `cowboy config sync`(不给名字)→ 磁盘上所有 profile 一次性重建回 DB
- 没有新 TUI 入口,纯 CLI;用户不需要学新交互
- 不影响运行时(`~/.claude/settings.json` 已经是磁盘最新,Claude 进程本就看到正确内容)
