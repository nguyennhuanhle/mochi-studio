import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { EditorError } from "../bindings";
import { editorErrorMessage } from "../lib/editor";
import Dialog from "./Dialog";

/**
 * S01: đóng project / thoát app khi không lưu được → không bao giờ bỏ thay đổi âm thầm.
 * Người dùng chọn: thử lưu lại, bỏ thay đổi (xác nhận rõ), hoặc ở lại làm tiếp.
 */
export default function SaveFailedDialog(props: {
  error: EditorError;
  /** "close": nút ← trong editor; "exit": đóng cửa sổ chính. */
  mode: "close" | "exit";
  /** Trả về lỗi mới nếu vẫn không lưu được. */
  onRetry: () => Promise<EditorError | null>;
  onDiscard: () => void;
  onStay: () => void;
}) {
  const { t } = useTranslation();
  const [error, setError] = useState(props.error);
  const [busy, setBusy] = useState(false);
  return (
    <Dialog
      title={t("saveFailed.title")}
      onClose={props.onStay}
      actions={
        <>
          <button className="danger" disabled={busy} onClick={props.onDiscard}>
            {props.mode === "exit" ? t("saveFailed.discardExit") : t("saveFailed.discardClose")}
          </button>
          <button disabled={busy} onClick={props.onStay}>
            {t("saveFailed.stay")}
          </button>
          <button
            className="primary"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              const e = await props.onRetry();
              setBusy(false);
              if (e) setError(e);
            }}
          >
            {t("saveFailed.retry")}
          </button>
        </>
      }
    >
      <p>{t("saveFailed.body")}</p>
      <p className="err">{editorErrorMessage(t, error)}</p>
    </Dialog>
  );
}
