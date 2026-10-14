import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Clip, EditOp, Ticks } from "../bindings";
import { secs, ticks } from "../lib/editor";
import { Section } from "./controls";

/**
 * Hiệu ứng v1 dùng chung cho mọi loại clip: freeze frame (E04)…
 * Các mục khác (keyframe, chuyển cảnh, màu & khung, âm thanh nâng cao) thêm dần theo Phase 6.
 */
export default function ClipFxInspector(props: { clip: Clip; playhead: Ticks; apply: (op: EditOp) => Promise<boolean> }) {
  const { clip } = props;
  const canFreeze = (clip.content.type === "video" || clip.content.type === "recording") && clip.hold == null;
  return <>{canFreeze && <FreezeSection {...props} />}</>;
}

function FreezeSection(props: { clip: Clip; playhead: Ticks; apply: (op: EditOp) => Promise<boolean> }) {
  const { t } = useTranslation();
  const { clip } = props;
  const [seconds, setSeconds] = useState("2");
  const end = clip.start + Math.round((clip.sourceOut - clip.sourceIn) / clip.speed);
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
