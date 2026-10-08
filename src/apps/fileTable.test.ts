// src/apps/fileTable.test.ts
// 文件列表表头领域逻辑单测：排序（点击表头翻转）+ 过滤（目录/文件）+ 列宽（拖拽调节/持久化默认值）
import { describe, it, expect } from "vitest";
import {
  sortFileEntries,
  DEFAULT_SORT,
  DEFAULT_COLUMN_WIDTHS,
  COLUMN_MIN_WIDTHS,
  clampColumnWidth,
  buildGridTemplate,
  filterFileEntries,
  DEFAULT_FILTER,
} from "./fileTable";
import type { FileEntry } from "./FileManager";

/* ── 测试数据工厂 ───────────────────────────────────────── */

function entry(partial: Partial<FileEntry> & { name: string }): FileEntry {
  return {
    is_dir: false,
    size: 0,
    mtime: "",
    permissions: "-rw-r--r--",
    ...partial,
  };
}

// 覆盖：目录/文件混合、目录 size 有值与为 0（协议端 is_dir → 0，但目录 size 无意义）、
// 时间格式变体混合（Z / +08:00 偏移 / 无时区）、空 mtime、相同 size（tie-break）
const FIXTURES: FileEntry[] = [
  entry({ name: "zeta.log", size: 2048, mtime: "2026-10-01T10:00:00Z", permissions: "-rw-r--r--" }),
  entry({ name: "alpha", is_dir: true, size: 4096, mtime: "2026-09-28T08:30:00Z", permissions: "drwxr-xr-x" }),
  entry({ name: "beta.md", size: 512, mtime: "2026-10-08T21:46:12+08:00", permissions: "-rw-r--r--" }),
  entry({ name: "Alpha.txt", size: 512, mtime: "2026-10-02T12:00:00", permissions: "-rw-------" }),
  entry({ name: "config", is_dir: true, size: 0, mtime: "2026-09-20T09:00:00Z", permissions: "drwx------" }),
  entry({ name: "gamma", size: 512, mtime: "", permissions: "-rwxr-xr-x" }),
];

function names(entries: FileEntry[]): string[] {
  return entries.map((e) => e.name);
}

/* ── 排序 ───────────────────────────────────────────────── */

describe("sortFileEntries", () => {
  it("默认排序（name asc）与历史行为一致：目录优先 + 名称 localeCompare", () => {
    const sorted = sortFileEntries(FIXTURES, DEFAULT_SORT);
    expect(names(sorted)).toEqual(["alpha", "config", "Alpha.txt", "beta.md", "gamma", "zeta.log"]);
  });

  it("所有排序键下目录始终排在文件前", () => {
    for (const key of ["name", "size", "mtime", "perm"] as const) {
      for (const dir of ["asc", "desc"] as const) {
        const sorted = sortFileEntries(FIXTURES, { key, dir });
        const dirCount = sorted.filter((e) => e.is_dir).length;
        // 前 dirCount 个必须全是目录
        expect(sorted.slice(0, dirCount).every((e) => e.is_dir)).toBe(true);
        expect(sorted.slice(dirCount).every((e) => !e.is_dir)).toBe(true);
      }
    }
  });

  it("name desc：组内名称翻转（目录仍在前）", () => {
    const sorted = sortFileEntries(FIXTURES, { key: "name", dir: "desc" });
    expect(names(sorted)).toEqual(["config", "alpha", "zeta.log", "gamma", "beta.md", "Alpha.txt"]);
  });

  it("size asc/desc：文件按数值比较；目录 size 无意义被忽略，组内按名称聚拢", () => {
    const asc = sortFileEntries(FIXTURES, { key: "size", dir: "asc" });
    // 目录组（alpha 4096 / config 0）：size 被忽略，恒按名称升序
    // 文件组：512 三者按名称次级键聚拢（Alpha.txt < beta.md < gamma），zeta.log 2048 最后
    expect(names(asc)).toEqual(["alpha", "config", "Alpha.txt", "beta.md", "gamma", "zeta.log"]);
    const desc = sortFileEntries(FIXTURES, { key: "size", dir: "desc" });
    // 降序：主键翻转，名称次级键仍恒升序（Explorer/Nautilus 惯例）
    expect(names(desc)).toEqual(["alpha", "config", "zeta.log", "Alpha.txt", "beta.md", "gamma"]);
  });

  it("mtime asc/desc：按真实时间数值比较，格式变体（Z/偏移/无时区）不影响顺序", () => {
    const asc = sortFileEntries(FIXTURES, { key: "mtime", dir: "asc" });
    // 目录组：config(09-20) < alpha(09-28)；文件组：zeta(10-01) < Alpha.txt(10-02) < beta(10-08)；
    // gamma 空时间排最后（不混进"最旧"，无论升降序）
    expect(names(asc)).toEqual(["config", "alpha", "zeta.log", "Alpha.txt", "beta.md", "gamma"]);
    const desc = sortFileEntries(FIXTURES, { key: "mtime", dir: "desc" });
    expect(names(desc)).toEqual(["alpha", "config", "beta.md", "Alpha.txt", "zeta.log", "gamma"]);
  });

  it("mtime 相同主值回退名称升序（组内聚拢，不随方向翻转）", () => {
    const same = [
      entry({ name: "b.txt", mtime: "2026-10-01T10:00:00Z" }),
      entry({ name: "a.txt", mtime: "2026-10-01T10:00:00Z" }),
      entry({ name: "c.txt", mtime: "2026-10-01T09:00:00Z" }),
    ];
    expect(names(sortFileEntries(same, { key: "mtime", dir: "asc" }))).toEqual(["c.txt", "a.txt", "b.txt"]);
    expect(names(sortFileEntries(same, { key: "mtime", dir: "desc" }))).toEqual(["a.txt", "b.txt", "c.txt"]);
  });

  it("不可解析的时间与空时间同等处理：排最后并按名称聚拢", () => {
    const bad = [
      entry({ name: "z.txt", mtime: "2026-10-01T10:00:00Z" }),
      entry({ name: "bad2.txt", mtime: "not-a-date" }),
      entry({ name: "bad1.txt", mtime: "" }),
      entry({ name: "a.txt", mtime: "2026-10-02T10:00:00Z" }),
    ];
    expect(names(sortFileEntries(bad, { key: "mtime", dir: "asc" })))
      .toEqual(["z.txt", "a.txt", "bad1.txt", "bad2.txt"]);
    expect(names(sortFileEntries(bad, { key: "mtime", dir: "desc" })))
      .toEqual(["a.txt", "z.txt", "bad1.txt", "bad2.txt"]);
  });

  it("perm asc/desc：按权限字符串比较（组内，同值按名称次级键聚拢）", () => {
    const asc = sortFileEntries(FIXTURES, { key: "perm", dir: "asc" });
    // 文件组字典序：-rw------- < -rw-r--r-- < -rwxr-xr-x；同值 beta.md/zeta.log 按名称聚拢
    expect(names(asc.slice(2))).toEqual(["Alpha.txt", "beta.md", "zeta.log", "gamma"]);
    const desc = sortFileEntries(FIXTURES, { key: "perm", dir: "desc" });
    expect(names(desc.slice(2))).toEqual(["gamma", "beta.md", "zeta.log", "Alpha.txt"]);
  });

  it("不修改入参数组（返回新数组）", () => {
    const original = [...FIXTURES];
    sortFileEntries(FIXTURES, { key: "size", dir: "desc" });
    expect(FIXTURES).toEqual(original);
  });

  it("空数组与单元素不崩溃", () => {
    expect(sortFileEntries([], DEFAULT_SORT)).toEqual([]);
    const single = [entry({ name: "only", is_dir: true })];
    expect(sortFileEntries(single, { key: "mtime", dir: "desc" })).toHaveLength(1);
  });
});

/* ── 过滤（目录/文件）──────────────────────────────────── */

describe("filterFileEntries", () => {
  it("默认 all 模式：原样返回全部条目", () => {
    expect(DEFAULT_FILTER).toBe("all");
    expect(filterFileEntries(FIXTURES, "all")).toHaveLength(6);
  });

  it("dirs 模式：只保留目录", () => {
    expect(names(filterFileEntries(FIXTURES, "dirs"))).toEqual(["alpha", "config"]);
  });

  it("files 模式：只保留文件（含零大小/空时间条目）", () => {
    expect(names(filterFileEntries(FIXTURES, "files")))
      .toEqual(["zeta.log", "beta.md", "Alpha.txt", "gamma"]);
  });
});

/* ── 列宽 ───────────────────────────────────────────────── */

describe("clampColumnWidth", () => {
  it("低于列最小值时夹取到最小值", () => {
    expect(clampColumnWidth("size", 10)).toBe(COLUMN_MIN_WIDTHS.size);
    expect(clampColumnWidth("name", 10)).toBe(COLUMN_MIN_WIDTHS.name);
  });

  it("超出上限时夹取（不低于最小值兜底）", () => {
    expect(clampColumnWidth("size", 500, 200)).toBe(200);
    // 上限比最小值还小：保底最小值，不产生矛盾值
    expect(clampColumnWidth("size", 500, 10)).toBe(COLUMN_MIN_WIDTHS.size);
  });

  it("无上限时不限制最大值，并取整", () => {
    expect(clampColumnWidth("mtime", 300.6)).toBe(301);
    expect(clampColumnWidth("perm", 120)).toBe(120);
  });
});

describe("buildGridTemplate", () => {
  it("默认值：名称列弹性（minmax），其余固定 px", () => {
    expect(buildGridTemplate(DEFAULT_COLUMN_WIDTHS)).toBe(
      "minmax(130px, 2fr) 90px 140px 90px"
    );
  });

  it("名称列被拖拽后转为固定 px", () => {
    expect(
      buildGridTemplate({ name: 400, size: 100, mtime: 150, perm: 80 })
    ).toBe("400px 100px 150px 80px");
  });
});

describe("DEFAULT_COLUMN_WIDTHS", () => {
  it("与 CSS 历史默认列宽一致", () => {
    expect(DEFAULT_COLUMN_WIDTHS).toEqual({ name: null, size: 90, mtime: 140, perm: 90 });
  });
});
