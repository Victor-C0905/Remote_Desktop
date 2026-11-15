---
name: quirel-verify
description: Quirel 任务完成验证与证据模板。在任何任务完成声明、请求合并、提交检查点之前必须加载本技能；修 bug、改前端、改 Rust、触碰红线后的必跑命令组合在此选择。
---

# 完成验证（AGENTS.md §6）

## 按改动面选命令
| 改动面 | 必跑 |
|---|---|
| 前端 src/ | `npm run verify:fe` |
| Rust 任一 crate | `npm run verify:rust` |
| 触碰红线 | 红线表对应回归（WindowManager.session.test.ts / dupcap 探针 / wire_compat.rs / check:sensitive-logs） |
| 完成声明 / 请求合并前 | `npm run verify:full` |

## bug 修复黄金流程（强制）
1. 先写复现测试（此刻必须 FAIL——红）。
2. 根因修复（配合 systematic-debugging 技能：先定位根因，禁止症状压制）。
3. 复现测试转 PASS（绿）+ `npm run verify:full` 全绿。
4. 复现测试永久入库；禁止删除或 ignore 它。

## 完成摘要四件套（模板）
```
变更摘要：<改了什么 / 为什么>
影响面：<模块 / 接口 / 受影响的现有测试（列出测试文件名）>
测试证据：<命令 + 结果（修 bug 时含复现测试红→绿记录）>
建议 squash 提交信息：<type(scope): 中文简述>
```

## 定位相关现有测试
按改动文件就近找：src/ 同目录 `*.test.ts(x)`；src-tauri / agent 文件内 `#[cfg(test)]`；
协议改动看 `quirel-protocol/tests/wire_compat.rs`。
