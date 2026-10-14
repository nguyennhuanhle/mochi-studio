import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type BackupInfo, type ProjectView } from "../bindings";
import { editorErrorMessage } from "../lib/editor";
import BackupList from "./BackupList";
import Dialog from "./Dialog";

/**
 * Lần trước app bị đóng đột ngột: project đang mở bằng bản tự lưu gần nhất. ĐỀ NGHỊ (không tự
 * quyết): giữ bản tự lưu, hoặc mở một bản đã lưu trước đó. Bản tự lưu hiện tại được giữ thành
 * backup trước khi thay, nên đổi ý vẫn quay lại được.
 */
export default function RecoveredDialog(props: { dir: string; onDone: (v: ProjectView | null) => void }) {
  const { t } = useTranslation();
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    commands.projectBackups().then(setBackups);
  }, []);

  const lastManual = backups.find((b) => b.manual);

  async function pick(b: BackupInfo) {
    setBusy(true);
    const r = await commands.projectOpenBackup(props.dir, b.path);
    setBusy(false);
    if (r.status === "ok") props.onDone(r.data);
    else setError(editorErrorMessage(t, r.error));
  }

  return (
    <Dialog
      title={t("recover.title")}
      onClose={() => props.onDone(null)}
      actions={
        <>
          {lastManual && (
            <button disabled={busy} onClick={() => pick(lastManual)}>
              {t("recover.openManual", { at: new Date(lastManual.savedAt).toLocaleString() })}
            </button>
          )}
          <button className="primary" disabled={busy} onClick={() => props.onDone(null)}>
            {t("recover.keepAutosave")}
          </button>
        </>
      }
    >
      <p>{t("recover.body")}</p>
      <details>
        <summary>{t("recover.other")}</summary>
        <BackupList backups={backups} onPick={pick} disabled={busy} />
      </details>
      {error && <p className="err">{error}</p>}
    </Dialog>
  );
}
