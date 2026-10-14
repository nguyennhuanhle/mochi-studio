import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type Clip, type EditOp, type MediaItem, type RecordingFx, type Ticks, type ZoomSegment } from "../bindings";
import { formatTimecode, secs, ticks } from "../lib/editor";
import { Check, ColorField, Section, Select, Slider } from "./controls";
import HighlightControls from "./HighlightControls";

function cornerCenter(c: "topLeft" | "topRight" | "bottomLeft" | "bottomRight") {
  return { x: c.endsWith("Left") ? 0.15 : 0.85, y: c.startsWith("top") ? 0.15 : 0.85 };
}

/** Ô số giây, ghi khi rời ô / Enter. */
function SecsInput(props: { value: number; onCommit: (v: number) => void }) {
  return (
    <input
      key={props.value.toFixed(2)}
      type="number"
      step={0.1}
      min={0}
      defaultValue={props.value.toFixed(1)}
      style={{ width: 64 }}
      onBlur={(e) => {
        const v = Number(e.target.value);
        if (Number.isFinite(v) && Math.abs(v - props.value) > 0.001) props.onCommit(v);
      }}
      onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
    />
  );
}

/** E05–E08: hiệu ứng của clip bản quay (nền, con trỏ, webcam, zoom). */
export default function RecordingInspector(props: {
  clip: Clip;
  fx: RecordingFx;
  media: MediaItem | undefined;
  playhead: Ticks;
  selectedZoom: string | null;
  onSelectZoom: (id: string | null) => void;
  apply: (op: EditOp) => Promise<boolean>;
  onRegenerate: () => void;
}) {
  const { t } = useTranslation();
  const { clip, fx } = props;
  const set = (patch: (f: RecordingFx) => RecordingFx) =>
    props.apply({ op: "setRecordingFx", clipId: clip.id, fx: patch(structuredClone(fx)) });
  const bg = fx.background;
  const hasCamera = props.media?.recording?.hasCamera ?? false;
  /** Thời điểm trong media ứng với đầu phát (zoom / ẩn webcam lưu theo thời gian trong media). */
  const mediaAtPlayhead = Math.max(
    clip.sourceIn,
    Math.min(clip.sourceOut, clip.sourceIn + Math.round((props.playhead - clip.start) * clip.speed)),
  );
  const zoom = fx.zoom.segments.find((z) => z.id === props.selectedZoom) ?? null;

  return (
    <div className="stack">
      <Section title={t("polish.background")}>
        <Select
          label={t("polish.background")}
          value={bg.kind}
          options={[
            { value: "none", label: t("polish.bgNone") },
            { value: "color", label: t("polish.bgColor") },
            { value: "gradient", label: t("polish.bgGradient") },
            { value: "image", label: t("polish.bgImage") },
          ]}
          onChange={(kind) => set((f) => ({ ...f, background: { ...f.background, kind } }))}
        />
        {(bg.kind === "color" || bg.kind === "gradient") && (
          <ColorField label={t("polish.color")} value={bg.color} onCommit={(color) => set((f) => ({ ...f, background: { ...f.background, color } }))} />
        )}
        {bg.kind === "gradient" && (
          <>
            <ColorField label={t("polish.color2")} value={bg.color2} onCommit={(color2) => set((f) => ({ ...f, background: { ...f.background, color2 } }))} />
            <Slider label={t("polish.angle")} value={bg.angle} min={0} max={360} format={(v) => `${Math.round(v)}°`} onCommit={(angle) => set((f) => ({ ...f, background: { ...f.background, angle } }))} />
          </>
        )}
        {bg.kind === "image" && (
          <button
            onClick={async () => {
              const [path] = await commands.pickMediaFiles();
              if (path) set((f) => ({ ...f, background: { ...f.background, image: path } }));
            }}
          >
            {t("polish.pickImage")} {bg.image ? `(${bg.image.split(/[\\/]/).pop()})` : ""}
          </button>
        )}
        {bg.kind !== "none" && (
          <>
            <Slider label={t("polish.padding")} value={bg.padding} min={0} max={40} onCommit={(padding) => set((f) => ({ ...f, background: { ...f.background, padding } }))} />
            <Slider label={t("polish.rounding")} value={bg.rounding} min={0} max={100} onCommit={(rounding) => set((f) => ({ ...f, background: { ...f.background, rounding } }))} />
            <Slider label={t("polish.shadow")} value={bg.shadow} min={0} max={100} onCommit={(shadow) => set((f) => ({ ...f, background: { ...f.background, shadow } }))} />
          </>
        )}
        <Slider
          label={t("polish.motionBlur")}
          value={bg.motionBlur}
          min={0}
          max={1}
          step={0.05}
          format={(v) => `${Math.round(v * 100)}%`}
          onCommit={(motionBlur) => set((f) => ({ ...f, background: { ...f.background, motionBlur } }))}
        />
      </Section>

      <Section title={t("polish.cursor")}>
        <Check label={t("polish.cursorHidden")} checked={fx.cursor.hidden} onChange={(hidden) => set((f) => ({ ...f, cursor: { ...f.cursor, hidden } }))} />
        {!fx.cursor.hidden && (
          <>
            <Slider label={t("polish.cursorSize")} value={fx.cursor.size} min={50} max={300} format={(v) => `${Math.round(v)}%`} onCommit={(size) => set((f) => ({ ...f, cursor: { ...f.cursor, size } }))} />
            <Check label={t("polish.hideIdle")} checked={fx.cursor.hideWhenIdle} onChange={(hideWhenIdle) => set((f) => ({ ...f, cursor: { ...f.cursor, hideWhenIdle } }))} />
            <Check label={t("polish.smooth")} checked={fx.cursor.smooth} onChange={(smooth) => set((f) => ({ ...f, cursor: { ...f.cursor, smooth } }))} />
            <Select
              label={t("polish.clickEffect")}
              value={fx.cursor.clickEffect}
              options={[
                { value: "none", label: t("polish.clickNone") },
                { value: "ripple", label: t("polish.clickRipple") },
              ]}
              onChange={(clickEffect) => set((f) => ({ ...f, cursor: { ...f.cursor, clickEffect } }))}
            />
            {/* E06 (v1.3): highlight con trỏ. Bản quay không có dữ liệu con trỏ → mờ + lý do. */}
            <HighlightControls
              value={fx.cursor.highlight}
              onChange={(highlight) => set((f) => ({ ...f, cursor: { ...f.cursor, highlight } }))}
              disabledReason={props.media?.recording?.hasCursor === false ? t("highlight.noCursorData") : undefined}
            />
          </>
        )}
      </Section>

      <Section title={t("polish.camera")} defaultOpen={hasCamera}>
        {!hasCamera ? (
          <p className="muted small">{t("polish.noCamera")}</p>
        ) : (
          <>
            <Check label={t("polish.cameraVisible")} checked={fx.camera.visible} onChange={(visible) => set((f) => ({ ...f, camera: { ...f.camera, visible } }))} />
            <Select
              label={t("polish.shape")}
              value={fx.camera.shape}
              options={[
                { value: "circle", label: t("polish.circle") },
                { value: "rounded", label: t("polish.rounded") },
                { value: "source", label: t("polish.source") },
              ]}
              onChange={(shape) => set((f) => ({ ...f, camera: { ...f.camera, shape } }))}
            />
            <Select
              label={t("polish.corner")}
              value={fx.camera.position ? "custom" : fx.camera.corner}
              options={[
                { value: "topLeft", label: t("polish.topLeft") },
                { value: "topRight", label: t("polish.topRight") },
                { value: "bottomLeft", label: t("polish.bottomLeft") },
                { value: "bottomRight", label: t("polish.bottomRight") },
                { value: "custom", label: t("polish.customPosition") },
              ]}
              onChange={(v) =>
                set((f) => ({
                  ...f,
                  camera:
                    v === "custom"
                      ? { ...f.camera, position: f.camera.position ?? cornerCenter(f.camera.corner) }
                      : { ...f.camera, corner: v, position: null },
                }))
              }
            />
            {fx.camera.position && (
              // E08: vị trí tự do — tâm webcam theo % khung (renderer giữ webcam nằm trọn trong khung).
              <>
                <Slider
                  label={t("polish.cameraX")}
                  value={fx.camera.position.x * 100}
                  min={0}
                  max={100}
                  format={(v) => `${Math.round(v)}%`}
                  onCommit={(x) => set((f) => ({ ...f, camera: { ...f.camera, position: { x: x / 100, y: f.camera.position?.y ?? 0.5 } } }))}
                />
                <Slider
                  label={t("polish.cameraY")}
                  value={fx.camera.position.y * 100}
                  min={0}
                  max={100}
                  format={(v) => `${Math.round(v)}%`}
                  onCommit={(y) => set((f) => ({ ...f, camera: { ...f.camera, position: { x: f.camera.position?.x ?? 0.5, y: y / 100 } } }))}
                />
              </>
            )}
            <Slider label={t("polish.cameraSize")} value={fx.camera.size} min={10} max={50} format={(v) => `${Math.round(v)}%`} onCommit={(size) => set((f) => ({ ...f, camera: { ...f.camera, size } }))} />
            <Check label={t("polish.mirror")} checked={fx.camera.mirror} onChange={(mirror) => set((f) => ({ ...f, camera: { ...f.camera, mirror } }))} />
            <Check label={t("polish.cameraBorder")} checked={fx.camera.border} onChange={(border) => set((f) => ({ ...f, camera: { ...f.camera, border } }))} />
            {fx.camera.border && (
              <>
                <ColorField label={t("polish.borderColor")} value={fx.camera.borderColor} onCommit={(borderColor) => set((f) => ({ ...f, camera: { ...f.camera, borderColor } }))} />
                <Slider
                  label={t("polish.borderWidth")}
                  value={fx.camera.borderWidth}
                  min={0.5}
                  max={10}
                  step={0.5}
                  format={(v) => `${v.toFixed(1)}%`}
                  onCommit={(borderWidth) => set((f) => ({ ...f, camera: { ...f.camera, borderWidth } }))}
                />
              </>
            )}
            <button
              onClick={() =>
                set((f) => ({
                  ...f,
                  camera: {
                    ...f.camera,
                    hiddenRanges: [
                      ...f.camera.hiddenRanges,
                      { start: Math.max(0, mediaAtPlayhead - ticks(2)), end: mediaAtPlayhead + ticks(2) },
                    ],
                  },
                }))
              }
            >
              {t("polish.hideCameraHere")}
            </button>
            {fx.camera.hiddenRanges.length > 0 && (
              <>
                <span className="row space small">
                  <span className="muted">{t("polish.hiddenRanges", { n: fx.camera.hiddenRanges.length })}</span>
                  <button className="link-btn" onClick={() => set((f) => ({ ...f, camera: { ...f.camera, hiddenRanges: [] } }))}>
                    {t("polish.clearHidden")}
                  </button>
                </span>
                {/* E08: sửa / xóa từng đoạn ẩn (giây trong bản quay). */}
                <ul className="hidden-ranges">
                  {fx.camera.hiddenRanges.map((r, i) => (
                    <li key={i} className="row small">
                      <SecsInput
                        value={secs(r.start)}
                        onCommit={(v) =>
                          set((f) => {
                            const hiddenRanges = [...f.camera.hiddenRanges];
                            hiddenRanges[i] = { ...hiddenRanges[i], start: ticks(Math.max(0, Math.min(v, secs(r.end) - 0.1))) };
                            return { ...f, camera: { ...f.camera, hiddenRanges } };
                          })
                        }
                      />
                      <span>→</span>
                      <SecsInput
                        value={secs(r.end)}
                        onCommit={(v) =>
                          set((f) => {
                            const hiddenRanges = [...f.camera.hiddenRanges];
                            hiddenRanges[i] = { ...hiddenRanges[i], end: ticks(Math.max(v, secs(r.start) + 0.1)) };
                            return { ...f, camera: { ...f.camera, hiddenRanges } };
                          })
                        }
                      />
                      <span className="muted">s</span>
                      <button
                        className="link-btn"
                        title={t("polish.deleteHidden")}
                        onClick={() => set((f) => ({ ...f, camera: { ...f.camera, hiddenRanges: f.camera.hiddenRanges.filter((_, j) => j !== i) } }))}
                      >
                        ✕
                      </button>
                    </li>
                  ))}
                </ul>
              </>
            )}
          </>
        )}
      </Section>

      <Section title={t("polish.zoom")}>
        {fx.zoom.autoError && (
          // R08: lúc dừng quay không tạo được zoom tự động → báo rõ, cho thử lại.
          <div className="warn small">
            {t("polish.autoZoomFailed", { detail: fx.zoom.autoError })}{" "}
            <button className="link-btn" onClick={props.onRegenerate}>
              {t("polish.regenerate")}
            </button>
          </div>
        )}
        <Select
          label={t("polish.zoomSpeed")}
          value={fx.zoom.speed}
          options={[
            { value: "slow", label: t("polish.slow") },
            { value: "mellow", label: t("polish.mellow") },
            { value: "quick", label: t("polish.quick") },
          ]}
          onChange={(speed) => set((f) => ({ ...f, zoom: { ...f.zoom, speed } }))}
        />
        <div className="row wrap">
          <button
            onClick={() =>
              props.apply({ op: "addZoom", clipId: clip.id, start: mediaAtPlayhead, end: mediaAtPlayhead + ticks(2), amount: 2 })
            }
          >
            {t("polish.addZoom")}
          </button>
          <button onClick={props.onRegenerate}>{t("polish.regenerate")}</button>
        </div>
        <ul className="zoom-list">
          {fx.zoom.segments.map((z) => (
            <li key={z.id}>
              <button className={z.id === props.selectedZoom ? "zoom-item selected" : "zoom-item"} onClick={() => props.onSelectZoom(z.id)}>
                {formatTimecode(z.start)}–{formatTimecode(z.end)} · {z.amount.toFixed(1)}× ·{" "}
                <span className="muted">{z.auto ? t("polish.auto") : t("polish.manual")}</span>
              </button>
            </li>
          ))}
        </ul>
        <span className="muted small">{t("polish.zoomCount", { n: fx.zoom.segments.length })}</span>
        {zoom && <ZoomEditor clip={clip} zoom={zoom} apply={props.apply} onDeleted={() => props.onSelectZoom(null)} />}
      </Section>
    </div>
  );
}

function ZoomEditor(props: { clip: Clip; zoom: ZoomSegment; apply: (op: EditOp) => Promise<boolean>; onDeleted: () => void }) {
  const { t } = useTranslation();
  const { clip, zoom } = props;
  const update = (patch: Partial<Pick<ZoomSegment, "amount" | "target">>) =>
    props.apply({
      op: "updateZoom",
      clipId: clip.id,
      zoomId: zoom.id,
      start: null,
      end: null,
      amount: patch.amount ?? null,
      target: patch.target ?? null,
    });
  const fixed = zoom.target.kind === "fixed" ? zoom.target : null;
  return (
    <div className="zoom-editor stack">
      <Slider label={t("polish.zoomAmount")} value={zoom.amount} min={1} max={5} step={0.1} format={(v) => `${v.toFixed(1)}×`} onCommit={(amount) => update({ amount })} />
      <Select
        label={t("polish.zoomTarget")}
        value={zoom.target.kind}
        options={[
          { value: "follow", label: t("polish.follow") },
          { value: "fixed", label: t("polish.fixed") },
        ]}
        onChange={(kind) => update({ target: kind === "follow" ? { kind: "follow" } : { kind: "fixed", x: 0.5, y: 0.5 } })}
      />
      {fixed && (
        <>
          <PointPicker
            mediaId={clip.mediaId}
            point={fixed}
            onPick={(x, y) => update({ target: { kind: "fixed", x, y } })}
          />
          <Slider label={t("polish.posX")} value={fixed.x} min={0} max={1} step={0.01} format={(v) => `${Math.round(v * 100)}%`} onCommit={(x) => update({ target: { kind: "fixed", x, y: fixed.y } })} />
          <Slider label={t("polish.posY")} value={fixed.y} min={0} max={1} step={0.01} format={(v) => `${Math.round(v * 100)}%`} onCommit={(y) => update({ target: { kind: "fixed", x: fixed.x, y } })} />
        </>
      )}
      <span className="muted small">
        {secs(zoom.end - zoom.start).toFixed(1)} s
      </span>
      <button
        className="danger"
        onClick={async () => {
          if (await props.apply({ op: "removeZoom", clipId: clip.id, zoomId: zoom.id })) props.onDeleted();
        }}
      >
        {t("polish.removeZoom")}
      </button>
    </div>
  );
}

/** E05: bấm lên ảnh bản quay để chọn điểm zoom cố định (preview là cửa sổ native nên không bấm trực tiếp được). */
function PointPicker(props: { mediaId: string | null; point: { x: number; y: number }; onPick: (x: number, y: number) => void }) {
  const { t } = useTranslation();
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    if (props.mediaId) commands.mediaThumbnail(props.mediaId).then((r) => r.status === "ok" && setSrc(r.data));
  }, [props.mediaId]);
  if (!src) return null;
  return (
    <div className="point-picker" title={t("polish.pickPoint")}>
      <img
        src={src}
        alt=""
        draggable={false}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          props.onPick(
            Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)),
            Math.min(1, Math.max(0, (e.clientY - r.top) / r.height)),
          );
        }}
      />
      <span className="point-mark" style={{ left: `${props.point.x * 100}%`, top: `${props.point.y * 100}%` }} />
      <span className="muted small">{t("polish.pickPoint")}</span>
    </div>
  );
}
