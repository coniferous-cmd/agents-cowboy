## Why

当前 profile 绑定采用 1:1 关系（一个 profile 只能绑定到一个项目）。当用户尝试把已绑定到其他项目的 profile 绑定到新项目时，会收到 "ProfileAlreadyBound" 错误，但错误信息没有告诉用户这个 profile 已经绑定到了哪个项目，导致用户困惑。

改为 1:N 关系（允许多个项目共享同一个 profile）可以：
- 消除这个令人困惑的错误
- 支持"工作 profile"场景：用户可以在多个相关项目间共享相同的配置
- 简化 profile 管理

## What Changes

- **移除 profile_name 的 UNIQUE 约束**：从 `project_profile_bindings` 表中移除 profile_name 的唯一约束，允许同一个 profile 被绑定到多个项目
- **修改 bind_profile 逻辑**：不再需要检查 profile 是否已被其他项目绑定，INSERT 失败时的错误处理需要调整
- **改进错误信息**：当绑定失败时，提供更清晰的错误信息
- **更新测试用例**：添加测试验证多项目绑定同一 profile 的场景

## Capabilities

### New Capabilities

（无新能力，这是对现有能力的行为变更）

### Modified Capabilities

- `project-profile-binding`: 允许多个项目绑定同一个 profile（从 1:1 改为 1:N）

## Impact

### 代码变更

- `src/claude_env/schema.rs`：移除 `profile_name TEXT NOT NULL UNIQUE` 中的 UNIQUE 约束
- `src/claude_env/profiles.rs`：
  - 修改 `bind_profile()` 方法的错误处理逻辑
  - 移除 `ProfileAlreadyBound` 错误（或改为仅在 profile 不存在时使用）
  - 更新 `unbind_profile()` 逻辑（可能需要处理多项目绑定的情况）
- `src/app/mod.rs`：更新错误处理和状态显示逻辑
- 测试文件：更新和新增相关测试

### 数据库迁移

需要执行数据库迁移来移除 UNIQUE 约束。对于已有数据，如果存在 profile 被绑定到多个项目的情况，需要处理数据一致性。

### 用户体验

- 用户可以把同一个 profile 绑定到多个项目
- 删除 profile 时，所有绑定到该 profile 的项目都会被解绑（FK ON DELETE RESTRICT 需要调整）
- 解绑操作可能需要更明确的 UI 提示

### 依赖

无外部依赖变更
