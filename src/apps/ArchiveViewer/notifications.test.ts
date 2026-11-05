import { describe, it, expect, beforeEach } from "vitest";
import { notifyArchiveFailure } from "./notifications";
import { useNotificationStore } from "../../stores/notificationStore";
import { useServersStore } from "../../stores/serversStore";
import { AuthMethod } from "../../types/server";

const auth = { method: AuthMethod.PASSWORD, username: "root" };

describe("notifyArchiveFailure", () => {
  beforeEach(() => {
    useNotificationStore.setState({ notifications: [] });
    useServersStore.setState({
      servers: [{
        id: "s1", name: "TXnotebook", host: "1.1.1.1", port: 22, auth,
        status: "connected" as const,
      }],
      activeServerId: null,
    });
  });

  it("全部解压失败写入通知中心（标题/正文/来源/服务器归属）", () => {
    notifyArchiveFailure({
      kind: "extractAll",
      serverId: "s1",
      archiveName: "a.tar.gz",
      message: "权限不足，无法写入目标目录",
      targetDir: "/tmp/out",
    });
    const [n] = useNotificationStore.getState().notifications;
    expect(n.title).toBe("解压失败");
    expect(n.body).toContain("权限不足");
    expect(n.body).toContain("a.tar.gz");
    expect(n.body).toContain("/tmp/out");
    expect(n.source).toBe("TXnotebook");
    expect(n.serverId).toBe("s1");
    expect(n.read).toBe(false);
  });

  it("单条提取失败使用独立标题；服务器不存在时回退到压缩包名", () => {
    notifyArchiveFailure({
      kind: "extractOne",
      serverId: "missing",
      archiveName: "a.zip",
      message: "压缩包损坏",
    });
    const [n] = useNotificationStore.getState().notifications;
    expect(n.title).toBe("提取失败");
    expect(n.source).toBe("a.zip");
  });

});
