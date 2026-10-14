import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { commands, type BackupInfo, type EditorError } from "../bindings";
import BackupList from "../components/BackupList";
import Dialog from "../components/Dialog";
import { editorErrorMessage } from "./editor";

/**
 * Mở một gói `.mochi` rồi vào trình biên tập. Xử lý lỗi theo use case:
 * hỏng → đề nghị mở backup; đang mở ở nơi khác (K-E3); phiên bản mới hơn (K-E6).
 */
export function useOpenProject() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [problem, setProblem] = useState<{ dir: string; error: EditorError } | null>(null);

  async function open(dir: string) {
    const r = await commands.projectOpen(dir);
    if (r.status === "ok") navigate("/editor");
    else setProblem({ dir, error: r.error });
  }

  async function openBackup(dir: string, b: BackupInfo) {
    const r = await commands.projectOpenBackup(dir, b.path);
    setProblem(null);
    if (r.status === "ok") navigate("/editor");
    else setProblem({ dir, error: r.error });
  }

  let dialog = null;
  if (problem) {
    const e = problem.error;
    const close = () => setProblem(null);
    if (e.code === "project" && e.error.code === "corrupt") {
      const latest = e.error.backups[0];
      // Bản mới nhất cũng hỏng thì người dùng chọn bản cũ hơn trong danh sách (giữ 10 bản).
      dialog = (
        <Dialog
          title={t("editor.corruptTitle")}
          onClose={close}
          actions={
            <>
              <button onClick={close}>{t("common.cancel")}</button>
              {latest && (
                <button className="primary" onClick={() => openBackup(problem.dir, latest)}>
                  {t("editor.openBackup", { at: new Date(latest.savedAt).toLocaleString() })}
                </button>
              )}
            </>
          }
        >
          <p>{t("editor.corruptBody")}</p>
          <p className="muted small">{e.error.detail}</p>
          {e.error.backups.length > 1 && (
            <details>
              <summary>{t("backups.older")}</summary>
              <BackupList backups={e.error.backups} onPick={(b) => openBackup(problem.dir, b)} />
            </details>
          )}
        </Dialog>
      );
    } else {
      const msg =
        e.code === "project" && e.error.code === "locked"
          ? t("editor.lockedBody")
          : e.code === "project" && e.error.code === "newerSchema"
            ? t("editor.newerBody", { found: e.error.found, supported: e.error.supported })
            : editorErrorMessage(t, e);
      dialog = (
        <Dialog title={t("app.name")} onClose={close} actions={<button onClick={close}>{t("common.close")}</button>}>
          <p>{msg}</p>
        </Dialog>
      );
    }
  }
  return { open, dialog };
}
