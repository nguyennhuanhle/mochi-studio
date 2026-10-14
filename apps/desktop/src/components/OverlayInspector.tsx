import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Clip, ClipContent, EditOp, ShapeKind } from "../bindings";
import { Check, ColorField, Section, Select, Slider } from "./controls";

/** E10: chữ, vùng làm mờ / pixelate, hình khối, callout, spotlight. Vị trí / xoay / keyframe ở mục "Chuyển động". */
export default function OverlayInspector(props: { clip: Clip; apply: (op: EditOp) => Promise<boolean> }) {
  const { t } = useTranslation();
  const { clip } = props;
  const c = clip.content;
  const setContent = (content: ClipContent) => props.apply({ op: "setOverlay", clipId: clip.id, content });
  const pct = (v: number) => `${Math.round(v * 100)}%`;

  if (c.type === "text") {
    return (
      <Section title={t("polish.text")}>
        <TextBox value={c.text} onCommit={(text) => setContent({ ...c, text })} />
        <FontBox value={c.font} onCommit={(font) => setContent({ ...c, font })} />
        <Slider label={t("polish.fontSize")} value={c.sizePx} min={12} max={200} onCommit={(sizePx) => setContent({ ...c, sizePx })} />
        <ColorField label={t("polish.textColor")} value={c.color} onCommit={(color) => setContent({ ...c, color })} />
        <Check
          label={t("polish.textBg")}
          checked={c.background !== null}
          onChange={(on) => setContent({ ...c, background: on ? "#1D1B26CC" : null })}
        />
        {c.background && (
          <ColorField
            label={t("polish.textBg")}
            value={c.background}
            onCommit={(bg) => setContent({ ...c, background: `${bg}${c.background!.slice(7, 9) || "CC"}` })}
          />
        )}
      </Section>
    );
  }
  if (c.type === "blur") {
    return (
      <Section title={t("polish.blur")}>
        <Check label={t("polish.pixelate")} checked={c.pixelate} onChange={(pixelate) => setContent({ ...c, pixelate })} />
        <Slider label={t("polish.strength")} value={c.strength} min={0} max={1} step={0.05} format={pct} onCommit={(strength) => setContent({ ...c, strength })} />
        <Slider label={t("polish.x")} value={c.x} min={0} max={0.99} step={0.01} format={pct} onCommit={(x) => setContent({ ...c, x })} />
        <Slider label={t("polish.y")} value={c.y} min={0} max={0.99} step={0.01} format={pct} onCommit={(y) => setContent({ ...c, y })} />
        <Slider label={t("polish.w")} value={c.width} min={0.01} max={1} step={0.01} format={pct} onCommit={(width) => setContent({ ...c, width })} />
        <Slider label={t("polish.h")} value={c.height} min={0.01} max={1} step={0.01} format={pct} onCommit={(height) => setContent({ ...c, height })} />
      </Section>
    );
  }
  if (c.type === "shape") {
    const line = c.shape === "arrow" || c.shape === "line";
    return (
      <Section title={t("shape.title")}>
        <Select<ShapeKind>
          label={t("shape.kind")}
          value={c.shape}
          options={(["rectangle", "ellipse", "arrow", "line"] as ShapeKind[]).map((k) => ({ value: k, label: t(`shape.${k}`) }))}
          onChange={(shape) => setContent({ ...c, shape })}
        />
        <ColorField label={t("shape.stroke")} value={c.stroke} onCommit={(stroke) => setContent({ ...c, stroke })} />
        <Slider label={t("shape.strokeWidth")} value={c.strokeWidth} min={1} max={60} format={(v) => `${Math.round(v)} px`} onCommit={(strokeWidth) => setContent({ ...c, strokeWidth })} />
        {!line && (
          <>
            <Check label={t("shape.fill")} checked={c.fill !== null} onChange={(on) => setContent({ ...c, fill: on ? `${c.stroke.slice(0, 7)}55` : null })} />
            {c.fill && <ColorField label={t("shape.fill")} value={c.fill} onCommit={(fill) => setContent({ ...c, fill: `${fill}${c.fill!.slice(7, 9) || "55"}` })} />}
          </>
        )}
        <Slider label={line ? t("shape.fromX") : t("shape.left")} value={c.x0} min={0} max={1} step={0.01} format={pct} onCommit={(x0) => setContent({ ...c, x0 })} />
        <Slider label={line ? t("shape.fromY") : t("shape.top")} value={c.y0} min={0} max={1} step={0.01} format={pct} onCommit={(y0) => setContent({ ...c, y0 })} />
        <Slider label={line ? t("shape.toX") : t("shape.right")} value={c.x1} min={0} max={1} step={0.01} format={pct} onCommit={(x1) => setContent({ ...c, x1 })} />
        <Slider label={line ? t("shape.toY") : t("shape.bottom")} value={c.y1} min={0} max={1} step={0.01} format={pct} onCommit={(y1) => setContent({ ...c, y1 })} />
      </Section>
    );
  }
  if (c.type === "callout") {
    return (
      <Section title={t("callout.title")}>
        <TextBox value={c.text} onCommit={(text) => setContent({ ...c, text })} />
        <FontBox value={c.font} onCommit={(font) => setContent({ ...c, font })} />
        <Slider label={t("polish.fontSize")} value={c.sizePx} min={8} max={200} onCommit={(sizePx) => setContent({ ...c, sizePx })} />
        <ColorField label={t("polish.textColor")} value={c.color} onCommit={(color) => setContent({ ...c, color })} />
        <ColorField label={t("callout.background")} value={c.background} onCommit={(background) => setContent({ ...c, background })} />
        <Slider label={t("callout.tailX")} value={c.tailX} min={0} max={1} step={0.01} format={pct} onCommit={(tailX) => setContent({ ...c, tailX })} />
        <Slider label={t("callout.tailY")} value={c.tailY} min={0} max={1} step={0.01} format={pct} onCommit={(tailY) => setContent({ ...c, tailY })} />
        <p className="muted small">{t("callout.hint")}</p>
      </Section>
    );
  }
  if (c.type === "spotlight") {
    return (
      <Section title={t("spotlight.title")}>
        <Check label={t("spotlight.ellipse")} checked={c.ellipse} onChange={(ellipse) => setContent({ ...c, ellipse })} />
        <Slider label={t("spotlight.dim")} value={c.dim} min={0} max={1} step={0.05} format={pct} onCommit={(dim) => setContent({ ...c, dim })} />
        <Slider label={t("polish.x")} value={c.x} min={0} max={0.98} step={0.01} format={pct} onCommit={(x) => setContent({ ...c, x })} />
        <Slider label={t("polish.y")} value={c.y} min={0} max={0.98} step={0.01} format={pct} onCommit={(y) => setContent({ ...c, y })} />
        <Slider label={t("polish.w")} value={c.width} min={0.02} max={1} step={0.01} format={pct} onCommit={(width) => setContent({ ...c, width })} />
        <Slider label={t("polish.h")} value={c.height} min={0.02} max={1} step={0.01} format={pct} onCommit={(height) => setContent({ ...c, height })} />
      </Section>
    );
  }
  return null;
}

function TextBox(props: { value: string; onCommit: (v: string) => void }) {
  const [v, setV] = useState(props.value);
  useEffect(() => setV(props.value), [props.value]);
  return (
    <textarea
      className="text-box"
      rows={3}
      value={v}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => v !== props.value && props.onCommit(v)}
    />
  );
}

/** Họ font (font cài trên máy, ví dụ "Segoe UI", "Arial"). Ghi khi rời ô. */
function FontBox(props: { value: string; onCommit: (v: string) => void }) {
  const { t } = useTranslation();
  const [v, setV] = useState(props.value);
  useEffect(() => setV(props.value), [props.value]);
  return (
    <label className="field">
      <span>{t("polish.font")}</span>
      <input
        value={v}
        maxLength={80}
        onChange={(e) => setV(e.target.value)}
        onBlur={() => v.trim() && v !== props.value && props.onCommit(v.trim())}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
    </label>
  );
}
