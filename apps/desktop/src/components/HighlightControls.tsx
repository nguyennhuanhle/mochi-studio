import { useTranslation } from "react-i18next";
import type { CursorHighlight } from "../bindings";
import { Check, ColorField, Slider } from "./controls";

export const DEFAULT_HIGHLIGHT: CursorHighlight = { enabled: false, color: "#FFD54A", size: 160, opacity: 45 };

/**
 * R13 / E06: highlight con trỏ (vòng sáng quanh con trỏ, kiểu Camtasia). Dùng ở màn thiết lập quay
 * và trong mục Con trỏ của bảng thuộc tính.
 */
export default function HighlightControls(props: {
  value: CursorHighlight;
  onChange: (v: CursorHighlight) => void;
  /** Bản quay không có dữ liệu con trỏ → mờ + lý do. */
  disabledReason?: string;
}) {
  const { t } = useTranslation();
  const v = props.value;
  if (props.disabledReason) {
    return (
      <div className="highlight-controls disabled" title={props.disabledReason}>
        <Check label={t("highlight.enable")} checked={false} onChange={() => {}} />
        <span className="muted small">{props.disabledReason}</span>
      </div>
    );
  }
  return (
    <div className="highlight-controls">
      <Check label={t("highlight.enable")} checked={v.enabled} onChange={(enabled) => props.onChange({ ...v, enabled })} />
      {v.enabled && (
        <>
          <div className="row">
            <ColorField label={t("highlight.color")} value={v.color} onCommit={(color) => props.onChange({ ...v, color })} />
            <span className="highlight-swatch" style={{ background: v.color, opacity: v.opacity / 100 }} aria-hidden />
          </div>
          <Slider
            label={t("highlight.size")}
            value={v.size}
            min={50}
            max={500}
            step={10}
            format={(x) => `${Math.round(x)}%`}
            onCommit={(size) => props.onChange({ ...v, size })}
          />
          <Slider
            label={t("highlight.opacity")}
            value={v.opacity}
            min={10}
            max={100}
            step={5}
            format={(x) => `${Math.round(x)}%`}
            onCommit={(opacity) => props.onChange({ ...v, opacity })}
          />
        </>
      )}
    </div>
  );
}
