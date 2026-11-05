/**
 * 压缩/解压能力卡片（设置页）
 *
 * 展示 Agent 探测到的格式可用性、实际工具与安装建议；
 * 支持手动重新检测（管理员中途安装工具后无需重连）。
 */

import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useArchiveCapabilitiesStore } from "../stores/archiveCapabilitiesStore";
import { archiveFormatLabel, describeCapability } from "../utils/archiveFormat";

export function ArchiveCapabilityCard({ serverId }: { serverId: string | null }) {
  const { t } = useTranslation();
  const state = useArchiveCapabilitiesStore((s) => (serverId ? s.byServer[serverId] : undefined));
  const fetchCapabilities = useArchiveCapabilitiesStore((s) => s.fetchCapabilities);

  useEffect(() => {
    if (serverId && !state) void fetchCapabilities(serverId);
  }, [serverId, state, fetchCapabilities]);

  const ready = state?.status === "ready" && state.data;
  const tools = ready ? state.data!.tools.map((tool) => tool.name).join(" / ") : "";

  return (
    <div className="st-card">
      <div className="st-card-header text-title">{t("archive.capability.title")}</div>
      {!serverId ? (
        <div className="st-hint">{t("archive.capability.disconnected")}</div>
      ) : !state || state.status === "probing" || (state.status === "ready" && !state.data) ? (
        <div className="st-hint">{t("archive.capability.checking")}</div>
      ) : state.status === "failed" ? (
        <div className="st-hint st-hint-warn">{state.error}</div>
      ) : (
        <>
          {tools && (
            <div className="st-hint">{t("archive.capability.tools", { tools })}</div>
          )}
          {state.data!.formats.map((cap) => {
            const status = cap.can_list
              ? cap.can_extract_all
                ? t("archive.capability.available", { tool: cap.tool ?? "?" })
                : t("archive.capability.partial", { tool: cap.tool ?? "?" })
              : describeCapability(state.data!.os, cap);
            return (
              <div className="st-option-row" key={cap.id}>
                <span className="st-option-label text-body">{archiveFormatLabel(cap.id)}</span>
                <span className={cap.can_list ? "st-hint" : "st-hint st-hint-warn"}>{status}</span>
              </div>
            );
          })}
        </>
      )}
      {serverId && (
        <div className="st-option-row">
          <button
            className="st-btn"
            disabled={state?.status === "probing"}
            onClick={() => void fetchCapabilities(serverId, true)}
          >
            {t("archive.capability.recheck")}
          </button>
        </div>
      )}
    </div>
  );
}
