import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router";
import { commands } from "../bindings";

type Rect = { x: number; y: number; w: number; h: number };
const ASPECTS: { label: string; ratio: number | null }[] = [
  { label: "free", ratio: null },
  { label: "16:9", ratio: 16 / 9 },
  { label: "9:16", ratio: 9 / 16 },
  { label: "1:1", ratio: 1 },
  { label: "4:3", ratio: 4 / 3 },
];

/**
 * Khung chọn vùng (R01) phủ kín một màn hình. Toạ độ CSS của cửa sổ này chính là toạ độ
 * LOGIC trên màn hình đó — đúng đơn vị mà ScreenCaptureTarget::Area của Cap cần.
 */
export default function RegionPicker() {
  const { t } = useTranslation();
  const [params] = useSearchParams();
  const displayId = params.get("display") ?? "";
  const [rect, setRect] = useState<Rect | null>(null);
  const [ratio, setRatio] = useState<number | null>(null);
  const drag = useRef<{ x: number; y: number } | null>(null);
  const rectRef = useRef<Rect | null>(null);
  rectRef.current = rect;

  const finish = (r: Rect | null) =>
    commands.regionPickerFinish(
      r && r.w >= 16 && r.h >= 16
        ? { kind: "area", displayId, x: r.x, y: r.y, width: r.w, height: r.h }
        : null,
    );

  useEffect(() => {
    document.documentElement.classList.add("overlay-window");
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") finish(null);
      if (e.key === "Enter") finish(rectRef.current);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function compute(x0: number, y0: number, x1: number, y1: number): Rect {
    let w = Math.abs(x1 - x0);
    let h = Math.abs(y1 - y0);
    if (ratio && w > 0 && h > 0) {
      if (w / h > ratio) w = h * ratio;
      else h = w / ratio;
    }
    const x = x1 >= x0 ? x0 : x0 - w;
    const y = y1 >= y0 ? y0 : y0 - h;
    return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) };
  }

  return (
    <div
      className="region-picker"
      onMouseDown={(e) => {
        if ((e.target as HTMLElement).closest(".region-toolbar")) return;
        drag.current = { x: e.clientX, y: e.clientY };
        setRect({ x: e.clientX, y: e.clientY, w: 0, h: 0 });
      }}
      onMouseMove={(e) => {
        if (drag.current) setRect(compute(drag.current.x, drag.current.y, e.clientX, e.clientY));
      }}
      onMouseUp={() => {
        drag.current = null;
      }}
    >
      {rect && (
        <div className="region-rect" style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h }}>
          <span className="region-size">
            {rect.w} × {rect.h}
          </span>
        </div>
      )}
      <div className="region-toolbar">
        <span className="muted small">{t("region.hint")}</span>
        {ASPECTS.map((a) => (
          <button
            key={a.label}
            className={ratio === a.ratio ? "tab active" : "tab"}
            onClick={() => setRatio(a.ratio)}
          >
            {a.ratio === null ? t("region.free") : a.label}
          </button>
        ))}
        <button onClick={() => finish(null)}>{t("common.cancel")}</button>
        <button className="primary" disabled={!rect || rect.w < 16 || rect.h < 16} onClick={() => finish(rect)}>
          {t("region.confirm")}
        </button>
      </div>
    </div>
  );
}
