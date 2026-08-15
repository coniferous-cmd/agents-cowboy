# Profiles 配置片段计划

## 实施状态

状态：已实现（2026-08-08）。本文档是 Profiles 实现与验收的事实来源；实现过程中若发现现有架构约束与计划冲突，先修订本文档，再修改代码。

- [x] 完成 SQLite schema 迁移、旧数据安全导出与遗留表清理
- [x] 完成 Profile、Snapshot、激活 journal、原子替换及崩溃恢复服务
- [x] 完成 `config` 与 `config history` CLI
- [x] 完成 Projects / Sessions / Profiles 三标签 TUI 与激活交互
- [x] 移除旧 `export`、项目配置编辑和运行时 env 注入能力
- [x] 更新 README、架构、接口与 UI 文档并通过全部验收测试

## 目标

让用户将多个可命名的 Claude Code 设置片段保存到 cowboy 的 SQLite 元数据数据库，并在需要时激活其中一个片段，覆盖写入 `~/.claude/settings.json`。

该能力替代旧的项目级配置编辑和环境变量元数据机制；Profiles 是应用唯一保留的 Claude 设置管理入口。

## 配置片段

### 存储位置、数据模型与命名

片段存储于应用现有的 SQLite 元数据数据库：macOS 默认路径为
`~/Library/Application Support/cowboy/cowboy.db`；其他平台使用
现有的平台配置目录中的 `cowboy/cowboy.db`。

新增 `claude_profiles` 表，至少包含：

- `id INTEGER PRIMARY KEY`
- `name TEXT NOT NULL UNIQUE`：规范化后的名称
- `settings_json TEXT NOT NULL`：完整的 JSON 对象文本
- `updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP`

`name` 只能由 ASCII 字母、数字、连字符（`-`）和下划线（`_`）组成，最大 64 字符；空名称和其他字符均应被拒绝。名称不区分大小写：输入在校验后以 ASCII 小写形式写入、查找和排序，因此不会产生大小写冲突的重复记录。

`settings_json` 必须解析为 JSON 对象。片段可以包含 Claude Code 支持的任意 settings 字段，不限于 `env`。

Profiles 与 snapshots 可能含 OAuth、API key 或 MCP 密钥。创建数据库父目录时必须仅授予当前用户访问权限；数据库本体以及 SQLite 的 WAL/SHM 辅助文件也必须使用同等或更严格的用户私有权限/ACL。

为在 Profile 切换时把上一份 `~/.claude/settings.json` 完整保留在 SQLite 内——包括任何 Profile 未覆盖、由 Claude Code 直接写入的字段（如 `permissions`、`hooks`、MCP 服务器配置）——新增独立的 `claude_settings_snapshots` 表：

- `id INTEGER PRIMARY KEY`
- `captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP`
- `source TEXT`：可选来源标记，例如 `pre-activate:work`，便于追溯
- `settings_json TEXT NOT NULL`：激活前 `settings.json` 的完整 JSON 对象文本

Snapshots 由激活流程自动写入，不进入 `config list` 输出，不需要走名称校验；用户通过专门的 `config history` 子命令查看、回滚和清理。

`settings_json` 的接受标准仅限「解析为 JSON 对象」：拒绝空字符串、解析错误、`null`、数组与标量；接受任意对象，包括 `{}` 与包含 `_comment`、`$schema` 等元字段的对象。不对字段做白名单校验，Claude Code 会忽略未知 key。该规则同时作用于 `claude_profiles.settings_json` 与 `claude_settings_snapshots.settings_json`。

`settings` 表保留为通用 KV 接口，并新增一个逻辑 key `active_profile_name`，保存当前激活 Profile 的规范化名。激活成功后写入；TUI 进入 Profiles tab 时以该字段渲染 active 标记。

`active_profile_name` 的语义是「最后一次由 cowboy 成功激活的 Profile」，不是对当前文件内容的推断。激活 snapshot 或删除该 Profile 时清空该 key；Claude Code 或用户在应用外修改 `settings.json` 不改变该记录，也不进行字节比对回填。

新增 `profile_activation_journal`，用于协调无法共同提交的 SQLite 与文件系统操作：

- `id INTEGER PRIMARY KEY`
- `target_kind TEXT NOT NULL`：`profile` 或 `snapshot`
- `target_id TEXT NOT NULL`
- `target_name TEXT`：Profile 的规范化名称；snapshot 为 `NULL`，用于崩溃恢复时补齐 active 状态
- `target_json_hash TEXT NOT NULL`
- `phase TEXT NOT NULL`：`prepared`、`file_replaced` 或 `failed`
- `error TEXT`：仅 `failed` 时记录恢复失败原因
- `created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP`

journal 是单例表（固定 `id=1`，并以 `CHECK` 约束 `target_kind` 和 `phase`）。每次激活在跨进程独占锁下进行：锁文件位于数据库同目录，使用操作系统 advisory lock；同一进程内也复用该锁。启动时若存在 `prepared` 或 `file_replaced` journal：若当前 `settings.json` 的 SHA-256 等于 `target_json_hash`，则按 `target_name` 补齐 active 状态并清理 journal；不相等则把 journal 更新为 `failed`、保留已捕获的 snapshot 并提示用户。`failed` 不再自动重试；下一次显式激活在取得锁后清理旧 failed journal 并开始新操作。这样即使文件替换后进程崩溃，数据库状态也可恢复。

哈希始终针对将要写入 `settings.json` 的原始 UTF-8 字节计算；激活过程不 pretty-print、不补尾换行。Profile 删除/改名和激活共用同一跨进程锁。临时文件或原子替换失败时，在返回错误前删除本次 prepared journal，但保留已经捕获的 snapshot。

### 激活规则

激活时按以下顺序执行：

1. 取得应用级互斥锁，读取并校验目标 Profile 的 `settings_json`，再读取当前 `~/.claude/settings.json`。
2. 当前文件存在时，必须解析为合法 JSON 对象；然后在 SQLite 事务中插入其完整 snapshot（`source` 为 `pre-activate:<目标 profile 名>`）、写入 `prepared` journal 并提交。当前文件不存在时不创建 snapshot，但仍写入 journal。
3. 在目标目录写入临时文件并原子替换 `~/.claude/settings.json`；目标目录不存在时创建它。原子替换统一走 `AtomicReplace::write(&path, &bytes)` 抽象：写 `<path>.tmp.<pid>.<nanos>` → `sync_all()` → `rename`；Windows 上目标存在时只能使用 `ReplaceFileW`，目标不存在时使用 `MoveFileExW`。不得使用 `remove` 后 `rename` 回退；无法安全替换时必须失败。替换前读取目标文件的 metadata，Unix 上把 tmp 文件 chmod 到与目标相同的 mode（目标不存在时默认 `0600`，因为 settings.json 含 OAuth / API key），Windows 上把 ACL 收窄到当前用户，确保替换后权限位与 ACL 与原文件一致或更严。
4. 在 SQLite 事务中把 journal 标记为 `file_replaced`、写入 `active_profile_name`、自动 prune snapshot、清理 journal并提交。若此事务因崩溃或错误未完成，下一次启动按 journal 恢复状态。清空 active 状态统一使用 `DELETE FROM settings WHERE key='active_profile_name'`。

原子替换前的任一步骤失败都不得改写现有的 `~/.claude/settings.json`，命令和 TUI 必须返回明确错误。文件已替换但最终数据库事务未完成时，journal 是唯一允许的中间状态，并由下一次启动恢复：

- 当前 settings.json 不存在：不产生 snapshot，直接进入步骤 2。
- 当前 settings.json 不是合法 JSON 或根节点不是对象：拒绝激活，提示「fix or delete settings.json first」，原文件字节不变。
- snapshot INSERT 失败（如数据库锁）：拒绝激活，原文件字节不变。
- Profile 不存在、无法读取、JSON 无效、根节点不是对象：拒绝激活，原文件字节不变。
- 临时文件创建或原子替换失败：拒绝激活，原文件字节不变。
- 文件替换后数据库最终事务失败：文件保留新内容，保留 journal，下一次启动完成或标记该操作的恢复结果。

不要求用户确认；不在 `~/.claude/` 旁生成备份文件；上一份配置的全部恢复路径都在 SQLite 内（见「历史子命令」）。

## 命令行接口

新增 `cowboy config` 命令组：

```text
cowboy config list
cowboy config create <name>
cowboy config edit <name>
cowboy config delete <name>
cowboy config activate <name>
```

- `list` 按规范化名称的稳定升序列出所有片段。
- `create` 创建内容为 `{}` 的片段；同名记录存在时失败，不覆盖现有内容。
- `edit` 将已存在片段的 JSON 写入临时文件，并使用 `$EDITOR` 打开它。编辑器成功退出后，只有内容是 JSON 对象才在一个数据库事务中更新记录；无效编辑保留在临时文件中并输出其路径。`$EDITOR` 未设置或编辑器非零退出时不更新记录。
- `delete` 删除指定片段记录；片段不存在时返回明确错误。
- `activate <name>` 激活单一片段，直接覆盖全局 settings 文件，并输出片段名称和目标路径。

`cowboy config history` 子命令组用于管理 snapshot：

```text
cowboy config history list
cowboy config history show <id>
cowboy config history activate <id>
cowboy config history delete <id>
cowboy config history prune --keep <n>
```

- `list` 按 `captured_at DESC, id DESC` 列出 id、时间、UTF-8 字节数和 `source`。
- `show <id>` 打印该 snapshot 的 JSON 内容。
- `activate <id>` 把该 snapshot 的 JSON 原子写回 `~/.claude/settings.json`；此操作**不**再产生新 snapshot，避免递归，且允许覆盖不存在或损坏的当前 settings 文件，以提供恢复路径。它仍写入 `target_kind=snapshot` 的 journal，但跳过当前文件校验与 snapshot 捕获；成功后清空 `active_profile_name`。
- `delete <id>` 删除一条 snapshot。
- `prune --keep <n>` 保留最新的 `n` 条 snapshot 并删除其余记录（允许 `n=0`）；成功激活 Profile 后在最终事务中也自动保留最近 100 条。`show` 会输出可能含密钥的 JSON，帮助文本必须提示此风险。

## TUI 交互

主界面由三个独立标签页组成：Projects、Sessions、Profiles。

顶层标签状态与 Projects / Sessions 双栏页面内的焦点状态彼此独立。Projects 与 Sessions 标签共享现有的双栏浏览页面；通过 `[` / `]` 进入 Projects 或 Sessions 标签时，分别把栏焦点对齐到 Projects 或 Sessions，但之后使用 `Tab` / `←` / `→` 只改变双栏焦点，不改变顶部当前标签。这样顶部标签记录用户通过 `[` / `]` 选择的导航位置，双栏焦点记录当前键盘操作目标，两者不互相隐式改写。Profiles 使用独立页面和独立行焦点。

- `[` 切换至前一个标签页，`]` 切换至后一个标签页；在 Projects / Sessions / Profiles 三个标签页之间首尾循环。
- Projects / Sessions tab 内的 `Tab`、左方向键、右方向键维持原意——在 Projects 与 Sessions 两栏之间切换焦点，不承担 tab 切换。
- Profiles tab 内的 `Tab`、左方向键、右方向键**不**承担 tab 切换；键位空闲或预留为后续实现分配。
- Projects 与 Sessions 保留浏览、项目内启动新会话、恢复会话、搜索、重命名和删除等现有能力。
- Profiles 页面列出所有可用片段。
- 上下方向键移动 Profiles 焦点行。
- Profile 区域按 Enter 直接激活焦点片段。
- Profiles 页面按 `n` 可创建新 Profile：先输入唯一名称，再退出 TUI 原始模式打开 `$EDITOR` 编辑 JSON；保存有效 JSON 后写入 SQLite 并刷新列表。按 `Ctrl+D` 删除焦点上的 Profile，随后按 `Enter`、`y` 或再次 `Ctrl+D` 确认；编辑已有 Profile 仍通过 CLI 完成。
- Profiles 页面顶部列出所有可用片段，下方以只读列表展示 snapshot（按 `captured_at DESC`），每行展示 `captured_at`、字节数和 `source`。
- 上下方向键在 Profile 区域与 snapshot 区域之间连续移动焦点。
- Snapshot 行按 Enter 直接激活当前焦点上的 snapshot；该操作允许覆盖损坏文件且不产生新 snapshot。Profile 激活会在切换前捕获 snapshot。
- 列表行渲染时显示 active 标记（例如 `●` 或 ` (active)`，由 UI 决定），来源是 `settings.active_profile_name`。标记仅在 Profile 区域出现，snapshot 行不标记。

## 移除的旧能力

移除以下与项目配置或环境变量元数据有关的能力：

- 在 TUI 中打开或创建项目 `.claude/settings.json` 的流程，包括 `e` 快捷键、项目配置确认弹窗、外部项目配置编辑器调用、相关状态和提示。
- `export` CLI 命令及其帮助文本。
- SQLite 中的环境变量定义、环境变量值及项目设置元数据表和迁移逻辑；保留通用 `settings`、主题和新的 Profiles 表。
- 项目或会话环境变量解析、恢复或启动 Claude 时的环境变量覆盖，以及相关存储接口。

启动或恢复 Claude 时，不再由 cowboy 注入项目或会话环境变量，子进程仅继承当前进程环境。

`claude_envs` 元数据表被删除的理由：Claude Code 自身维护 env var 文档，第三方 curated 表易过时；Profiles 让用户直接编辑 JSON，无需元数据包装。该决定不影响运行时——本计划同样移除项目/会话 env 注入。

## 实施顺序

1. 定义并实现 SQLite schema 迁移：以 `PRAGMA user_version=1` 作为 Profiles schema 版本；fresh DB 直接创建新 schema，`user_version=0` 且存在 legacy 表的数据库执行迁移。创建 `claude_profiles`、`claude_settings_snapshots` 和 `profile_activation_journal`。Drop `claude_project_settings` 前，先把全部行序列化到实际解析出的 Claude 配置目录下的 `cowboy-migrated-<YYYYMMDDTHHMMSS.nnnnnnnnnZ>.json`（Windows-safe UTC 文件名；冲突时追加递增序号）。格式为 `{"projects":[{"path":..., "settings_json":"<原始文本>"}]}`，`settings_json` 作为 JSON 字符串保存以无损保留旧库中的任意文本。dump 使用临时文件原子写入、权限为 Unix `0600` / Windows 当前用户私有 ACL；父目录同样收窄。dump 失败则中止迁移，重试时复用内容相同的已有 dump，避免重复。配置目录优先读取迁移前 `settings.claude_config_dir` 的非空绝对路径，否则使用默认 `~/.claude`。随后在一个 `BEGIN IMMEDIATE` 事务中清理 singular、plural 及 `_legacy` 旧对象，按「env-values 索引 → env-values 表 → project-settings 表 → env-definitions 表」顺序删除、执行 `PRAGMA foreign_key_check` 并写入 schema version；保留 `settings` 和主题数据。迁移与旧 schema 初始化、env seed 的删除必须在同一版本完成；默认 settings 改为 `INSERT ... ON CONFLICT DO NOTHING`，不得覆盖用户自定义目录。
2. 实现 Profiles、Snapshots 与 journal 数据库仓储、名称校验、JSON 校验、应用级激活锁和可恢复的激活服务，并为 CLI 与 TUI 提供共享接口。
3. 添加 `config` 命令组（含 `config history` 子命令）和帮助文本。
4. 改造 TUI 为三标签布局并实现 Profiles 单选、激活与下方 snapshot 列表的展示/激活。
5. 删除旧项目配置编辑、env 数据模型、运行时 env 注入和 `export` 能力。
6. 更新 README、架构/接口/UI 文档及本计划，使命令、数据库 schema、名称校验规则、快捷键和移除项有单一事实来源；再更新测试并清除过时引用。

## 验收与测试

- Profiles：schema 初始化和从已有数据库迁移、名称校验（含大小写冲突和 64/65 字符边界）、稳定排序、CLI 参数错误、创建/编辑/删除、缺失片段和无效 JSON。
- Snapshots：schema 初始化、激活前 snapshot 捕获（含 `source` 标签）、`history list/show/activate/delete/prune`、history 激活不产生新 snapshot，且可覆盖损坏的 settings.json。
- 激活：单片段原子覆盖（跨平台）、目标目录创建、切换前 snapshot 与 journal 写入、崩溃后的 journal 恢复，以及原子替换前发生源或目标写入错误时全局 settings 文件字节不变；Windows 不允许 `remove` 后 `rename` 回退；激活后 settings.json 的 Unix 权限位与 Windows ACL 与原文件一致或更严。
- TUI：`[` / `]` 三标签循环、Projects / Sessions tab 内 `Tab` / `←` / `→` 维持两栏焦点切换、Profiles tab 内 `Tab` / `←` / `→` 不切 tab、Profile 选中切换、Enter 激活、空选择的无副作用行为、active Profile 行渲染标记。
- JSON 校验：拒绝 `null`、数组、标量、空字符串与解析错误；接受任意 JSON 对象（含 `{}`、含 `_comment` / `$schema` 等元字段）。
- 迁移：含数据的旧库迁移后生成 `cowboy-migrated-*.json` dump 文件；空库迁移不生成空 dump；dump 失败中止迁移。
- 回归：项目浏览、项目内新会话、会话恢复、搜索、重命名和删除仍正常。
- 清理：编译与测试中不再存在 `export`、项目配置编辑或运行时 env 覆盖的遗留引用。
