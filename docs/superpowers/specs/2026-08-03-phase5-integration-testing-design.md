# Phase 5: 集成测试与部署验证 - 设计文档

## 文档信息

- **版本**: v1.0
- **创建日期**: 2026-08-03
- **阶段**: Phase 5 - 集成测试与部署验证
- **前置条件**: Phase 0-4 已完成，82 个单元测试全部通过

---

## 1. 背景与目标

### 1.1 当前状态

- Phase 0-4 已完成（TASK-000 ~ TASK-030）
- 82 个单元测试全部通过
- 16 个 nix 0.29 API 兼容性错误已修复
- 3 个预存测试失败已修复（diff 空行、executor 序列化）
- 集成测试文件存在但大部分为空（`integration_test.rs`、`worker_test.rs`、`manager_test.rs`）
- `auth_integration_test.rs` 全为 `#[ignore]` 占位符

### 1.2 Phase 5 目标

根据 ARCHITECTURE.md，Phase 5 需要：
1. 验证所有旧架构功能
2. 建立性能基准线
3. 压力测试
4. 部署配置完善

### 1.3 约束

- **测试环境**: WSL（Windows Subsystem for Linux）
- **权限处理**: 使用 mock 或条件编译避免 root 依赖
- **性能目标**: 建立基准线，不设硬性指标
- **不引入新依赖**: 性能测试使用手动计时框架

---

## 2. 发现的问题

### 2.1 install.sh 与 systemd service 不一致

**问题**: `agent/deploy/install.sh` 内联生成的 systemd service 文件与 `systemd/quireld.service` 严重不一致。

**差异对比**:

| 配置项 | systemd/quireld.service | install.sh 内联生成 |
|--------|-------------------------------------|---------------------|
| Type | notify | simple |
| KillMode | process (Phase 4) | 缺失 |
| ExecReload | /bin/kill -HUP $MAINPID (Phase 4) | 缺失 |
| CapabilityBoundingSet | 有 | 缺失 |
| LimitNOFILE | 65536 | 缺失 |
| Wants | network-online.target | 缺失 |

**影响**: 通过 install.sh 安装的服务缺少 Phase 4 热更新功能。

### 2.2 集成测试缺失

现有集成测试只覆盖组件级测试（启动/停止/清理），缺少：
- Manager↔Worker 真实 IPC 通信测试
- PTY 会话创建全流程测试
- 文件操作/命令执行业务逻辑测试
- 性能基准数据
- 压力测试场景

---

## 3. 设计方案

### 3.1 总体架构

```
Phase 5 任务分解
├── 集成测试 (TASK-031 ~ TASK-034)
│   ├── Manager↔Worker IPC 全链路
│   ├── PTY 会话全流程
│   ├── 文件操作业务
│   └── 命令执行与系统信息
├── 性能基准 (TASK-035)
│   └── 手动计时框架 + 基准建立
├── 压力测试 (TASK-036 ~ TASK-037)
│   ├── 并发压力
│   └── 长时间运行与资源泄漏
├── 部署配置 (TASK-038 ~ TASK-039)
│   ├── 修复 install.sh 一致性
│   └── 部署验证测试
└── 收尾 (TASK-040)
    └── 完整验证与文档更新
```

---

## 4. 任务详细设计

### TASK-031: Manager↔Worker IPC 全链路测试

**目标**: 验证 Manager 与 Worker 之间的真实 IPC 通信

**测试场景**:
1. 启动真实 Worker 子进程（`agent --worker --ipc-socket <path>`）
2. Manager 接受 Worker 连接
3. Manager 发送 ManagerRequest，Worker 返回 WorkerResponse
4. 验证 SCM_RIGHTS FD 传递（Worker 发送 PTY master_fd）

**文件**: `agent/tests/integration_test.rs`

**实现要点**:
- 使用 `std::process::Command` 启动 Worker 子进程
- 使用临时 Unix Socket 路径（tempfile）
- 超时处理（避免死锁）
- 子进程清理（Drop trait 或显式 kill）

**验证标准**:
- Worker 能连接到 Manager
- 消息往返正确（request_id 匹配）
- FD 传递成功（可读取/写入）

---

### TASK-032: PTY 会话全流程测试

**目标**: 验证 PTY 会话从创建到销毁的完整流程

**测试场景**:
1. 通过 Worker 创建 PTY 会话（bash）
2. 验证 master_fd 注册到 PtyRegistry
3. 向 PTY 写入命令（`echo hello\n`）
4. 读取 PTY 输出，验证包含 "hello"
5. 销毁会话，验证资源清理（FD 关闭、注册表注销）

**文件**: `agent/tests/worker_test.rs`

**实现要点**:
- 使用 `nix::unistd::read`/`write` 直接操作 master_fd
- 读取超时处理（PTY 输出可能延迟）
- 验证 OrphanProcessReaper 回收

**验证标准**:
- PTY 会话创建成功
- 命令执行输出正确
- 会话销毁后无资源泄漏

---

### TASK-033: 文件操作业务测试

**目标**: 验证文件操作的业务逻辑

**测试场景**:
1. ReadDir: 列出目录，验证 DirEntry 字段
2. ReadFile: 读取文件内容，验证 offset/length
3. WriteFile: 写入文件，验证 bytes_written
4. 用户目录限制: 验证不能访问 home_dir 之外的路径
5. mtime 版本验证

**文件**: `agent/tests/worker_test.rs`

**实现要点**:
- 使用 tempfile 创建临时测试目录
- 创建测试文件和目录结构
- 验证错误处理（权限拒绝、文件不存在）

**验证标准**:
- 文件操作正确执行
- 用户目录限制生效
- 错误场景正确处理

---

### TASK-034: 命令执行与系统信息测试

**目标**: 验证命令执行和系统信息查询

**测试场景**:
1. ExecuteCommand: 执行 `echo hello`，验证 stdout
2. 执行带 stderr 的命令，验证 stderr
3. 验证 exit_code
4. 超时测试: 执行 `sleep 10`，设置 1 秒超时
5. GetSystemInfo: 验证 CPU/内存/磁盘字段

**文件**: `agent/tests/worker_test.rs`

**验证标准**:
- 命令输出正确
- 超时处理生效
- 系统信息字段完整

---

### TASK-035: 性能基准建立

**目标**: 建立当前性能基准数据

**测试指标**:
1. PTY 吞吐量: 写入/读取大块数据（1MB）的速率
2. IPC 往返延迟: Manager→Worker→Manager 单次往返时间
3. 并发会话基准: 创建 10/50/100 个 PTY 会话的时间

**文件**: `agent/tests/bench_test.rs`

**实现要点**:
- 手动计时框架（`std::time::Instant`）
- 多次运行取平均/中位数
- 输出基准报告（JSON 或文本）
- 标记为 `#[ignore]`（手动运行，避免 CI 拖慢）

**验证标准**:
- 基准测试可重复运行
- 输出包含平均值、中位数、p99
- 记录到性能基准文档

---

### TASK-036: 并发压力测试

**目标**: 验证高并发场景下的稳定性

**测试场景**:
1. 50+ 并发 PTY 会话创建
2. 并发文件操作（多线程同时读写）
3. 会话快速创建/销毁循环（100 次）
4. 验证无死锁、无 panic

**文件**: `agent/tests/stress_test.rs`

**验证标准**:
- 50 并发会话创建成功
- 无死锁或 panic
- 资源使用合理

---

### TASK-037: 长时间运行与资源泄漏检测

**目标**: 验证长时间运行的稳定性

**测试场景**:
1. 5 分钟持续运行（创建/销毁会话循环）
2. 内存增长检测（读取 `/proc/self/status` VmRSS）
3. FD 泄漏检测（读取 `/proc/self/fd` 目录）
4. Worker 崩溃恢复压力测试

**文件**: `agent/tests/stress_test.rs`

**验证标准**:
- 5 分钟运行无崩溃
- 内存增长 < 50%
- FD 数量稳定（无泄漏）
- Worker 崩溃后自动恢复

---

### TASK-038: 修复 install.sh 与 systemd service 一致性

**目标**: 确保 install.sh 安装的 service 文件包含 Phase 4 功能

**修改方案**:
- install.sh 不再内联生成 service 文件
- 改为复制项目根目录的 `systemd/quireld.service`
- 添加服务名称变量替换（如果 `--name` 指定了自定义名称）

**文件**: `agent/deploy/install.sh`

**验证标准**:
- 安装后的 service 包含 `KillMode=process`
- 安装后的 service 包含 `ExecReload`
- `systemctl reload` 可用

---

### TASK-039: 部署验证测试

**目标**: 验证部署配置的完整性

**测试场景**:
1. 验证 systemd service 文件语法（`systemd-analyze verify`）
2. 验证 install.sh 的 dry-run 模式
3. 验证配置文件路径和权限

**文件**: `agent/tests/deploy_test.rs`

**验证标准**:
- service 文件语法正确
- install.sh 逻辑正确

---

### TASK-040: Phase 5 完整验证与文档更新

**目标**: 确保所有测试通过，更新文档

**执行步骤**:
1. 运行全部测试套件（`cargo test`）
2. 运行集成测试（`cargo test --test *`）
3. 运行性能基准（`cargo test --test bench_test -- --ignored`）
4. 运行压力测试（`cargo test --test stress_test -- --ignored`）
5. 更新 TASK_BREAKDOWN.md
6. 生成性能基准报告

**验证标准**:
- 所有非 ignored 测试通过
- 性能基准报告生成
- TASK_BREAKDOWN.md 更新完成

---

## 5. 测试文件组织

```
agent/tests/
├── integration_test.rs       # TASK-031: Manager↔Worker IPC 全链路
├── worker_test.rs            # TASK-032~034: PTY/文件/命令业务测试
├── manager_test.rs           # Manager 级别集成测试
├── ipc_integration_test.rs   # 现有: IpcServer 组件测试
├── hot_update_test.rs        # 现有: Phase 4 热更新测试
├── auth_integration_test.rs  # 现有: auth 集成测试（ignored）
├── auth_test.rs              # 现有: auth 单元测试
├── bench_test.rs             # TASK-035: 性能基准（新增）
├── stress_test.rs            # TASK-036~037: 压力测试（新增）
└── deploy_test.rs            # TASK-039: 部署验证（新增）
```

---

## 6. 风险与缓解

### 6.1 WSL 环境限制

**风险**: WSL 可能缺少某些 Linux 功能（如完整的 systemd）

**缓解**:
- 测试不依赖 systemd 运行，只验证 service 文件语法
- PTY/IPC 测试使用直接系统调用，不依赖 systemd

### 6.2 权限问题

**风险**: WSL 默认用户非 root，某些操作需要 root

**缓解**:
- PTY 创建和 IPC 通信不需要 root
- 文件操作测试在用户目录内进行
- 需要 root 的测试标记 `#[ignore]`

### 6.3 测试超时

**风险**: 集成测试可能因 IPC 死锁而挂起

**缓解**:
- 所有测试使用 `tokio::time::timeout` 包装
- Worker 子进程设置 watchdog 超时

---

## 7. 成功标准

- [ ] 10+ 集成测试通过
- [ ] 性能基准数据建立
- [ ] 压力测试通过（50 并发、5 分钟运行）
- [ ] install.sh 与 systemd service 一致
- [ ] TASK_BREAKDOWN.md 更新完成
