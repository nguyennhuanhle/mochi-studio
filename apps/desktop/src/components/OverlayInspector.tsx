import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Clip, ClipContent, EditOp, Transform } from "../bindings";
import { Check, ColorField, Section, Slider } from "./controls";

/** E10: chữ và vùng làm mờ / pixelate. */
export default function OverlayInspector(props: { clip: Clip; apply: (op: EditOp) => Promise<boolean> }) {
  const { t } = useTranslation();
  const { clip } = props;
  const c = clip.content;
  const setContent = (content: ClipContent) => props.apply({ op: "setOverlay", clipId: clip.id, content });

  if (c.type === "text") {
    const tf: Transform = clip.transform ?? { x: 0.5, y: 0.85, scale: 1, opacity: 1 };
    const setTf = (patch: Partial<Transform>) =>
      props.apply({ op: "setClipTransform", clipId: clip.id, transform: { ...tf, ...patch } });
    return (
      <Section title={t("polish.text")}>
        <TextBox value={c.text} onCommit={(text) => setContent({ ...c, text })} />
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
        <Slider label={t("polish.posX")} value={tf.x} min={0} max={1} step={0.01} format={(v) => `${Math.round(v * 100)}%`} onCommit={(x) => setTf({ x })} />
        <Slider label={t("polish.posY")} value={tf.y} min={0} max={1} step={0.01} format={(v) => `${Math.round(v * 100)}%`} onCommit={(y) => setTf({ y })} />
        <Slider label={t("polish.scale")} value={tf.scale} min={0.2} max={3} step={0.05} format={(v) => `${v.toFixed(2)}×`} onCommit={(scale) => setTf({ scale })} />
      </Section>
    );
  }
  if (c.type === "blur") {
    const pct = (v: number) => `${Math.round(v * 100)}%`;
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
