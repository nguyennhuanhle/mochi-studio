import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type ExportError } from "../bindings";
import { dismissExport, exportErrorMessage, formatEta, retryExport, useExportJobs } from "../lib/exports";
import { formatBytes } from "../lib/recorder";

/** X04: các lần xuất đang chạy / chờ / xong, hiện ở góc dưới phải trên mọi trang. */
export default function ExportPanel() {
  const { t } = useTranslation();
  const jobs = useExportJobs();
  const [retryError, setRetryError] = useState<Record<string, ExportError>>({});
  if (jobs.length === 0) return null;
  return (
    <div className="export-panel" aria-live="polite">
      {jobs.map((j) => {
        const pct = j.frames > 0 ? Math.min(100, (j.frame / j.frames) * 100) : 0;
        const file = j.output.split(/[\\/]/).pop();
        return (
          <div key={j.jobId} className="export-job">
            <div className="row">
              <span className="name" title={j.output}>
                {file || j.name}
              </span>
              {(j.state === "running" || j.state === "queued") && (
                <button className="link-btn" onClick={() => commands.exportCancel(j.jobId)}>
                  {t("common.cancel")}
                </button>
              )}
              {(j.state === "done" || j.state === "failed" || j.state === "cancelled") && (
                <button className="link-btn" onClick={() => dismissExport(j.jobId)} aria-label={t("common.close")}>
                  ✕
                </button>
              )}
            </div>
            {j.state === "queued" && <div className="muted">{t("export.queued")}</div>}
            {j.state === "running" && (
              <>
                <div className="export-bar">
                  <div style={{ width: `${pct}%` }} />
                </div>
                <div className="muted">
                  {j.frame > 0
                    ? t("export.progress", { pct: pct.toFixed(0), eta: formatEta(j.etaSecs), fps: j.fps.toFixed(0) })
                    : t("export.preparing")}
                  {j.encoder && ` · ${j.encoder}`}
                </div>
              </>
            )}
            {j.fallback && j.state !== "failed" && <div className="muted">{t("export.fallback", { to: j.fallback })}</div>}
            {j.state === "done" && (
              <>
                <div className="muted">{t("export.done", { size: formatBytes(j.bytes), secs: j.secs.toFixed(0) })}</div>
                <div className="row" style={{ marginTop: 6 }}>
                  <button onClick={() => commands.exportOpenFile(j.output)}>{t("export.openFile")}</button>
                  <button onClick={() => commands.revealPath(j.output)}>{t("export.openFolder")}</button>
                </div>
              </>
            )}
            {j.state === "cancelled" && <div className="muted">{t("export.err.cancelled")}</div>}
            {j.state === "failed" && j.error && (
              <>
                <div className="err">{exportErrorMessage(t, j.error)}</div>
                {retryError[j.jobId] && <div className="err">{exportErrorMessage(t, retryError[j.jobId])}</div>}
                <div className="row" style={{ marginTop: 6 }}>
                  {j.settings && (
                    <button
                      onClick={async () => {
                        const e = await retryExport(j);
                        if (e) setRetryError((r) => ({ ...r, [j.jobId]: e }));
                      }}
                    >
                      {t("export.retry")}
                    </button>
                  )}
                  {j.settings && retryError[j.jobId]?.code === "fileExists" && (
                    // K-X1: ghi đè chỉ sau khi người dùng bấm xác nhận rõ ràng cho đúng file này.
                    <button
                      onClick={async () => {
                        const e = await retryExport(j, true);
                        if (e) setRetryError((r) => ({ ...r, [j.jobId]: e }));
                      }}
                    >
                      {t("export.overwriteRetry")}
                    </button>
                  )}
                  {j.error.code === "crashed" && (
                    <button onClick={() => j.error?.code === "crashed" && commands.revealPath(j.error.log)}>{t("export.openLog")}</button>
                  )}
                </div>
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
