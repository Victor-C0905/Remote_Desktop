import { describe, expect, it } from "vitest";
import { isDangerousUrl } from "./dangerousUrl";

describe("isDangerousUrl（C-01）", () => {
  it("拦截脚本类 scheme（大小写不敏感）", () => {
    expect(isDangerousUrl("javascript:alert(1)")).toBe(true);
    expect(isDangerousUrl("data:text/html,<script>")).toBe(true);
    expect(isDangerousUrl("vbscript:x")).toBe(true);
    expect(isDangerousUrl("JavaScript:alert(1)")).toBe(true);
  });

  it("复现：内嵌控制字符绕过朴素正则，剥离后仍拦截", () => {
    expect(isDangerousUrl("java\tscript:alert(1)")).toBe(true);
    expect(isDangerousUrl("java\nscript:alert(1)")).toBe(true);
    expect(isDangerousUrl("java\x00script:alert(1)")).toBe(true);
  });

  it("正常地址放行", () => {
    expect(isDangerousUrl("https://example.com")).toBe(false);
    expect(isDangerousUrl("http://a.com/p?q=1")).toBe(false);
    expect(isDangerousUrl("about:blank")).toBe(false);
    // 包含"script"字样的普通域名/路径不误伤
    expect(isDangerousUrl("https://script.page")).toBe(false);
    expect(isDangerousUrl("https://a.com/javascript-tools")).toBe(false);
  });
});
