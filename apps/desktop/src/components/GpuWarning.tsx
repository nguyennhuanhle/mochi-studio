import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type GpuCheck } from "../bindings";

let cached: GpuCheck | null = null;

/** Lỗi "GPU / driver không đạt yêu cầu tối thiểu": báo rõ, vẫn cho quay. */
export default function GpuWarning() {
  const { t } = useTranslation();
  const [check, setCheck] = useState<GpuCheck | null>(cached);
  useEffect(() => {
    if (!cached) commands.systemGpuCheck().then((c) => setCheck((cached = c)));
  }, []);
  if (!check || check.ok) return null;
  return (
    <div className="warn-box" role="alert">
      <strong>{t("gpu.title")}</strong>
      <div>
        {check.software
          ? t("gpu.software", { name: check.adapter })
          : check.oldDriver
            ? t("gpu.oldDriver", { name: check.adapter, driver: check.driver })
            : t("gpu.none", { detail: check.adapter })}
      </div>
      <div className="muted small">{t("gpu.requirement")}</div>
    </div>
  );
}
