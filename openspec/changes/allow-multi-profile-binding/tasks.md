## 1. 数据库迁移

- [x] 1.1 创建新的 `project_profile_bindings_v2` 表结构（移除 profile_name 的 UNIQUE 约束）
- [x] 1.2 编写数据迁移逻辑：将旧表数据迁移到新表
- [x] 1.3 实现表重命名逻辑：备份旧表，新表替换
- [x] 1.4 添加迁移测试：验证数据迁移的正确性

## 2. 核心逻辑修改

- [x] 2.1 修改 `bind_profile()` 方法：移除 ProfileAlreadyBound 错误处理
- [x] 2.2 更新 `unbind_profile()` 方法：处理多项目绑定的情况
- [x] 2.3 修改 `project_binding()` 方法：确保返回正确的绑定信息
- [x] 2.4 更新 `profile_bindings()` 方法：返回所有绑定到该 profile 的项目

## 3. 错误处理更新

- [x] 3.1 移除 `ProfileAlreadyBound` 错误类型（或保留但不再使用）
- [x] 3.2 更新 `bind_profile()` 的错误返回逻辑
- [x] 3.3 添加新的错误类型：`ProfileInUse`（用于删除绑定 profile 时）

## 4. UI 和应用层更新

- [x] 4.1 更新 `app/mod.rs` 中的绑定错误处理
- [x] 4.2 修改 toast 消息显示逻辑
- [x] 4.3 添加绑定成功后的状态更新

## 5. 测试用例

- [x] 5.1 添加测试：同一 profile 绑定到多个项目
- [x] 5.2 添加测试：不同 profile 绑定到同一项目（upsert 行为）
- [x] 5.3 添加测试：绑定不存在的 profile
- [x] 5.4 添加测试：解绑后重新绑定
- [x] 5.5 添加测试：删除绑定 profile 时的错误处理
- [x] 5.6 运行完整测试套件，确保所有测试通过

## 6. 验证和清理

- [x] 6.1 执行数据库迁移测试
- [x] 6.2 手动测试多项目绑定场景
- [x] 6.3 运行 clippy 检查代码质量
- [ ] 6.4 更新相关文档（如果需要）
