import type { MouseEvent as ReactMouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { commands, type JobStatus, type MediaItem, type ProjectView } from "../bindings";
import { useJobs } from "../lib/jobs";
import { formatTimecode } from "../lib/editor";


const KIND_ICON: Record<MediaItem["kind"], string> = {
  recording: "⏺",
  video: "🎞",
  audio: "♪",
  image: "🖼",
};

/** E01: kho media của project. Kéo một media thả lên timeline để thêm clip. */
export default function MediaBin(props: {
  view: ProjectView;
  onImport: () => void;
  onRemove: (m: MediaItem) => void;
  onRelink: (m: MediaItem) => void;
  onRelinkFolder: () => void;
  /** Bắt đầu kéo media (kéo bằng chuột, không dùng HTML5 DnD vì Tauri dùng nó cho file từ Explorer). */
  onDragStart: (m: MediaItem, e: ReactMouseEvent) => void;
  thumbs: Record<string, string | null>;
  /** S03: ảnh thu nhỏ / waveform lỗi (media id → lý do). */
  taskErrors: Record<string, string>;
  onRetryTasks: (id: string) => void;
}) {
  const { t } = useTranslation();
  const { view, thumbs } = props;
  const jobs = useJobs();
  return (
    <aside className="media-bin">
      <div className="pane-head">
        <strong>{t("editor.media")}</strong>
        <button className="primary small-btn" onClick={props.onImport}>
          + {t("editor.import")}
        </button>
      </div>
      {view.project.media.length === 0 && <p className="muted small">{t("editor.noMedia")}</p>}
      <ul className="media-list">
        {view.project.media.map((m) => {
          const missing = view.missing.includes(m.id);
          return (
            <li
              key={m.id}
              className={missing ? "media-item missing" : "media-item"}
              onMouseDown={(e) => {
                if (!missing && e.button === 0 && !(e.target as HTMLElement).closest("button")) props.onDragStart(m, e);
              }}
              title={m.absPath}
            >
              <div className="media-thumb">
                {thumbs[m.id] ? <img src={thumbs[m.id]!} alt="" /> : <span>{KIND_ICON[m.kind]}</span>}
              </div>
              <div className="media-meta">
                <span className="media-name">{m.name}</span>
                <span className="muted small">
                  {KIND_ICON[m.kind]} {formatTimecode(m.duration)}
                  {m.video ? ` · ${m.video.width}×${m.video.height}` : ""}
                </span>
                {!missing && jobs[m.id] && <ProxyBadge status={jobs[m.id]} onRetry={() => commands.jobsRetry(m.id)} />}
                {!missing && m.id in props.taskErrors && (
                  <span className="missing-badge" title={props.taskErrors[m.id]}>
                    {t("jobs.previewDataFailed")} ·{" "}
                    <button className="link-btn" onClick={() => props.onRetryTasks(m.id)}>
                      {t("jobs.retry")}
                    </button>
                  </span>
                )}
                {missing && (
                  <span className="missing-badge">
                    {t("editor.missing")} ·{" "}
                    <button className="link-btn" onClick={() => props.onRelink(m)}>
                      {t("editor.relink")}
                    </button>{" "}
                    <button className="link-btn" onClick={props.onRelinkFolder}>
                      {t("editor.relinkFolder")}
                    </button>
                  </span>
                )}
              </div>
              <button className="icon-x" title={t("editor.remove")} aria-label={t("editor.remove")} onClick={() => props.onRemove(m)}>
                ×
              </button>
            </li>
          );
        })}
      </ul>
    </aside>
  );
}

/** S03: trạng thái proxy của media nặng (preview dùng proxy, xuất luôn dùng file gốc). */
function ProxyBadge(props: { status: JobStatus; onRetry: () => void }) {
  const { t } = useTranslation();
  const s = props.status;
  switch (s.state) {
    case "queued":
      return <span className="muted small">{t("jobs.proxyQueued")}</span>;
    case "running":
      return <span className="muted small">{t("jobs.proxyRunning", { pct: Math.round(s.progress * 100) })}</span>;
    case "suspended":
      return <span className="muted small">{t("jobs.proxySuspended")}</span>;
    case "ready":
      return <span className="proxy-ok small">{t("jobs.proxyReady")}</span>;
    case "failed":
      return (
        <span className="missing-badge" title={s.message}>
          {t("jobs.proxyFailed")} ·{" "}
          <button className="link-btn" onClick={props.onRetry}>
            {t("jobs.retry")}
          </button>
        </span>
      );
  }
}
