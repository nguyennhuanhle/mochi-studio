import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Clip, EditOp, ProjectView, Ticks } from "../bindings";
import { formatTimecode, secs, ticks } from "../lib/editor";
import ClipFxInspector from "./ClipFxInspector";
import OverlayInspector from "./OverlayInspector";
import RecordingInspector from "./RecordingInspector";

/** Thuộc tính clip đang chọn: tốc độ (E04), âm thanh cơ bản (E11). Giá trị ngoài giới hạn được kẹp ở Rust. */
export default function Inspector(props: {
  view: ProjectView;
  clipId: string | null;
  apply: (op: EditOp) => Promise<boolean>;
  playhead: Ticks;
  selectedZoom: string | null;
  onSelectZoom: (id: string | null) => void;
  onRegenerateZoom: (clipId: string) => void;
}) {
  const { t } = useTranslation();
  const clip = props.view.project.clips.find((c) => c.id === props.clipId) ?? null;
  const media = clip ? props.view.project.media.find((m) => m.id === clip.mediaId) : null;

  if (!clip) {
    return (
      <aside className="inspector">
        <div className="pane-head">
          <strong>{t("editor.inspector")}</strong>
        </div>
        <p className="muted small">{t("editor.noSelection")}</p>
      </aside>
    );
  }
  const duration = Math.round((clip.sourceOut - clip.sourceIn) / clip.speed);
  if (clip.content.type === "text" || clip.content.type === "blur") {
    return (
      <aside className="inspector">
        <div className="pane-head">
          <strong>{t("editor.inspector")}</strong>
        </div>
        <span className="muted small">
          {t("editor.clipStart")}: {formatTimecode(clip.start)} · {t("editor.clipDuration")}: {formatTimecode(duration)}
        </span>
        <OverlayInspector clip={clip} apply={props.apply} />
      </aside>
    );
  }
  const hasAudio = clip.content.type !== "image" && media?.kind !== "image";
  const canDetach =
    (clip.content.type === "video" && !!media?.audio) || (clip.content.type === "recording" && !clip.muted);
  const audio = (patch: { volumeDb?: number; fadeIn?: number; fadeOut?: number; muted?: boolean }) =>
    props.apply({
      op: "setClipAudio",
      clipId: clip.id,
      volumeDb: patch.volumeDb ?? null,
      fadeIn: patch.fadeIn ?? null,
      fadeOut: patch.fadeOut ?? null,
      muted: patch.muted ?? null,
    });

  return (
    <aside className="inspector">
      <div className="pane-head">
        <strong>{t("editor.inspector")}</strong>
      </div>
      <div className="stack">
        <strong className="ellipsis" title={media?.name}>
          {media?.name}
        </strong>
        <span className="muted small">
          {t("editor.clipStart")}: {formatTimecode(clip.start)} · {t("editor.clipDuration")}: {formatTimecode(duration)}
        </span>

        <NumberField
          label={t("editor.speed")}
          hint={t("editor.speedHint")}
          value={clip.speed}
          step={0.25}
          onCommit={(v) => props.apply({ op: "setClipSpeed", clipId: clip.id, speed: v })}
        />

        {hasAudio && (
          <>
            <label className="field">
              <span>
                {t("editor.volume")}: {clip.volumeDb.toFixed(0)} dB
              </span>
              <input
                type="range"
                min={-60}
                max={12}
                step={1}
                defaultValue={clip.volumeDb}
                key={`${clip.id}-${clip.volumeDb}`}
                onMouseUp={(e) => audio({ volumeDb: Number((e.target as HTMLInputElement).value) })}
                onKeyUp={(e) => audio({ volumeDb: Number((e.target as HTMLInputElement).value) })}
              />
            </label>
            <NumberField
              label={t("editor.fadeIn")}
              value={secs(clip.fadeIn)}
              step={0.1}
              onCommit={(v) => audio({ fadeIn: ticks(Math.max(0, v)) })}
            />
            <NumberField
              label={t("editor.fadeOut")}
              value={secs(clip.fadeOut)}
              step={0.1}
              onCommit={(v) => audio({ fadeOut: ticks(Math.max(0, v)) })}
            />
            <label className="check">
              <input type="checkbox" checked={clip.muted} onChange={(e) => audio({ muted: e.target.checked })} />
              <span>{t("editor.clipMuted")}</span>
            </label>
            {canDetach && (
              <button onClick={() => props.apply({ op: "detachAudio", clipId: clip.id })}>{t("polish.detachAudio")}</button>
            )}
          </>
        )}
        {clip.content.type === "recording" && (
          <RecordingInspector
            clip={clip}
            fx={clip.content.fx!}
            media={media ?? undefined}
            playhead={props.playhead}
            selectedZoom={props.selectedZoom}
            onSelectZoom={props.onSelectZoom}
            apply={props.apply}
            onRegenerate={() => props.onRegenerateZoom(clip.id)}
            otherRecordings={
              new Set(
                props.view.project.clips
                  .filter((c) => c.content.type === "recording" && c.mediaId !== clip.mediaId)
                  .map((c) => c.mediaId),
              ).size
            }
          />
        )}
        <ClipFxInspector clip={clip} playhead={props.playhead} apply={props.apply} />
      </div>
    </aside>
  );
}

/** Ô số chỉ gửi lệnh khi rời ô / Enter; hiển thị lại giá trị thật (đã kẹp) từ Rust. */
function NumberField(props: { label: string; hint?: string; value: number; step: number; onCommit: (v: number) => void }) {
  const [text, setText] = useState(String(round(props.value)));
  useEffect(() => setText(String(round(props.value))), [props.value]);
  const commit = () => {
    const v = Number(text.replace(",", "."));
    if (Number.isFinite(v) && v !== props.value) props.onCommit(v);
    else setText(String(round(props.value)));
  };
  return (
    <label className="field">
      <span>
        {props.label}
        {props.hint && <small className="muted"> ({props.hint})</small>}
      </span>
      <input
        type="text"
        inputMode="decimal"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
    </label>
  );
}

const round = (v: number) => Math.round(v * 100) / 100;

export type { Clip };
