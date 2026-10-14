import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Clip, DeviceFrame, Easing, EditOp, Keyframe, Look, Ticks, Transform, TransitionKind } from "../bindings";
import { formatTimecode, secs, ticks } from "../lib/editor";
import { Check, ColorField, Section, Select, Slider } from "./controls";

/**
 * Hiệu ứng v1 dùng chung cho mọi loại clip: chuyển động + keyframe (E12), chuyển cảnh (E12),
 * màu & khung (E15), freeze frame (E04).
 */
export default function ClipFxInspector(props: { clip: Clip; playhead: Ticks; apply: (op: EditOp) => Promise<boolean> }) {
  const { clip } = props;
  const c = clip.content.type;
  const visual = c !== "audio" && c !== "blur" && c !== "spotlight";
  const media = c === "video" || c === "recording" || c === "image";
  const canFreeze = (c === "video" || c === "recording") && clip.hold == null;
  return (
    <>
      {visual && <MotionSection {...props} />}
      {c !== "audio" && <TransitionSection {...props} />}
      {media && <LookSection {...props} />}
      {canFreeze && <FreezeSection {...props} />}
    </>
  );
}

const DEFAULT_TF: Transform = { x: 0.5, y: 0.5, scale: 1, opacity: 1, rotation: 0 };

function easeApply(e: Easing, u: number) {
  const x = Math.min(1, Math.max(0, u));
  switch (e) {
    case "linear":
      return x;
    case "easeIn":
      return x * x * x;
    case "easeOut":
      return 1 - (1 - x) ** 3;
    default:
      return x < 0.5 ? 4 * x * x * x : 1 - (-2 * x + 2) ** 3 / 2;
  }
}

/** Như `Clip::transform_at` bên Rust: nội suy keyframe tại thời điểm timeline `t`. */
export function transformAt(clip: Clip, t: Ticks): Transform {
  const base = { ...DEFAULT_TF, ...(clip.transform ?? {}) };
  const ks = [...(clip.keyframes ?? [])].sort((a, b) => a.at - b.at);
  if (ks.length === 0) return base;
  const local = t - clip.start;
  const tf = (k: Keyframe): Transform => ({ x: k.x, y: k.y, scale: k.scale, rotation: k.rotation, opacity: k.opacity });
  if (local <= ks[0].at) return tf(ks[0]);
  if (local >= ks[ks.length - 1].at) return tf(ks[ks.length - 1]);
  const i = ks.findIndex((k) => k.at > local);
  const [a, b] = [ks[i - 1], ks[i]];
  const u = easeApply(b.easing, (local - a.at) / Math.max(1, b.at - a.at));
  const lerp = (p: number, q: number) => p + (q - p) * u;
  return { x: lerp(a.x, b.x), y: lerp(a.y, b.y), scale: lerp(a.scale, b.scale), rotation: lerp(a.rotation, b.rotation), opacity: lerp(a.opacity, b.opacity) };
}

function clipLength(clip: Clip) {
  return Math.round((clip.sourceOut - clip.sourceIn) / clip.speed);
}

function MotionSection(props: { clip: Clip; playhead: Ticks; apply: (op: EditOp) => Promise<boolean> }) {
  const { t } = useTranslation();
  const { clip } = props;
  const ks = clip.keyframes ?? [];
  const inside = props.playhead >= clip.start && props.playhead <= clip.start + clipLength(clip);
  // Đầu phát ngoài clip → hiện giá trị ở đầu clip.
  const at = inside ? props.playhead : clip.start;
  const cur = transformAt(clip, at);
  const frame = ticks(1 / 30);
  const local = Math.round((at - clip.start) / frame) * frame;
  const pct = (v: number) => `${Math.round(v * 100)}%`;
  const setKeyframes = (keyframes: Keyframe[]) => props.apply({ op: "setKeyframes", clipId: clip.id, keyframes });
  // Có keyframe → sửa giá trị = thêm / cập nhật keyframe tại đầu phát (kiểu Camtasia).
  const set = (patch: Partial<Transform>) => {
    const next = { ...cur, ...patch };
    if (ks.length === 0) return props.apply({ op: "setClipTransform", clipId: clip.id, transform: next });
    const existing = ks.find((k) => Math.abs(k.at - local) < frame / 2);
    const k: Keyframe = { at: local, x: next.x, y: next.y, scale: next.scale, rotation: next.rotation ?? 0, opacity: next.opacity, easing: existing?.easing ?? "easeInOut" };
    return setKeyframes([...ks.filter((x) => x !== existing), k]);
  };
  return (
    <Section title={t("fx.motion")} defaultOpen={ks.length > 0}>
      <Slider label={t("polish.posX")} value={cur.x} min={-0.5} max={1.5} step={0.01} format={pct} onCommit={(x) => set({ x })} />
      <Slider label={t("polish.posY")} value={cur.y} min={-0.5} max={1.5} step={0.01} format={pct} onCommit={(y) => set({ y })} />
      <Slider label={t("polish.scale")} value={cur.scale} min={0.1} max={5} step={0.05} format={(v) => `${v.toFixed(2)}×`} onCommit={(scale) => set({ scale })} />
      <Slider label={t("fx.rotation")} value={cur.rotation ?? 0} min={-180} max={180} step={1} format={(v) => `${Math.round(v)}°`} onCommit={(rotation) => set({ rotation })} />
      <Slider label={t("fx.opacity")} value={cur.opacity} min={0} max={1} step={0.01} format={pct} onCommit={(opacity) => set({ opacity })} />
      <div className="row wrap">
        <button
          disabled={!inside}
          title={inside ? t("fx.addKeyframeHint") : t("fx.freezeOutside")}
          onClick={() => {
            const k: Keyframe = { at: local, x: cur.x, y: cur.y, scale: cur.scale, rotation: cur.rotation ?? 0, opacity: cur.opacity, easing: "easeInOut" };
            setKeyframes([...ks.filter((x) => Math.abs(x.at - local) >= frame / 2), k]);
          }}
        >
          ◆ {t("fx.addKeyframe")}
        </button>
        {ks.length > 0 && <button onClick={() => setKeyframes([])}>{t("fx.clearKeyframes")}</button>}
      </div>
      {ks.length > 0 && (
        <ul className="plain-list keyframe-list">
          {[...ks]
            .sort((a, b) => a.at - b.at)
            .map((k) => (
              <li key={k.at} className="row">
                <span className="kf-time">◆ {formatTimecode(k.at)}</span>
                <select
                  value={k.easing}
                  aria-label={t("fx.easing")}
                  onChange={(e) => setKeyframes(ks.map((x) => (x === k ? { ...x, easing: e.target.value as Easing } : x)))}
                >
                  {(["linear", "easeIn", "easeOut", "easeInOut"] as Easing[]).map((e) => (
                    <option key={e} value={e}>
                      {t(`fx.ease_${e}`)}
                    </option>
                  ))}
                </select>
                <button className="tb" title={t("fx.deleteKeyframe")} onClick={() => setKeyframes(ks.filter((x) => x !== k))}>
                  ✕
                </button>
              </li>
            ))}
        </ul>
      )}
      <p className="muted small">{ks.length ? t("fx.keyframeHint") : t("fx.staticHint")}</p>
    </Section>
  );
}

const KINDS: TransitionKind[] = ["fade", "fadeBlack", "slideLeft", "slideRight", "slideUp", "slideDown", "wipeLeft", "wipeRight", "zoom", "typewriter"];

function TransitionSection(props: { clip: Clip; apply: (op: EditOp) => Promise<boolean> }) {
  const { t } = useTranslation();
  const { clip } = props;
  const isText = clip.content.type === "text" || clip.content.type === "callout";
  const kinds = KINDS.filter((k) => k !== "typewriter" || isText);
  const edge = (out: boolean) => {
    const tr = out ? clip.transitionOut : clip.transitionIn;
    const set = (kind: TransitionKind | "none", seconds: number) =>
      props.apply({
        op: "setTransition",
        clipId: clip.id,
        out,
        transition: kind === "none" ? null : { kind, duration: ticks(seconds) },
      });
    const dur = tr ? secs(tr.duration) : 0.5;
    return (
      <div className="stack">
        <Select<TransitionKind | "none">
          label={out ? t("fx.transitionOut") : t("fx.transitionIn")}
          value={tr?.kind ?? "none"}
          options={[{ value: "none", label: t("common.none") }, ...kinds.map((k) => ({ value: k, label: t(`fx.tr_${k}`) }))]}
          onChange={(k) => set(k, dur)}
        />
        {tr && (
          <Slider label={t("fx.duration")} value={dur} min={0.1} max={3} step={0.1} format={(v) => `${v.toFixed(1)} s`} onCommit={(v) => set(tr.kind, v)} />
        )}
      </div>
    );
  };
  return (
    <Section title={t("fx.transitions")} defaultOpen={!!(clip.transitionIn || clip.transitionOut)}>
      {edge(false)}
      {edge(true)}
      <p className="muted small">{t("fx.transitionHint")}</p>
    </Section>
  );
}

const DEVICES: DeviceFrame[] = ["none", "laptop", "phone", "browser"];

function LookSection(props: { clip: Clip; apply: (op: EditOp) => Promise<boolean> }) {
  const { t } = useTranslation();
  const look: Look = {
    brightness: 0,
    contrast: 0,
    saturation: 0,
    borderWidth: 0,
    borderColor: "#FFFFFF",
    cornerRadius: 0,
    shadow: 0,
    device: "none",
    ...(props.clip.look ?? {}),
  };
  const set = (patch: Partial<Look>) => props.apply({ op: "setLook", clipId: props.clip.id, look: { ...look, ...patch } });
  const signed = (v: number) => (v > 0 ? `+${Math.round(v)}` : `${Math.round(v)}`);
  const changed = JSON.stringify(look) !== JSON.stringify({ ...look, brightness: 0, contrast: 0, saturation: 0, borderWidth: 0, cornerRadius: 0, shadow: 0, device: "none" });
  return (
    <Section title={t("fx.look")} defaultOpen={changed}>
      <Select<DeviceFrame> label={t("fx.device")} value={look.device ?? "none"} options={DEVICES.map((d) => ({ value: d, label: t(`fx.dev_${d}`) }))} onChange={(device) => set({ device })} />
      <Slider label={t("fx.brightness")} value={look.brightness ?? 0} min={-100} max={100} format={signed} onCommit={(brightness) => set({ brightness })} />
      <Slider label={t("fx.contrast")} value={look.contrast ?? 0} min={-100} max={100} format={signed} onCommit={(contrast) => set({ contrast })} />
      <Slider label={t("fx.saturation")} value={look.saturation ?? 0} min={-100} max={100} format={signed} onCommit={(saturation) => set({ saturation })} />
      <Slider label={t("fx.cornerRadius")} value={look.cornerRadius ?? 0} min={0} max={50} format={(v) => `${Math.round(v)}%`} onCommit={(cornerRadius) => set({ cornerRadius })} />
      <Slider label={t("fx.borderWidth")} value={look.borderWidth ?? 0} min={0} max={40} format={(v) => `${Math.round(v)} px`} onCommit={(borderWidth) => set({ borderWidth })} />
      {(look.borderWidth ?? 0) > 0 && <ColorField label={t("fx.borderColor")} value={look.borderColor ?? "#FFFFFF"} onCommit={(borderColor) => set({ borderColor })} />}
      <Slider label={t("fx.shadow")} value={look.shadow ?? 0} min={0} max={100} format={(v) => `${Math.round(v)}%`} onCommit={(shadow) => set({ shadow })} />
      {changed && <button onClick={() => set({ brightness: 0, contrast: 0, saturation: 0, borderWidth: 0, cornerRadius: 0, shadow: 0, device: "none" })}>{t("fx.resetLook")}</button>}
      <Check label={t("fx.grayscale")} checked={(look.saturation ?? 0) <= -100} onChange={(on) => set({ saturation: on ? -100 : 0 })} />
    </Section>
  );
}

function FreezeSection(props: { clip: Clip; playhead: Ticks; apply: (op: EditOp) => Promise<boolean> }) {
  const { t } = useTranslation();
  const { clip } = props;
  const [seconds, setSeconds] = useState("2");
  const end = clip.start + clipLength(clip);
  const inside = props.playhead >= clip.start && props.playhead <= end;
  const duration = () => ticks(Math.min(600, Math.max(0.1, Number(seconds.replace(",", ".")) || 2)));
  return (
    <Section title={t("fx.freeze")} defaultOpen={false}>
      <label className="field">
        <span>
          {t("fx.freezeSecs")} <small className="muted">(0,1–600)</small>
        </span>
        <input type="text" inputMode="decimal" value={seconds} onChange={(e) => setSeconds(e.target.value)} />
      </label>
      <div className="row wrap">
        <button
          disabled={!inside}
          title={inside ? undefined : t("fx.freezeOutside")}
          onClick={() => props.apply({ op: "freezeFrame", clipId: clip.id, at: props.playhead, duration: duration() })}
        >
          {t("fx.freezeHere", { at: secs(props.playhead).toFixed(2) })}
        </button>
        <button onClick={() => props.apply({ op: "freezeFrame", clipId: clip.id, at: end, duration: duration() })}>
          {t("fx.extendLast")}
        </button>
      </div>
      <p className="muted small">{t("fx.freezeHint")}</p>
    </Section>
  );
}
