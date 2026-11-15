---
name: quirel-protocol-change
description: Quirel 线上协议变更规范。修改 quirel-protocol/src 下任何类型、Envelope、serde 字段、新增消息类型，或 agent/src/protocol 与客户端消费点需要同步时，必须先加载本技能。
---

# 协议变更 checklist（AGENTS.md §5 红线表第 4 行）

## 破坏性判定（不确定时按破坏性处理并报告用户）
破坏性 = 必须客户端与 quireld 同发，0.x 阶段至少升次版本号（AGENTS.md §4）：
- 删除/重命名枚举 variant、结构体字段；字段类型变更；序列化格式（tag/表示）变更；
  收紧校验使旧消息被拒。
兼容性变更（仍需评估）：
- 新增 `#[serde(default)]` 可选字段（前提：旧端反序列化忽略未知字段）
- 新增消息类型（前提：旧端按 unknown 处理）

## 改动步骤
1. TDD：先在 `quirel-protocol/tests/wire_compat.rs` 加/改 wire 兼容用例（先红后绿）。
2. 改 `quirel-protocol/src`，保持双端语义一致。
3. 检查 agent 侧（agent/src/protocol/）与客户端侧（src-tauri）消费点是否同步。
4. `npm run verify:rust` 全绿；同步更新 `docs/IPC_PROTOCOL.md`。
5. 判定为破坏性时：在完成摘要中明确声明，等待用户安排双端同发版本。
