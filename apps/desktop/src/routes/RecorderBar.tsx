import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  events,
  type DeviceKind,
  type RecorderState,
  type RecorderWarning,
  type RecordingStats,
} from "../bindings";
import { formatBytes, formatDuration, isSevere, warningMessage } from "../lib/recorder";

/** Thanh điều khiển nổi khi đang quay (R07). Cửa sổ này bị loại khỏi bản quay. */
export default function RecorderBar() {
  const { t } = useTranslation();
  const [state, setState] = useState<RecorderState>("countdown");
  const [stats, setStats] = useState<RecordingStats | null>(null);
  const [warning, setWarning] = useState<RecorderWarning | null>(null);
  const [hasMic, setHasMic] = useState(false);
  /** R07: các nguồn của phiên quay (ngoài màn hình) và nguồn nào đang mất kết nối. */
  const [sources, setSources] = useState<DeviceKind[]>([]);
  const [lost, setLost] = useState<DeviceKind[]>([]);

  useEffect(() => {
    document.documentElement.classList.add("overlay-window");
    commands.recorderState().then((r) => r.status === "ok" && setState(r.data));
    commands.settingsGet().then((s) => {
      const c = s.lastRecording;
      setHasMic(!!c?.mic);
      setSources([...(c?.camera ? (["camera"] as const) : []), ...(c?.systemAudio ? (["systemAudio"] as const) : [])]);
    });
    let timer: number | undefined;
    const un = events.recorderEventMsg.listen(({ payload }) => {
      if (payload.type === "stateChanged") setState(payload.state);
      if (payload.type === "stats") setStats(payload.stats);
      if (payload.type === "warning") {
        const w = payload.warning;
        if (w.code === "deviceLost") setLost((l) => [...new Set([...l, w.kind])]);
        if (w.code === "deviceRestored") setLost((l) => l.filter((k) => k !== w.kind));
        setWarning(payload.warning);
        clearTimeout(timer);
        timer = window.setTimeout(() => setWarning(null), isSevere(payload.warning) ? 12000 : 6000);
      }
    });
    return () => {
      un.then((f) => f());
      clearTimeout(timer);
    };
  }, []);

  const busy = state === "finalizing" || state === "preparing";
  const paused = state === "paused";

  return (
    <div className="recorder-bar" data-tauri-drag-region>
      <span className={paused ? "rec-dot paused" : "rec-dot"} data-tauri-drag-region />
      <span className="rec-time" data-tauri-drag-region>
        {state === "finalizing" ? t("bar.finalizing") : formatDuration(stats?.elapsedMs ?? 0)}
      </span>
      <span className="rec-size muted" data-tauri-drag-region>
        {paused ? t("bar.paused") : stats ? formatBytes(stats.bytes) : ""}
      </span>
      <div className="rec-sources">
        {sources.map((k) => (
          <span
            key={k}
            className={lost.includes(k) ? "src-chip lost" : "src-chip"}
            title={t(lost.includes(k) ? "bar.sourceLost" : "bar.sourceOk", { name: t(`device.${k}`) })}
            aria-label={t(lost.includes(k) ? "bar.sourceLost" : "bar.sourceOk", { name: t(`device.${k}`) })}
          >
            {k === "camera" ? "📷" : "🔊"}
          </span>
        ))}
        {hasMic && lost.includes("microphone") && (
          <span className="src-chip lost" title={t("bar.sourceLost", { name: t("device.microphone") })}>
            🎙
          </span>
        )}
      </div>
      <div className="rec-actions">
        {hasMic && (
          <button
            className={stats?.micMuted ? "icon-btn off" : "icon-btn"}
            title={stats?.micMuted ? t("bar.unmute") : t("bar.mute")}
            aria-label={stats?.micMuted ? t("bar.unmute") : t("bar.mute")}
            disabled={busy}
            onClick={() => commands.recorderSetMicMuted(!stats?.micMuted)}
          >
            {stats?.micMuted ? "🔇" : "🎙"}
          </button>
        )}
        <button
          className="icon-btn"
          title={paused ? t("bar.resume") : t("bar.pause")}
          aria-label={paused ? t("bar.resume") : t("bar.pause")}
          disabled={busy || state === "countdown"}
          onClick={() => (paused ? commands.recorderResume() : commands.recorderPause())}
        >
          {paused ? "▶" : "❚❚"}
        </button>
        <button
          className="icon-btn stop"
          title={t("bar.stop")}
          aria-label={t("bar.stop")}
          disabled={busy}
          onClick={() => commands.recorderStop()}
        >
          ■
        </button>
        <button
          className="icon-btn"
          title={t("bar.cancel")}
          aria-label={t("bar.cancel")}
          disabled={busy}
          onClick={() => commands.recorderCancel()}
        >
          ✕
        </button>
      </div>
      {warning && (
        <div className={isSevere(warning) ? "bar-warning severe" : "bar-warning"} role="status">
          {warningMessage(t, warning)}
        </div>
      )}
    </div>
  );
}
