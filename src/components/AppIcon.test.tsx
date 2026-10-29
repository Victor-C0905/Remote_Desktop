// src/components/AppIcon.test.tsx
// 渐变引用一致性防线：AppIcon body 引用 url(#icg-*) 渐变——普通实例的 defs
// 上移全局 sprite（body 直挂、常驻渲染，普通实例不再内嵌），强制主题实例用
// 「icg-<theme>-」后缀 id 内嵌于实例。曾因 icg-folder/icg-paper 未定义导致
// 文件夹/纸形按 fill:none 渲染（P0）；也因渐变 id 全文档去重导致强制主题
// 实例串色；又因首个 defs 宿主恰为 display:none 的最小化窗口，Chromium 无法
// 从隐藏子树绘制 paint server，全站磁贴透明（P0，2026-10-28）——此测试防止
// 三者回归。
import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
// tsconfig types 白名单未含 node（避免 node 全局类型污染 DOM 侧 setTimeout 推导），
// 但 vitest 运行时提供 node:fs，这里局部压制即可
// @ts-expect-error vitest 运行时提供 node:fs，仅类型声明缺失
import { readFileSync } from "node:fs";
import { AppIcon, type AppIconName } from "./AppIcon";

const ALL_NAMES: AppIconName[] = [
  "terminal", "files", "monitor", "editor", "settings", "browser",
  "image", "hex", "pdf", "archive", "package",
  "connection", "appearance", "keyboard", "notifications", "stats", "about",
  "documents", "pictures", "music", "videos", "network", "home",
  "download", "disk",
  "folder", "folder-open", "folder-plus", "file", "file-text", "file-code",
  "file-json", "file-image", "file-audio", "file-video", "file-archive",
  "config", "database", "lock",
  "trash", "search",
];

function refsAndIds(svg: Element) {
  const html = svg.innerHTML;
  const refs = [...html.matchAll(/url\(#([^)]+)\)/g)].map((m) => m[1]);
  const ids = [...html.matchAll(/id="([^"]+)"/g)].map((m) => m[1]);
  return { refs, ids: new Set(ids) };
}

describe("AppIcon 渐变引用一致性", () => {
  it.each([
    ["跟随全局", undefined],
    ["强制 classic", "classic"],
    ["强制 paper", "paper"],
  ] as const)("所有图标（%s）的 url(#…) 引用在同文档内必有定义", (_label, forced) => {
    for (const name of ALL_NAMES) {
      const { container } = render(
        <AppIcon name={name} iconTheme={forced} />
      );
      const svg = container.querySelector("svg");
      expect(svg, name).not.toBeNull();
      const { refs, ids } = refsAndIds(svg!);
      for (const ref of refs) {
        if (forced) {
          // 强制实例：渐变必须内嵌于自身（后缀 id，上下文=预览卡容器）
          expect(ids.has(ref), `${name} 引用的渐变 #${ref} 未在实例内定义`).toBe(true);
        } else {
          // 普通实例：defs 在全局 sprite（body 直挂），按文档 id 解析必须可达
          expect(
            document.getElementById(ref),
            `${name} 引用的渐变 #${ref} 在文档中无定义`
          ).not.toBeNull();
        }
      }
    }
  });

  // P0（2026-10-28）防线：普通实例引用的渐变 defs 必须挂在「常驻渲染」的
  // 全局 sprite（body 直挂）——若有人改回内嵌实例或把 sprite 挂进可被
  // display:none 的窗口容器，最小化窗口将再次使全站磁贴透明
  it("全局渐变 sprite 存在于 body 直挂位置，且覆盖普通实例引用的基础渐变", () => {
    render(<AppIcon name="folder" />);
    const sprite = document.getElementById("icon-defs-sprite");
    expect(sprite, "icon-defs-sprite 未挂载").not.toBeNull();
    expect(sprite!.parentElement).toBe(document.body);
    const spriteIds = new Set(
      [...sprite!.innerHTML.matchAll(/id="(icg-[^"]+)"/g)].map((m) => m[1])
    );
    // B() 磁贴、SHEET 纸形、全出血文件夹各自引用的基础渐变逐一在位
    for (const id of ["icg-blue", "icg-purple", "icg-folder", "icg-paper"]) {
      expect(spriteIds.has(id), `sprite 缺少渐变 #${id}`).toBe(true);
    }
  });

  it("普通实例不内嵌 defs（引用统一解析到全局 sprite）", () => {
    const { container } = render(<AppIcon name="files" />);
    const svg = container.querySelector("svg")!;
    expect(svg.innerHTML).toContain("url(#icg-blue)");
    expect(svg.innerHTML).not.toContain("<defs>");
  });

  it("强制主题实例的渐变 id 带主题后缀，与跟随全局的实例不共用 id", () => {
    const { container: forcedC } = render(<AppIcon name="folder" iconTheme="classic" />);
    const { container: plainC } = render(<AppIcon name="folder" />);
    expect(forcedC.querySelector("svg")!.innerHTML).toContain("icg-classic-folder");
    expect(plainC.querySelector("svg")!.innerHTML).toContain("url(#icg-folder)");
    expect(plainC.querySelector("svg")!.innerHTML).not.toContain("icg-classic-");
  });

  // 只查「id 有 defs」不够：defs 的 stop-color 与 body 的 fill/stroke 都走 var()，
  // var() 未定义时按规范回落黑色/初始值（icg-paper 引用 --ic-paper-light 曾致
  // classic 档纸形全黑）——必须校验每个 --ic-* 引用在 variables.css 里有定义
  it("所有图标引用的 --ic-* 变量在 variables.css 中必有定义", () => {
    // 相对 cwd（vitest 固定从仓库根运行）；不引 node:path 以少一个类型缺口
    const css = readFileSync("src/styles/variables.css", "utf8");
    const defined = new Set(css.match(/--ic-[\w-]+(?=\s*:)/g) ?? []);
    const referenced = new Set<string>();
    const collect = (svg: Element) => {
      for (const m of svg.innerHTML.matchAll(/var\((--ic-[\w-]+)[),]/g)) referenced.add(m[1]);
    };
    for (const forced of [undefined, "classic", "paper"] as const) {
      for (const name of ALL_NAMES) {
        const { container } = render(<AppIcon name={name} iconTheme={forced} />);
        collect(container.querySelector("svg")!);
      }
    }
    const missing = [...referenced].filter((v) => !defined.has(v));
    expect(missing, "以下 --ic-* 变量被图标引用但 variables.css 未定义").toEqual([]);
  });
});
