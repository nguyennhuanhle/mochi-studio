import { useState } from "react";
import { useTranslation } from "react-i18next";
import { saveBrand, useBrand } from "../lib/brand";
import Dialog from "./Dialog";

/** E14: sửa theme thương hiệu — tối đa 8 màu + font mặc định cho chữ mới. */
export default function BrandDialog(props: { onClose: () => void }) {
  const { t } = useTranslation();
  const brand = useBrand();
  const [colors, setColors] = useState<string[]>(brand.colors);
  const [font, setFont] = useState(brand.font);
  return (
    <Dialog
      title={t("brand.title")}
      onClose={props.onClose}
      actions={
        <>
          <button onClick={props.onClose}>{t("common.cancel")}</button>
          <button
            className="primary"
            onClick={async () => {
              await saveBrand({ colors, font });
              props.onClose();
            }}
          >
            {t("brand.save")}
          </button>
        </>
      }
    >
      <div className="stack">
        <span>{t("brand.colors")}</span>
        <div className="row wrap brand-colors">
          {colors.map((c, i) => (
            <span key={i} className="row">
              <input
                type="color"
                value={c}
                aria-label={t("brand.color", { n: i + 1 })}
                onChange={(e) => setColors(colors.map((x, j) => (j === i ? e.target.value.toUpperCase() : x)))}
              />
              <button className="tb" title={t("brand.removeColor")} onClick={() => setColors(colors.filter((_, j) => j !== i))}>
                ✕
              </button>
            </span>
          ))}
          {colors.length < 8 && <button onClick={() => setColors([...colors, "#FFFFFF"])}>{t("brand.addColor")}</button>}
        </div>
        <label className="field">
          <span>{t("brand.font")}</span>
          <input value={font} maxLength={80} placeholder="Segoe UI" onChange={(e) => setFont(e.target.value)} />
        </label>
        <p className="muted small">{t("brand.hint")}</p>
      </div>
    </Dialog>
  );
}
