## Context

`src/claude_env/profiles.rs` 已经把"DB 是 source of truth,文件是 mirror"做成了一套事务:`update_profile_json` 在一笔 SQLite 事务里把 `claude_profiles.settings_json` 改了,然后立刻 `AtomicReplace::write` 把同一个 JSON 写到 `profile_file_path(name)`(第 155–174 行)。`copy_profile`、`perform_activation` 走完全一样的模式。

把"DB 是 source of truth"翻过来用一次的需求现在落不到任何路径上。`edit` 命令的 `$EDITOR` 流程也走 `update_profile_json`,结果是 DB 与磁盘一致——但**用户用 `vim settings.work.json` 直接改文件**这条腿就丢在外面。

下面这一节集中描述 sync 的算法、解析器影响、错误处理边界、测试策略。

## Goals / Non-Goals

**Goals**

- 让 DB 与磁盘重新一致,只需一个命令
- 单次调用可以把整个 profiles/ 目录 sweep 一遍,不必每个名字跑一次
- 全程复用现有 `validate_profile_name` / `validate_settings_json` / `AtomicReplace` 的不变量,不开新写入路径
- 与 TUI 无关:不动 AppState、不动 layout、不动 keymap
- 单文件失败不污染其他文件

**Non-Goals**

- 不实现"双向 reconcile"(DB↔磁盘谁更新走 mtime/hash 比对)——这是另一种语义,本次不做
- 不实现"按 modified-time 排序的批量重放"--本次只按文件→DB 单一方向
- 不实现 dry-run 模式——本期可观测性通过 stdout 的逐行报告已经够用;若以后要 `--dry-run` 再加
- 不实现对 `~/.claude/settings.json` 符号链接的修复——activation 早就管这事了
- 不实现 schema migration——`claude_profiles` 表已经够用,新功能只是行级 INSERT/UPDATE
- 不实现 TUI 入口——CLI 加 `sync` 子命令即可

## Decisions

### CLI 解析:`sync` 作为新动词加入现有 dispatch

`parse_config_args` 现在的形态是一个对 `command` 字符串的 match 循环,把第一个 token 当动词(`config.rs:21-65`),然后是 `list` / `create` / `edit` / `delete` / `activate` / `bind` / `unbind` / `copy` 八个分支。`sync` 加入的方式跟 `bind` 同形:消耗一个 token,**可选**作为 `<name>`,然后 `reject_extra` 拦截多余 token。

```rust
"sync" => {
    let name = args.next();
    reject_extra(&mut args, "config sync [name]")?;
    ConfigCommand::Sync { name }
}
```

`name: Option<String>` 直接落到 `SyncReport` 的 reconcile-some 与 reconcile-all 两条分支。

**为什么不做成 `--sync` flag**:用户的需求是"增加一个 --sync option"。如果把 `--sync` 当作在动词槽位之前抢占的全局 flag(`cowboy config --sync work`),需要在 `parse_config_args` 顶部加一遍 `peek + dispatch` 特判,破坏现有 8 个 verb 的对称结构。若跟着动词走(`cowboy config create --sync foo`)则一个 flag 多个语义,违反"一个 flag 一个语义"。

子命令形态 (`cowboy config sync [name]`)与现有 list/create/edit 完全对称,代码改动控制在 dispatcher 一个 match 臂 + handler 一个 match 臂 + help/README 几行。

**替代方案**:

- **`cowboy config --sync`(动词前 flag)** —— 抢占式分发,parser 顶部加特判。代码改动更大、`help` 输出不对称、与现有形状偏离。放弃。
- **`cowboy --sync`(顶层 flag)** —— 把 sync 提到 cowboy 主命令。sync 在语义上属于 config 子命令,放顶层就要在 main.rs 专门分发,反而让分发逻辑分裂。放弃。

### Sync 语义核心:磁盘是 source of truth,DB 是 mirror

**关键不变量**:**sync 路径不写文件**。磁盘上的 `settings.<name>.json` 是用户(或外部工具)写出来的真值,DB 行只是这同一份 JSON 的另一份存储,只要把 DB 拉齐到磁盘。

**为什么**:`update_profile_json` 已经把"DB + 文件同一事务"原子化。sync 走的是这套的另一面——文件已经在那了,DB 还没跟上。写文件会引入多一次磁盘写,而且**会改文件的 mtime**,对可能在用 `git`/`backup tooling`/`make` 之类工具扫这个目录的用户是意外操作。sync 只读磁盘、不写磁盘,语义干净。

### 写入路径:复用现有基础设施

每个 profile 的 reconcile 走以下分支(`profiles.rs` 新增 `sync_profiles_from_disk`):

```
1.  fs::read_to_string(profile_file_path(&name))
2.  validate_settings_json(&raw)        ← 复用 profiles.rs:53
3.  validate_profile_name(&name)        ← 复用 profiles.rs:36
4.  transaction:
       if row exists:
           if row.settings_json == raw: 报告 unchanged
           else: UPDATE settings_json=raw  报告 updated
       else:
           INSERT (name, raw)             报告 inserted
```

INSERT 失败(`ProfileExists` 约束冲突)落回 UPDATE——极端 race:用户在 sync 跑时手动 `create_profile` 同名。其实不可能并发(CLI 是单进程),但用 `INSERT ... ON CONFLICT DO UPDATE` 一条 SQL 等价表达更省事:

```sql
INSERT INTO claude_profiles (name, settings_json) VALUES (?1, ?2)
ON CONFLICT(name) DO UPDATE SET settings_json = excluded.settings_json,
                              updated_at = CURRENT_TIMESTAMP
```

这一条 SQL 自带"insert-or-update"语义,不需要 transaction,也不需要专门跑 `SELECT` 判断存在性。`updated_at` 字段若真的存在差异(配对时基于对比结果区分 `inserted` 与 `updated`),**sync 路径需要在 `SyncReport` 里区分两者**,所以保留两步法:

```
1. self.profile(name) -> Result<Option<ClaudeProfile>>
2. match profile:
       None              -> INSERT
       Some(p) if same   -> report unchanged
       Some(p) if diff   -> UPDATE
```

两步法的好处:`SyncReport.inserted` 与 `updated` 是两个不同的 list,符合 spec 的 scenario"区分至少 inserted / updated / unchanged / invalid"。

**JSON 校验失败的 profile**:跳到 `SyncReport.invalid`,不抛错。继续下一个。这种语义让 `cowboy config sync` 在"半坏状态"下也输出一个完整的报告,而不是早返回。

**文件缺失的 profile**(磁盘没有,DB 有):sync **不删** DB 行。Spec 明确规定了"No reason to delete"。原因:删除是数据丢失,sync 的语义是"让 DB 跟上磁盘",不是"让磁盘决定 DB 生死"。用户真要删除,用 `cowboy config delete <name>`。

### `SyncReport` 数据结构

```rust
pub struct SyncReport {
    pub inserted: Vec<String>,       // INSERT 成功的 name
    pub updated: Vec<String>,        // UPDATE 成功的 name
    pub unchanged: Vec<String>,      // 内容已一致的 name
    pub invalid: Vec<InvalidEntry>,  // JSON 解析失败的 name + error
}

pub struct InvalidEntry {
    pub name: String,
    pub error: String,
}
```

handler 输出格式(逐行可解析):
```
synced work: updated
synced home: unchanged
synced newproj: inserted
synced broken: invalid (trailing comma at line 3)
```

失败计数 > 0 时 exit code 仍为 0(spec scenario 已规定"操作完成即退出 0")。这是 cowboy 现有 handler 的行为模式——所有 `handle_*` 都是正常路径返回 `Ok`,错误走 `Result<_, Error>` 上抛。

### `parse_config_args` 解析状态机细节

`parse_config_args` 的现有形状(`config.rs:12-68`)就是一个对第一个 token 做 match 的循环,后续 verb 各自按位置参数个数决定 args.next 的调用次数。新分支与 `bind` 同形:

```rust
"sync" => {
    let name = args.next();
    reject_extra(&mut args, "config sync [name]")?;
    ConfigCommand::Sync { name }
}
```

不需要新字段、不动 `cmd/mod.rs`,一次 match 臂 + 一次 helper(已存在 `reject_extra`)就够。handler 配对加一条 match 臂即可。所有改动局限于 `src/cmd/config.rs` 一个文件。

### 测试策略(TDD:负向断言先,然后正向)

按 CLAUDE.md 的 Red→Green→Refactor 流程:

**1. 负向断言测试(先加,实现仍未到位时应当失败)**

- `parse(&["sync"])` → `Sync { name: None }`
- `parse(&["sync", "work"])` → `Sync { name: Some("work") }`
- `parse(&["sync", "work", "extra"])` → error
- `parse(&["sync", "create", "work"])` → error(多余的第二个 token)
- `SyncReport` 数据结构构造与字段检查

**2. 存储层负向测试**(handler 没接上时,这些应当 panic 或 not run)

- `sync_profiles_from_disk` 在 profiles_dir 不存在时返回空 report 不报错
- `sync_profiles_from_disk` 跳过 `notes.txt` 这种无关文件

**3. 正向行为测试**

- `sync_inserts_profile_from_disk_when_no_db_row`
- `sync_updates_db_row_when_disk_differs`
- `sync_no_ops_when_db_and_disk_match`
- `sync_skips_invalid_json_and_returns_entry_in_report`
- `sync_leaves_db_row_when_disk_file_missing`(新加关键的 not-delete case)
- `sync_walks_all_files_in_profiles_dir_when_called_with_none`
- `sync_only_targets_given_name_when_some`
- `sync_preserves_project_bindings`(modifies bindings 表格不直接,但 sync 跑后绑定还在)

**4. handler 测试**

- `sync_handler_inserts_new_profile_from_disk_file`
- `sync_handler_updates_drifted_row`
- `sync_handler_reports_invalid_json_without_aborting`
- `sync_handler_leaves_db_row_when_file_missing`
- `sync_handler_writes_human_readable_summary_to_stdout`

**5. 验证**

- `cargo fmt --check`
- `cargo test`(149 lib + 100 binary 不退化)
- `cargo clippy --all-targets --all-features -- -D warnings`
- `speckit validate --change add-config-sync`(应只剩 `< 1000 char` 之类的弱 warning)

### Risk / Trade-offs

#### 风险 1:磁盘内容陈旧覆盖 DB 中更新的内容

**描述**:`sync work` 会用磁盘上的 `settings.work.json` 覆盖 DB。如果用户上次用 `cowboy config edit work` 改了 DB(`updated_at` 是 T1),随后又在外部编辑了磁盘文件(T2 < T1),sync 跑后 DB 是 T2 的旧版本。

**为什么接受**:用户主动跑 sync 就是声明"以磁盘为准"。这是 sync 的语义。CLI 输出列了每个 reconcile 的结果(`synced work: updated`),足够人眼审计。CLI 不应替用户决定胜负。

**缓解**:`SyncReport` 暴露每个被覆盖的 name,用户 commit 前 `git diff` 即可。

**替代方案**:

- mtime 比对——实现复杂且跨时区、跨文件系统不稳定,放弃
- hash 比对——只比较"是否相同"已经够,不真的判定谁更新

#### 风险 2:`profiles_dir()` 还没被任何 profile 写入过,不存在的目录

**现状**:`profiles_dir()`(第 415 行)会 `ensure_private_dir(&dir)`,这是 idempotent 的——不存在就创建。如果磁盘上根本没有 `profiles/` 目录,说明用户从未 `update_profile_json` 过任何 profile(`create_profile` 不写文件,`copy_profile` 和 `perform_activation` 写)。

**sync 跑时的行为**:
- `name=Some(x)`:读 `profiles_dir()/settings.x.json` → `NotFound`,跳过(SPEC 要求不删 DB 行)
- `name=None`:目录存在但无 `settings.*.json` 文件 → 空 report 正常返回

不需要额外处理。`profiles_dir()` 已经负责建目录。

#### 风险 3:`settings.<name>.json` 存在但不是合法 UTF-8

**现状**:`fs::read_to_string` 失败抛 `InvalidData`。`validate_settings_json` 接收的是 `&str`。

**处理**:把 `read_to_string` 错误转化为 `SyncReport.invalid` 一条,继续下一个。这样二进制误入目录的 profile 不会让 sync 中止。

#### 风险 4:JSON 中有 `<name>` 用大写(违反 `validate_profile_name` 规范)

**现状**:文件命名由外部工具决定。`ProfileExists` 约束在 SQLite 层用大写,所以 `Work.json` 对应 DB 里的 `work` 还是 `work`?

实际上现在 DB 列是大写归一化的(`validate_profile_name` 把 `Work` 转 `work`)。sync 路径**必须**走 `validate_profile_name(name)`,若不合法(例如 `Work With Spaces.json`)则在 `SyncReport.invalid` 报告"name is not a valid profile name",不报错到 stderr。

#### 风险 5:大量 profile 的性能

`profiles_dir()` 列出文件 → `O(N)`。每个 profile 一次 `fs::read + JSON parse + single-row SQL`:100 个 profile 量级 1 秒内能跑完。仅 CLI 路径,不进 TUI 热路径,无需性能优化。

#### 风险 6:sync 跑时用户同时在另一个终端改 `update_profile_json`

SQLite 是本地锁。`ClaudeEnvStore` 现在没有显式 long-running transaction,sync 路径不持有任何锁;另一个 `update_profile_json` 会强制自己的事务 commit 或 rollback。**结果不可预测**:sync 可能读到串行交叉的中间态。

**缓解**:这是个 CLI 工具,CLI 调用通常是人手动作业,不同 terminal 并发不常见。接受。如果以后需要严格并发安全,可在 sync 入口取 `activation_lock`(现有 helper),加上 `BEGIN IMMEDIATE`;但本期不做。

#### 权衡:简单 sync vs 完整双向 sync

完整双向 reconcile 要解决"DB ↔ 磁盘谁新"——典型方案是基于 mtime 但不可靠,基于 hash 等价于"内容相同就跳过"已经够用。本次只做单向 sync,理由:

- 用户需求是"让 DB 跟上磁盘",不是"让两边各自动一致"
- 双向会引入 sync 失败时"两边都改动怎么办"的策略分歧(丢弃一个、合并、报错...)。每一种策略都要新 CLI flag,UX 复杂度上涨
- 双向的 80% 用例就是单方向,剩下的 20% 用例没人需要

代码量减少大约 30 行;sync 路径的算法语义保持单一;测试覆盖更紧凑。
