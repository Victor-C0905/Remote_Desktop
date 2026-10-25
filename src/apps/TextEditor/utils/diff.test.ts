/**
 * diff 端到端一致性测试：calculateDiff（客户端生成）→ applyDiff（客户端镜像）
 * 覆盖真实事故场景——全选替换（曾致 agent panic / response dropped）
 */
import { describe, it, expect } from "vitest";
import { calculateDiff, applyDiff } from "./diff";

function roundtrip(oldText: string, newText: string) {
  const diffs = calculateDiff(oldText, newText);
  return applyDiff(oldText, diffs);
}

describe("diff 端到端（生成→应用 回环一致）", () => {
  it("单行修改", () => {
    expect(roundtrip("line1\nline2\nline3", "line1\nmodified\nline3")).toBe("line1\nmodified\nline3");
  });

  it("末尾追加多行", () => {
    expect(roundtrip("a\nb", "a\nb\nc\nd\ne")).toBe("a\nb\nc\nd\ne");
  });

  it("开头插入多行", () => {
    expect(roundtrip("a\nb", "x\ny\na\nb")).toBe("x\ny\na\nb");
  });

  it("删除中段", () => {
    expect(roundtrip("a\nb\nc\nd", "a\nd")).toBe("a\nd");
  });

  it("删插交错（中段整块替换）", () => {
    const old = "head\n" + Array.from({ length: 10 }, (_, i) => `old${i}`).join("\n") + "\ntail";
    const new_ = "head\n" + Array.from({ length: 50 }, (_, i) => `new${i}`).join("\n") + "\ntail";
    expect(roundtrip(old, new_)).toBe(new_);
  });

  it("★ 全选替换：30 行 → 80 行（事故复现场景，曾致 agent panic）", () => {
    const old = Array.from({ length: 30 }, (_, i) => `旧内容第${i}行`).join("\n");
    const new_ = Array.from({ length: 80 }, (_, i) => `新内容第${i}行`).join("\n");
    const diffs = calculateDiff(old, new_);
    // 行号不应超过旧文件坐标范围 +1（曾暴涨到新文件坐标 ~80）
    for (const d of diffs) {
      expect(d.line).toBeLessThanOrEqual(old.split("\n").length + 1);
    }
    expect(applyDiff(old, diffs)).toBe(new_);
  });

  it("全选替换：80 行 → 5 行（反向收缩）", () => {
    const old = Array.from({ length: 80 }, (_, i) => `x${i}`).join("\n");
    const new_ = Array.from({ length: 5 }, (_, i) => `y${i}`).join("\n");
    expect(roundtrip(old, new_)).toBe(new_);
  });

  it("同点连续插入顺序保持", () => {
    expect(roundtrip("a\nz", "a\ni1\ni2\ni3\nz")).toBe("a\ni1\ni2\ni3\nz");
  });

  it("空文件 → 多行", () => {
    expect(roundtrip("", "a\nb\nc")).toBe("a\nb\nc");
  });

  it("多行 → 空文件", () => {
    expect(roundtrip("a\nb\nc", "")).toBe("");
  });
});
