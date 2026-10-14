import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import {
  commands,
  events,
  type EditorError,
  type RecorderError,
  type RecorderWarning,
  type RecoverableRecording,
} from "../bindings";
import { errorMessage, formatBytes, warningMessage } from "../lib/recorder";
import { useOpenProject } from "../lib/useOpenProject";
import Dialog from "./Dialog";
import ExportPanel from "./ExportPanel";
import SaveFailedDialog from "./SaveFailedDialog";

/** Chỉ hỏi khôi phục một lần mỗi lần mở app. */
let recoveryChecked = false;
/** Lý do bản quay vừa tự dừng, chờ MainShell của trang kế tiếp hiện. */
let pendingStopReason: RecorderWarning | null = null;

/**
 * Vỏ của cửa sổ chính: nhận kết quả quay (Finished/Failed) và hỏi khôi phục bản quay dở (S02).
 */
export default function MainShell({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [failed, setFailed] = useState<RecorderError | null>(null);
  const [recoverable, setRecoverable] = useState<RecoverableRecording[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [trimmed, setTrimmed] = useState<string | null>(null);
  const [exitError, setExitError] = useState<EditorError | null>(null);
  /** Lý do bản quay tự dừng (ổ đầy, lỗi pipeline): thanh nổi đã đóng nên báo lại ở cửa sổ chính. */
  const [stopReason, setStopReason] = useState<RecorderWarning | null>(null);
  useEffect(() => setStopReason(pendingStopReason), []);
  const { open: openProject, dialog: openDialog } = useOpenProject();

  // S01: đóng cửa sổ mà project chưa lưu được → backend chặn việc đóng, hỏi người dùng ở đây.
  useEffect(() => {
    const un = events.closeSaveFailed.listen(({ payload }) => setExitError(payload.error));
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Cache vượt giới hạn → đã xóa proxy cũ nhất: báo nhẹ.
  useEffect(() => {
    const un = events.cacheTrimmed.listen(({ payload }) => {
      setTrimmed(t("cache.trimmed", { files: payload.files, size: formatBytes(payload.bytes) }));
      setTimeout(() => setTrimmed(null), 6000);
    });
    return () => {
      un.then((f) => f());
    };
  }, [t]);

  useEffect(() => {
    let reason: RecorderWarning | null = null;
    const un = events.recorderEventMsg.listen(({ payload }) => {
      if (payload.type === "stateChanged" && payload.state === "preparing") {
        reason = null;
        pendingStopReason = null;
        setStopReason(null);
      }
      if (payload.type === "warning" && ["stoppedDiskFull", "pipelineFailed"].includes(payload.warning.code)) {
        reason = payload.warning;
      }
      if (payload.type === "finished") {
        // Trang sẽ chuyển (MainShell mới) → giữ lý do ở cấp module cho MainShell kế tiếp.
        pendingStopReason = reason;
        setStopReason(reason);
        // R08: dừng quay → mở thẳng trình biên tập. Bản quá ngắn thì hỏi giữ hay bỏ trước.
        if (payload.recording.tooShort) navigate("/recorded", { state: payload.recording });
        else openProject(payload.recording.projectDir);
      }
      if (payload.type === "failed") setFailed(payload.error);
    });
    return () => {
      un.then((f) => f());
    };
  }, [navigate]);

  useEffect(() => {
    // Bài tự chụp chỉ có ở bản dev: import động để bản release không chứa mã này.
    if (import.meta.env.DEV) import("../lib/devSnapshot").then((m) => m.runDevSnapshot(navigate));
  }, [navigate]);

  useEffect(() => {
    if (recoveryChecked) return;
    recoveryChecked = true;
    commands.recoveryList().then(setRecoverable);
  }, []);

  async function restore(r: RecoverableRecording) {
    setBusy(r.recordingDir);
    await commands.recoveryRestore(r.recordingDir);
    setBusy(null);
    setRecoverable((list) => list.filter((x) => x.recordingDir !== r.recordingDir));
  }

  async function discard(r: RecoverableRecording) {
    await commands.recoveryDiscard(r.recordingDir);
    setRecoverable((list) => list.filter((x) => x.recordingDir !== r.recordingDir));
  }

  return (
    <>
      {children}
      <ExportPanel />
      {exitError && (
        <SaveFailedDialog
          error={exitError}
          mode="exit"
          onRetry={async () => {
            const r = await commands.appExit(false);
            return r.status === "error" ? r.error : null;
          }}
          onDiscard={() => commands.appExit(true)}
          onStay={() => setExitError(null)}
        />
      )}
      {trimmed && <div className="toast muted-box">{trimmed}</div>}
      {openDialog}
      {stopReason && (
        <div className="toast error-box" role="alert">
          {warningMessage(t, stopReason)}
          <button
            onClick={() => {
              pendingStopReason = null;
              setStopReason(null);
            }}
          >
            {t("common.close")}
          </button>
        </div>
      )}
      {failed && (
        <div className="toast error-box" role="alert">
          {errorMessage(t, failed)}
          {failed.code === "permissionDenied" && (
            <button onClick={() => commands.openPrivacySettings(failed.kind)}>{t("common.openSettings")}</button>
          )}
          <button onClick={() => setFailed(null)}>{t("common.close")}</button>
        </div>
      )}
      {recoverable.length > 0 && (
        <Dialog
          title={t("recovery.title")}
          onClose={() => setRecoverable([])}
          actions={<button onClick={() => setRecoverable([])}>{t("common.close")}</button>}
        >
          <p>{t("recovery.body")}</p>
          <ul className="recovery-list">
            {recoverable.map((r) => (
              <li key={r.recordingDir} className="row">
                <span style={{ flex: 1 }}>
                  {t("recovery.item", {
                    name: r.name,
                    secs: (r.estimatedDurationMs / 1000).toFixed(0),
                    segments: r.segments,
                  })}
                </span>
                <button className="danger" disabled={busy !== null} onClick={() => discard(r)}>
                  {t("recovery.discard")}
                </button>
                <button className="primary" disabled={busy !== null} onClick={() => restore(r)}>
                  {busy === r.recordingDir ? t("recovery.restoring") : t("recovery.restore")}
                </button>
              </li>
            ))}
          </ul>
        </Dialog>
      )}
    </>
  );
}
