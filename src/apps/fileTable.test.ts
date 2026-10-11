// src/apps/fileTable.test.ts
// 文件列表表头领域逻辑单测：排序（点击表头翻转）+ 列筛选（表头下拉）+ 列宽（拖拽调节/持久化默认值）
import { describe, it, expect } from "vitest";
import {
  sortFileEntries,
  DEFAULT_SORT,
  DEFAULT_COLUMN_WIDTHS,
  COLUMN_MIN_WIDTHS,
  clampColumnWidth,
  buildGridTemplate,
  sizeBucketOf,
  mtimeBucketOf,
  nameBucketOf,
  typeKeyOf,
  typeKeyLabel,
  listFileTypeBuckets,
  toggleBucket,
  filterByColumnFilters,
  hasActiveColumnFilters,
  DEFAULT_COLUMN_FILTERS,
  SIZE_BUCKETS,
  MTIME_BUCKETS,
  NAME_BUCKETS,
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
    for (const key of ["name", "size", "mtime", "type", "perm"] as const) {
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

  it("type asc/desc：文件组按扩展名字典序，同类型回退名称；无扩展名排组内最后", () => {
    const pool = [
      entry({ name: "z.md", mtime: "2026-10-01T00:00:00Z" }),
      entry({ name: "a.txt" }),
      entry({ name: "b.TXT" }),      // 同类型（大小写归一）
      entry({ name: "c.md" }),
      entry({ name: "noext", is_dir: false, permissions: "-rw-r--r--" }),  // 无扩展名视同未知类型
    ];
    const asc = sortFileEntries(pool, { key: "type", dir: "asc" });
    // md 组：c.md < z.md（同类型回退名称）；txt 组：a.txt < b.TXT；无扩展名排最后（同 mtime 空值惯例）
    expect(names(asc)).toEqual(["c.md", "z.md", "a.txt", "b.TXT", "noext"]);
    const desc = sortFileEntries(pool, { key: "type", dir: "desc" });
    // 主键翻转（txt 组先于 md 组），名称次级键仍恒升序，无扩展名仍排最后
    expect(names(desc)).toEqual(["a.txt", "b.TXT", "c.md", "z.md", "noext"]);
  });
});

/* ── 类型列（文件类型表头 + 动态类型筛选）────────────────── */

describe("typeKeyOf / typeKeyLabel", () => {
  it("目录 → __dir（标签「文件夹」）；无扩展名文件 → __none（标签「文件」）", () => {
    expect(typeKeyOf(entry({ name: "dir", is_dir: true }))).toBe("__dir");
    expect(typeKeyOf(entry({ name: "README" }))).toBe("__none");
    expect(typeKeyOf(entry({ name: ".hidden" }))).toBe("__none");  // 隐藏文件无扩展名语义
    expect(typeKeyLabel("__dir")).toBe("文件夹");
    expect(typeKeyLabel("__none")).toBe("文件");
  });

  it("扩展名小写归一（同一类型不同大小写同组）", () => {
    expect(typeKeyOf(entry({ name: "A.TXT" }))).toBe("txt");
    expect(typeKeyOf(entry({ name: "b.md" }))).toBe("md");
    expect(typeKeyOf(entry({ name: "archive.tar.gz" }))).toBe("gz");  // 取最后一段
    expect(typeKeyOf(entry({ name: "empty." }))).toBe("__none");      // 点结尾无扩展名
    expect(typeKeyLabel("txt")).toBe("TXT");
    expect(typeKeyLabel("gz")).toBe("GZ");
  });
});

describe("listFileTypeBuckets", () => {
  it("列出当前目录实际出现的类型：文件夹置顶，其余按类型字母序去重", () => {
    const pool = [
      entry({ name: "a.txt", is_dir: false }),
      entry({ name: "b.TXT", is_dir: false }),      // 与 a.txt 同类型
      entry({ name: "c.md", is_dir: false }),
      entry({ name: "dir1", is_dir: true }),
      entry({ name: "dir2", is_dir: true }),
    ];
    expect(listFileTypeBuckets(pool)).toEqual([
      { key: "__dir", label: "文件夹" },
      { key: "md", label: "MD" },
      { key: "txt", label: "TXT" },
    ]);
  });

  it("空目录返回空清单；无目录时不列文件夹项", () => {
    expect(listFileTypeBuckets([])).toEqual([]);
    expect(listFileTypeBuckets([entry({ name: "a.txt" })]))
      .toEqual([{ key: "txt", label: "TXT" }]);
  });
});

/* ── 列宽 ───────────────────────────────────────────────── */

describe("clampColumnWidth", () => {
  it("低于列最小值时夹取到最小值", () => {
    expect(clampColumnWidth("size", 10)).toBe(COLUMN_MIN_WIDTHS.size);
    expect(clampColumnWidth("name", 10)).toBe(COLUMN_MIN_WIDTHS.name);
    expect(clampColumnWidth("type", 10)).toBe(COLUMN_MIN_WIDTHS.type);
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
  it("默认值：名称列弹性（minmax），其余固定 px（列序 name/size/mtime/type/perm）", () => {
    expect(buildGridTemplate(DEFAULT_COLUMN_WIDTHS)).toBe(
      "minmax(130px, 2fr) 90px 140px 90px 90px"
    );
  });

  it("名称列被拖拽后转为固定 px", () => {
    expect(
      buildGridTemplate({ name: 400, size: 100, mtime: 150, type: 90, perm: 80 })
    ).toBe("400px 100px 150px 90px 80px");
  });
});

describe("DEFAULT_COLUMN_WIDTHS", () => {
  it("与 CSS 历史默认列宽一致", () => {
    expect(DEFAULT_COLUMN_WIDTHS).toEqual({ name: null, size: 90, mtime: 140, type: 90, perm: 90 });
  });
});

/* ── 列筛选（表头下拉菜单，Win11 Explorer 风格）──────────── */

const KB = 1024, MB = 1024 * 1024, GB = 1024 * 1024 * 1024;

describe("sizeBucketOf", () => {
  it("空文件 → empty（0 字节独立成组）", () => {
    expect(sizeBucketOf(entry({ name: "a", size: 0 }))).toBe("empty");
  });

  it("区间左开右闭：16 KB 归极小、1 MB 归小、128 MB 归中、1 GB 归大", () => {
    expect(sizeBucketOf(entry({ name: "t1", size: 1 }))).toBe("tiny");
    expect(sizeBucketOf(entry({ name: "t2", size: 16 * KB }))).toBe("tiny");
    expect(sizeBucketOf(entry({ name: "s1", size: 16 * KB + 1 }))).toBe("small");
    expect(sizeBucketOf(entry({ name: "s2", size: 1 * MB }))).toBe("small");
    expect(sizeBucketOf(entry({ name: "m1", size: 1 * MB + 1 }))).toBe("medium");
    expect(sizeBucketOf(entry({ name: "m2", size: 128 * MB }))).toBe("medium");
    expect(sizeBucketOf(entry({ name: "l1", size: 128 * MB + 1 }))).toBe("large");
    expect(sizeBucketOf(entry({ name: "l2", size: 1 * GB }))).toBe("large");
    expect(sizeBucketOf(entry({ name: "h", size: 1 * GB + 1 }))).toBe("huge");
  });

  it("目录 → null：size 恒为 0 但无意义，不参与大小筛选（对齐 Explorer：文件夹不落任何区间）", () => {
    expect(sizeBucketOf(entry({ name: "d", is_dir: true, size: 0 }))).toBeNull();
    expect(sizeBucketOf(entry({ name: "d2", is_dir: true, size: 4096 }))).toBeNull();
  });
});

describe("mtimeBucketOf", () => {
  // 固定 now = 2026-10-08（周四）12:00 本地时间 → 周一=10-05，昨天=10-07，上周=09-28~10-04
  // mtime 用无时区字符串（本地解释），保证测试在任何时区的机器上都稳定
  const NOW = new Date(2026, 9, 8, 12, 0).getTime();
  const bucket = (mtime: string) => mtimeBucketOf(entry({ name: "x", mtime }), NOW);

  it("今天：当日 0 点起至未来（同日），日界即边界", () => {
    expect(bucket("2026-10-08T00:00:00")).toBe("today");
    expect(bucket("2026-10-08T23:59:59")).toBe("today");
    expect(bucket("2026-10-08T18:00:00")).toBe("today");  // 未来时间（晚于 now）也归今天
  });

  it("昨天：前一本地日历日", () => {
    expect(bucket("2026-10-07T00:00:00")).toBe("yesterday");
    expect(bucket("2026-10-07T23:59:59")).toBe("yesterday");
  });

  it("本周：周一起（不含今昨），周一 0 点为周界", () => {
    expect(bucket("2026-10-05T00:00:00")).toBe("thisWeek");
    expect(bucket("2026-10-06T12:00:00")).toBe("thisWeek");
  });

  it("上周：上一个完整周（上周一 0 点 ~ 上周日 24 点）", () => {
    expect(bucket("2026-09-28T00:00:00")).toBe("lastWeek");
    expect(bucket("2026-10-04T23:59:59")).toBe("lastWeek");
  });

  it("更早：上周一 0 点之前", () => {
    expect(bucket("2026-09-27T23:59:59")).toBe("earlier");
    expect(bucket("2020-01-01T00:00:00")).toBe("earlier");
  });

  it("空时间与不可解析时间 → null，不落任何时间组", () => {
    expect(bucket("")).toBeNull();
    expect(bucket("not-a-date")).toBeNull();
  });
});

describe("toggleBucket", () => {
  it("不在列表 → 追加到末尾（新数组，不改入参）", () => {
    const prev: ("a" | "b")[] = ["a"];
    expect(toggleBucket(prev, "b")).toEqual(["a", "b"]);
    expect(prev).toEqual(["a"]);
  });

  it("已在列表 → 移除并保序", () => {
    expect(toggleBucket(["a", "b", "c"] as ("a" | "b" | "c")[], "b")).toEqual(["a", "c"]);
  });
});

describe("filterByColumnFilters", () => {
  const NOW = new Date(2026, 9, 8, 12, 0).getTime();
  // 覆盖：各大小区间 + 各时间组 + 目录（参与时间筛选、不参与大小筛选）+ 无时间条目
  const POOL: FileEntry[] = [
    entry({ name: "dir-today", is_dir: true, size: 0, mtime: "2026-10-08T09:00:00" }),
    entry({ name: "empty.txt", size: 0, mtime: "2026-10-08T10:00:00" }),
    entry({ name: "tiny.txt", size: 512, mtime: "2026-10-07T10:00:00" }),
    entry({ name: "small.txt", size: 100 * KB, mtime: "2026-10-06T10:00:00" }),
    entry({ name: "med.bin", size: 5 * MB, mtime: "2026-10-02T10:00:00" }),
    entry({ name: "big.iso", size: 500 * MB, mtime: "2025-01-01T10:00:00" }),
    entry({ name: "huge.img", size: 2 * GB, mtime: "" }),
  ];
  const f = (size: any[], mtime: any[]) =>
    filterByColumnFilters(POOL, { size, mtime, name: [], type: [] }, NOW);
  const nm = (es: FileEntry[]) => es.map((e) => e.name);

  it("无筛选：原样返回（不复制数组，零开销）", () => {
    expect(filterByColumnFilters(POOL, DEFAULT_COLUMN_FILTERS, NOW)).toBe(POOL);
  });

  it("大小筛选：命中区间并集；目录被滤除（不参与大小筛选）", () => {
    expect(nm(f(["empty"], []))).toEqual(["empty.txt"]);
    // 多选 = 并集
    expect(nm(f(["empty", "huge"], []))).toEqual(["empty.txt", "huge.img"]);
    expect(nm(f(["small", "medium"], []))).toEqual(["small.txt", "med.bin"]);
  });

  it("时间筛选：目录同参与；无时间条目被滤除", () => {
    expect(nm(f([], ["today"]))).toEqual(["dir-today", "empty.txt"]);
    // 昨天 = tiny.txt；目录 dir-today 不属昨天组
    expect(nm(f([], ["yesterday"]))).toEqual(["tiny.txt"]);
    expect(nm(f([], ["thisWeek"]))).toEqual(["small.txt"]);
    expect(nm(f([], ["lastWeek"]))).toEqual(["med.bin"]);
    expect(nm(f([], ["earlier"]))).toEqual(["big.iso"]);
    // 无时间条目（huge.img）不落任何组：勾选任何组都不可见
    expect(nm(f([], ["today", "yesterday", "thisWeek", "lastWeek", "earlier"]))).toHaveLength(6);
  });

  it("大小+时间同时激活：先过大小再过时间（AND 叠加）", () => {
    // today 组里的文件中只有 empty.txt 是 empty 区间
    expect(nm(f(["empty"], ["today"]))).toEqual(["empty.txt"]);
    // yesterday 组里 tiny.txt 在 tiny 区间
    expect(nm(f(["tiny"], ["yesterday"]))).toEqual(["tiny.txt"]);
    // 交集为空
    expect(nm(f(["large"], ["today"]))).toEqual([]);
  });

  it("空数组与全量默认值不崩溃", () => {
    expect(filterByColumnFilters([], DEFAULT_COLUMN_FILTERS, NOW)).toEqual([]);
  });

  it("名称筛选：按字母/拼音组并集过滤；目录同参与（名称组无 null 情形）", () => {
    const pool = [
      entry({ name: "a.txt" }),
      entry({ name: "宝.zip" }),      // 拼音 bǎo → b → A–F
      entry({ name: "z.txt" }),
      entry({ name: "0.txt" }),       // 数字开头 → 其他
      entry({ name: "dir-a", is_dir: true }),
    ];
    const nm = (es: FileEntry[]) => es.map((e) => e.name);
    expect(nm(filterByColumnFilters(pool, { name: ["aToF"], size: [], mtime: [], type: [] }, NOW)))
      .toEqual(["a.txt", "宝.zip", "dir-a"]);
    expect(nm(filterByColumnFilters(pool, { name: ["other"], size: [], mtime: [], type: [] }, NOW)))
      .toEqual(["0.txt"]);
    // 与大小筛选叠加（AND）
    expect(nm(filterByColumnFilters(pool, { name: ["sToZ"], size: [], mtime: [], type: [] }, NOW)))
      .toEqual(["z.txt"]);
  });

  it("类型筛选：按勾选类型并集过滤；目录经 __dir 组同参与", () => {
    const pool = [
      entry({ name: "a.txt" }),
      entry({ name: "b.md" }),
      entry({ name: "noext" }),                       // __none 组
      entry({ name: "dir1", is_dir: true }),
    ];
    const nm = (es: FileEntry[]) => es.map((e) => e.name);
    expect(nm(filterByColumnFilters(pool, { name: [], size: [], mtime: [], type: ["txt"] }, NOW)))
      .toEqual(["a.txt"]);
    // 多选并集（文件夹 + md）；过滤保持输入顺序，排序由 sortFileEntries 单独负责
    expect(nm(filterByColumnFilters(pool, { name: [], size: [], mtime: [], type: ["__dir", "md"] }, NOW)))
      .toEqual(["b.md", "dir1"]);
  });
});

describe("nameBucketOf", () => {
  it("ASCII 字母按首字母归组（不区分大小写，段边界 g/m/s）", () => {
    expect(nameBucketOf(entry({ name: "Alpha.txt" }))).toBe("aToF");
    expect(nameBucketOf(entry({ name: "file.bin" }))).toBe("aToF");   // f
    expect(nameBucketOf(entry({ name: "git" }))).toBe("gToL");
    expect(nameBucketOf(entry({ name: "list.txt" }))).toBe("gToL");  // l
    expect(nameBucketOf(entry({ name: "map.json" }))).toBe("mToR");
    expect(nameBucketOf(entry({ name: "run.sh" }))).toBe("mToR");     // r
    expect(nameBucketOf(entry({ name: "system.log" }))).toBe("sToZ");
    expect(nameBucketOf(entry({ name: "zeta.log" }))).toBe("sToZ");
  });

  it("汉字按拼音首字母归组（zh collation 拼音序）", () => {
    expect(nameBucketOf(entry({ name: "宝库.zip" }))).toBe("aToF");     // bǎo → b
    expect(nameBucketOf(entry({ name: "工作目录" }))).toBe("gToL");    // gōng → g
    expect(nameBucketOf(entry({ name: "蚂蚁笔记.md" }))).toBe("mToR");  // mǎ → m
    expect(nameBucketOf(entry({ name: "系统配置" }))).toBe("sToZ");     // xì → x
  });

  it("数字/符号/隐藏文件/emoji 归入其他组", () => {
    expect(nameBucketOf(entry({ name: "123.log" }))).toBe("other");
    expect(nameBucketOf(entry({ name: ".hidden" }))).toBe("other");
    expect(nameBucketOf(entry({ name: "_cache" }))).toBe("other");
    expect(nameBucketOf(entry({ name: "😀图片.png" }))).toBe("other");
  });

  it("NAME_BUCKETS 清单四段字母 + 其他，标签非空", () => {
    expect(NAME_BUCKETS.map((b) => b.key)).toEqual(["aToF", "gToL", "mToR", "sToZ", "other"]);
    expect(NAME_BUCKETS.every((b) => b.label.length > 0)).toBe(true);
  });
});

describe("hasActiveColumnFilters / 桶清单", () => {
  it("默认无筛选；任一列勾选即激活", () => {
    expect(hasActiveColumnFilters(DEFAULT_COLUMN_FILTERS)).toBe(false);
    expect(hasActiveColumnFilters({ size: [], mtime: ["today"], name: [], type: [] })).toBe(true);
    expect(hasActiveColumnFilters({ size: ["empty"], mtime: [], name: [], type: [] })).toBe(true);
    expect(hasActiveColumnFilters({ size: [], mtime: [], name: ["aToF"], type: [] })).toBe(true);
    expect(hasActiveColumnFilters({ size: [], mtime: [], name: [], type: ["txt"] })).toBe(true);
  });

  it("桶清单与 Win11 Explorer 分组一致（大小六档、时间五组），key 与标签一一对应", () => {
    expect(SIZE_BUCKETS.map((b) => b.key)).toEqual([
      "empty", "tiny", "small", "medium", "large", "huge",
    ]);
    expect(SIZE_BUCKETS.every((b) => b.label.length > 0)).toBe(true);
    expect(MTIME_BUCKETS.map((b) => b.key)).toEqual([
      "today", "yesterday", "thisWeek", "lastWeek", "earlier",
    ]);
    expect(MTIME_BUCKETS.every((b) => b.label.length > 0)).toBe(true);
  });
});
