import { useState } from "react";
import { useTranslation } from "react-i18next";
import Dialog from "./Dialog";

/**
 * R11 / E14: preset có tên — chọn để áp, lưu cấu hình hiện tại thành preset, xóa preset.
 * Trùng tên khi lưu → ghi đè preset cũ (nút đổi thành "Ghi đè").
 */
export default function PresetBar(props: {
  names: string[];
  onApply: (name: string) => void;
  onSave: (name: string) => void;
  onDelete: (name: string) => void;
  /** Lý do chưa lưu được (vd. chưa chọn vùng quay); có → nút Lưu bị khóa. */
  cannotSave?: string;
}) {
  const { t } = useTranslation();
  const [chosen, setChosen] = useState("");
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const trimmed = name.trim();
  const exists = props.names.includes(trimmed);
  return (
    <div className="preset-bar row">
      <select
        value={props.names.includes(chosen) ? chosen : ""}
        aria-label={t("preset.choose")}
        onChange={(e) => {
          setChosen(e.target.value);
          if (e.target.value) props.onApply(e.target.value);
        }}
      >
        <option value="">{props.names.length ? t("preset.choose") : t("preset.none")}</option>
        {props.names.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </select>
      <button
        disabled={!!props.cannotSave}
        title={props.cannotSave}
        onClick={() => {
          setName(chosen);
          setNaming(true);
        }}
      >
        {t("preset.save")}
      </button>
      {props.names.includes(chosen) && (
        <button
          title={t("preset.delete")}
          onClick={() => {
            props.onDelete(chosen);
            setChosen("");
          }}
        >
          ✕
        </button>
      )}
      {naming && (
        <Dialog
          title={t("preset.saveTitle")}
          onClose={() => setNaming(false)}
          actions={
            <>
              <button onClick={() => setNaming(false)}>{t("common.cancel")}</button>
              <button
                className="primary"
                disabled={!trimmed || trimmed.length > 60}
                onClick={() => {
                  props.onSave(trimmed);
                  setChosen(trimmed);
                  setNaming(false);
                }}
              >
                {exists ? t("preset.overwrite") : t("preset.save")}
              </button>
            </>
          }
        >
          <label className="field">
            <span>{t("preset.name")}</span>
            <input autoFocus value={name} maxLength={60} onChange={(e) => setName(e.target.value)} />
          </label>
          {exists && <p className="muted small">{t("preset.exists", { name: trimmed })}</p>}
        </Dialog>
      )}
    </div>
  );
}
