import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useLocation, useNavigate } from "react-router";
import { commands, type FinishedRecording } from "../bindings";
import { formatDuration } from "../lib/recorder";
import Dialog from "../components/Dialog";
import { useOpenProject } from "../lib/useOpenProject";

/** Kết quả sau khi dừng quay. R08 (mở editor) có từ Vòng 2–3. */
export default function Recorded() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const rec = useLocation().state as FinishedRecording | null;
  const [askShort, setAskShort] = useState(!!rec?.tooShort);
  const { open, dialog } = useOpenProject();

  if (!rec) {
    return (
      <div className="page">
        <Link to="/">← {t("common.back")}</Link>
      </div>
    );
  }

  return (
    <div className="page">
      <h2 style={{ margin: 0 }}>✓ {t("recorded.title")}</h2>
      <div className="panel stack">
        <strong>{rec.name}</strong>
        <span>
          {t("recorded.duration")}: {formatDuration(rec.durationMs)} · {t("recorded.segments")}: {rec.segments}
        </span>
        <span className="muted small">
          {t("recorded.location")}: {rec.projectDir}
        </span>
      </div>
      <div className="row">
        <button className="primary" onClick={() => open(rec.projectDir)}>
          {t("recorded.openEditor")}
        </button>
        <button onClick={() => commands.revealPath(rec.recordingDir)}>{t("common.openFolder")}</button>
        <button onClick={() => navigate("/record")}>{t("recorded.again")}</button>
      </div>

      {dialog}
      {askShort && (
        <Dialog
          title={t("recorded.tooShortTitle")}
          onClose={() => setAskShort(false)}
          actions={
            <>
              <button
                className="danger"
                onClick={async () => {
                  await commands.recorderDiscard(rec.projectDir);
                  navigate("/record");
                }}
              >
                {t("common.discard")}
              </button>
              <button className="primary" onClick={() => setAskShort(false)}>
                {t("common.keep")}
              </button>
            </>
          }
        >
          <p>{t("recorded.tooShortBody", { secs: (rec.durationMs / 1000).toFixed(1) })}</p>
        </Dialog>
      )}
    </div>
  );
}
