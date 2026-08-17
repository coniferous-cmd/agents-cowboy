## Context

当前系统使用 SQLite 数据库存储 profile 绑定关系，`project_profile_bindings` 表的 `profile_name` 字段有 UNIQUE 约束，导致一个 profile 只能绑定到一个项目。

数据库 schema 位置：`src/claude_env/schema.rs`
绑定逻辑位置：`src/claude_env/profiles.rs` 中的 `bind_profile()` 方法

## Goals / Non-Goals

**Goals:**
- 移除 `profile_name` 的 UNIQUE 约束，支持 1:N 关系
- 保持数据完整性，确保现有功能不受影响
- 更新错误处理逻辑，提供清晰的错误信息

**Non-Goals:**
- 不改变 UI 显示逻辑（项目仍然显示最新的绑定 profile）
- 不改变激活 profile 的全局行为
- 不添加 profile 分组或分层功能

## Decisions

### 1. 数据库迁移策略

**决策**：创建新的表结构，迁移数据，然后重命名。

**理由**：
- SQLite 不支持直接修改 UNIQUE 约束
- 创建新表可以确保数据完整性
- 迁移过程可以在事务中完成，失败时可以回滚

**备选方案**：
- 删除表并重建：风险更高，需要备份数据
- 添加新列：不适用，因为需要移除约束

### 2. bind_profile 逻辑调整

**决策**：修改 INSERT 语句，移除 ON CONFLICT 的特殊处理。

**当前逻辑**：
```sql
INSERT INTO ... ON CONFLICT(project_cwd) DO UPDATE SET profile_name=excluded.profile_name
```

**新逻辑**：
```sql
INSERT INTO ... ON CONFLICT(project_cwd) DO UPDATE SET profile_name=excluded.profile_name
```

**理由**：
- 保持 upsert 语义：同一个项目的绑定可以更新
- 移除 profile_name 的唯一约束后，INSERT 不会因为 profile_name 重复而失败

### 3. 错误处理调整

**决策**：移除 `ProfileAlreadyBound` 错误类型，因为不再适用。

**理由**：
- 1:N 关系下，profile 可以被多个项目绑定
- 唯一可能的错误是 profile 不存在（已有 `ProfileNotFound`）
- 简化错误处理逻辑

### 4. 外键约束处理

**决策**：保持 `ON DELETE RESTRICT` 约束。

**理由**：
- 防止误删正在使用的 profile
- 用户需要先解绑所有项目，才能删除 profile
- 这是安全的设计，避免意外数据丢失

## Risks / Trade-offs

### Risk 1: 数据迁移失败
- **风险**：迁移过程中如果发生错误，可能导致数据不一致
- **缓解**：使用事务包装迁移过程，失败时回滚到原始状态

### Risk 2: 现有测试覆盖不足
- **风险**：修改约束后，某些边界情况可能未被测试覆盖
- **缓解**：添加新的测试用例，验证多项目绑定同一 profile 的场景

### Risk 3: UI 显示混淆
- **风险**：用户可能困惑为什么同一个 profile 显示在多个项目中
- **缓解**：在 UI 中添加说明，或在 profile 详情中显示绑定的项目列表

## Migration Plan

### 阶段 1: 准备
1. 备份现有数据库
2. 验证现有数据的完整性

### 阶段 2: 迁移
1. 创建新的 `project_profile_bindings_v2` 表（无 UNIQUE 约束）
2. 迁移数据到新表
3. 重命名表：旧表备份，新表替换

### 阶段 3: 验证
1. 运行现有测试，确保功能正常
2. 添加新的测试用例
3. 手动测试多项目绑定场景

### 回滚策略
- 如果迁移失败，从备份恢复数据库
- 代码变更可以通过 git revert 回滚

## Open Questions

（无）
