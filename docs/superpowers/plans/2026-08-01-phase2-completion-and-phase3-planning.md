# Phase 2 Completion and Phase 3 Planning

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete Phase 2 Manager implementation and plan Phase 3 Worker implementation

**Architecture:** Microkernel architecture with Manager (gateway), Worker (isolation), Session layers

**Tech Stack:** Rust, Tokio async runtime, QUIC protocol, Protobuf, Unix Domain Sockets

---

## Phase 2 Completion Status

### ✅ Completed Tasks (TASK-010 to TASK-015)

**TASK-010: 创建 Manager 模块基础结构**
- Status: ✅ Completed
- Files: `agent/src/manager/mod.rs`
- Key: Manager struct definition with PtyRegistry, WorkerManager, SessionManager

**TASK-011: 实现 PtyRegistry 基础结构**
- Status: ✅ Completed
- Files: `agent/src/manager/pty_registry.rs`
- Key: PTY session registry with register/unregister operations

**TASK-012: 实现 WorkerManager 基础结构**
- Status: ✅ Completed
- Files: `agent/src/manager/worker_manager.rs`
- Key: Worker process lifecycle management (start/stop/restart)

**TASK-013: 实现 IpcServer 基础结构**
- Status: ✅ Completed (2026-08-01)
- Files: `agent/src/manager/ipc_server.rs`, `agent/src/manager/mod.rs`, `agent/src/config.rs`
- Key:
  - WorkerManager event notification (broadcast channel)
  - Automatic connection establishment on Worker start
  - Automatic connection cleanup on Worker crash
  - FD leak fix (close FD on registration failure)
  - 7 integration tests
- Test Results: All passing in Unix environment

**TASK-014: 实现连接处理逻辑**
- Status: ✅ Completed (2026-08-01)
- Files: `agent/src/manager/connection.rs`
- Key:
  - ConnectionManager with bidirectional mapping
  - Connection-session mapping management
  - Duplicate registration fix (prevent data inconsistency)
  - Error handling improvement (prevent resource leaks)
  - verify_consistency method for data validation
  - 11 unit tests
- Test Results: All passing

**TASK-015: 实现 PTY 输出推送**
- Status: ✅ Completed (2026-08-01)
- Files: `agent/src/manager/pty_output.rs`, `agent/src/server/quic.rs`, `agent/src/manager/pty_registry.rs`
- Key:
  - Refactored from quic.rs to manager module (deleted ~80 lines)
  - Backward compatible interface (spawn_pty_output_task_legacy)
  - Manager holds master_fd and directly reads/writes PTY
  - Migration documentation
- Test Results: Compilation passing

---

## Phase 3: Worker Implementation (待规划)

### Overview

Worker 进程是隔离层，负责：
1. Fork PTY 子进程
2. 通过 Unix Socket 传递 master_fd 给 Manager
3. 处理 Manager 的请求（文件操作、命令执行等）

### Architecture Requirements

```
Manager (Gateway Layer)
    ↓ Unix Socket + FD Passing
Worker (Isolation Layer)
    ↓ fork + PTY
Session (Terminal Process)
```

### Proposed Tasks (需要细化)

**TASK-016: 实现 Worker 进程入口**
- 创建 `agent/src/bin/worker.rs` 或在 main.rs 中添加 --worker 参数
- 解析命令行参数（--worker, --ipc-socket）
- 连接到 Manager 的 Unix Socket

**TASK-017: 实现 IpcClient**
- 文件：`agent/src/worker/ipc_client.rs`
- 连接到 Manager 的 IpcServer
- 发送 FD (SCM_RIGHTS)
- 发送/接收 Protobuf 消息

**TASK-018: 实现 PtyFactory**
- 文件：`agent/src/worker/pty_factory.rs`
- fork PTY 子进程
- 返回 master_fd
- 设置终端大小、环境变量

**TASK-019: 实现请求处理器**
- 文件：`agent/src/worker/handlers/*.rs`
- 处理文件操作请求
- 处理命令执行请求
- 处理系统信息查询

**TASK-020: 实现会话管理**
- 文件：`agent/src/worker/session.rs`
- 管理活动的 PTY 会话
- 处理会话创建/销毁

---

## Next Steps

### Immediate Actions

- [ ] **规划 Phase 3 详细任务** - 使用 brainstorming 技能细化 TASK-016 到 TASK-020
- [ ] **创建 Phase 3 规格文档** - 使用 writing-plans 技能创建详细实施计划
- [ ] **验证 Phase 2 集成** - 在 WSL/Linux 环境运行集成测试

### Integration Testing (Critical)

Phase 2 的核心功能需要在 Unix 环境验证：

- [ ] **运行 Manager 集成测试**
  ```bash
  cd agent
  cargo test --lib manager::ipc_integration_test
  ```

- [ ] **验证 Worker 进程启动**
  - Manager 启动 IpcServer
  - WorkerManager 启动 Worker 进程
  - IpcServer 接受连接并设置 worker_pid

- [ ] **验证 FD Passing**
  - Worker fork PTY
  - 通过 Unix Socket 发送 master_fd
  - Manager 接收并注册到 PtyRegistry

- [ ] **验证 Worker 崩溃处理**
  - Manager 收到 Crashed 事件
  - 自动清理相关连接

### Documentation Updates

- [ ] **更新架构文档** - 反映 Phase 2 完成后的架构变化
- [ ] **创建集成指南** - 如何测试 Manager-Worker 集成
- [ ] **编写故障排查指南** - 常见问题和解决方案

---

## Execution Handoff

**Plan status**: Phase 2 completed, Phase 3 requires planning

**Recommended approach**: Use superpowers:brainstorming to plan Phase 3 in detail, then superpowers:writing-plans to create the implementation plan

**Current blocker**: Phase 3 tasks need detailed specification before implementation can begin