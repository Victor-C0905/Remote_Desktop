import { describe, it, expect } from "vitest";
import {
  findArchiveSafetyIssue,
  hasTraversal,
  isSafeExtractOneEntry,
  isSafeExtractOnePath,
  treeHasTraversal,
} from "./pathSafety";
import type { ArchiveEntryKind } from "./parsers";
import type { ArchiveTreeNode } from "./tree";

const node = (
  path: string,
  isDir = false,
  children: ArchiveTreeNode[] = [],
  kind: ArchiveEntryKind = isDir ? "dir" : "file",
  linkTarget?: string,
): ArchiveTreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  isDir,
  kind,
  linkTarget,
  size: 0,
  children,
});

describe("hasTraversal", () => {
  it("拒绝绝对路径与 .. 段", () => {
    expect(hasTraversal("/etc/passwd")).toBe(true);
    expect(hasTraversal("../outside")).toBe(true);
    expect(hasTraversal("dir/../../outside")).toBe(true);
    expect(hasTraversal("a\0b")).toBe(true);
  });
  it("普通相对路径允许", () => {
    expect(hasTraversal("dir/file.txt")).toBe(false);
    expect(hasTraversal("file.txt")).toBe(false);
    expect(hasTraversal("./dir/file.txt")).toBe(false);
  });
});

describe("isSafeExtractOnePath", () => {
  it("拒绝以 - 开头的文件名（工具选项注入）", () => {
    expect(isSafeExtractOnePath("-rf")).toBe(false);
    expect(isSafeExtractOnePath("dir/-p")).toBe(false);
  });
  it("拒绝穿越，允许普通条目", () => {
    expect(isSafeExtractOnePath("../x")).toBe(false);
    expect(isSafeExtractOnePath("dir/file.txt")).toBe(true);
  });
});

describe("isSafeExtractOneEntry", () => {
  it("只有普通文件允许打开，链接/特殊文件拒绝", () => {
    expect(isSafeExtractOneEntry("dir/f.txt", "file")).toBe(true);
    expect(isSafeExtractOneEntry("link", "symlink")).toBe(false);
    expect(isSafeExtractOneEntry("hard", "hardlink")).toBe(false);
    expect(isSafeExtractOneEntry("dev", "special")).toBe(false);
  });
});

describe("treeHasTraversal", () => {
  it("检测嵌套的不安全条目", () => {
    expect(treeHasTraversal([node("a", true, [node("../evil")])])).toBe(true);
    expect(treeHasTraversal([node("a", true, [node("b.txt")])])).toBe(false);
    expect(treeHasTraversal([node("/abs")])).toBe(true);
  });
});

describe("findArchiveSafetyIssue", () => {
  it("按优先级返回 traversal / special / unsafeLink", () => {
    expect(findArchiveSafetyIssue([node("a", true, [node("../evil")])])).toBe("traversal");
    expect(findArchiveSafetyIssue([node("dev", false, [], "special")])).toBe("special");
    expect(findArchiveSafetyIssue([node("lnk", false, [], "symlink", "/etc")])).toBe("unsafeLink");
    expect(findArchiveSafetyIssue([node("lnk", false, [], "symlink", "dir/file.txt")])).toBeNull();
    expect(findArchiveSafetyIssue([node("dir", true, [node("f.txt")])])).toBeNull();
  });
});
