import { useTranslation } from "react-i18next";
import type { BackupInfo } from "../bindings";

/** Danh sách backup (mới nhất trước) để chọn mở. Bản người dùng tự lưu có nhãn riêng. */
export default function BackupList(props: { backups: BackupInfo[]; onPick: (b: BackupInfo) => void; disabled?: boolean }) {
  const { t } = useTranslation();
  if (props.backups.length === 0) return <p className="muted small">{t("backups.none")}</p>;
  return (
    <ul className="backup-list">
      {props.backups.map((b) => (
        <li key={b.path}>
          <button disabled={props.disabled} onClick={() => props.onPick(b)}>
            {new Date(b.savedAt).toLocaleString()}
            <span className="muted small"> · {b.manual ? t("backups.manual") : t("backups.auto")}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}
