import { Fragment, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { useTranslation } from "react-i18next";
import type { Clip, EditOp, MediaItem, ProjectView, Ticks, Track } from "../bindings";
import { formatTimecode, secs, ticks } from "../lib/editor";
import Dialog from "./Dialog";

const TRACK_H = 74;
/** E05: dải zoom riêng dưới mỗi track có bản quay. */
const ZOOM_ROW_H = 26;
const HEADER_W = 156;
const RULER_H = 26;
const SNAP_PX = 8;

type Drag = {
  kind: "move" | "start" | "end";
  clip: Clip;
  x0: number;
  y0: number;
  dx: number;
  trackId: string;
};

function clipEnd(c: Clip): Ticks {
  return c.start + Math.round((c.sourceOut - c.sourceIn) / c.speed);
}

function accepts(track: Track, content: Clip["content"]["type"] | MediaItem["kind"]) {
  if (track.kind === "video") return ["recording", "video", "image"].includes(content);
  if (track.kind === "audio") return content === "audio";
  return content === "text" || content === "blur";
}

/** Mốc vạch thước theo mức zoom: giữ khoảng cách vạch ~80px. */
function rulerStep(pxPerSec: number) {
  const steps = [0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300];
  return steps.find((s) => s * pxPerSec >= 80) ?? 600;
}

export default function Timeline(props: {
  view: ProjectView;
  playhead: Ticks;
  selected: string | null;
  pxPerSec: number;
  waveforms: Record<string, number[]>;
  thumbs: Record<string, string | null>;
  onSelect: (id: string | null) => void;
  onSeek: (t: Ticks) => void;
  apply: (op: EditOp) => Promise<boolean>;
  selectedZoom: string | null;
  onSelectZoom: (clipId: string, zoomId: string) => void;
  /** X03: đoạn in/out để xuất (phím I / O). */
  inOut: { start: Ticks | null; end: Ticks | null };
}) {
  const { t } = useTranslation();
  const { view, pxPerSec } = props;
  const p = view.project;
  const scroller = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragRef = useRef<Drag | null>(null);
  dragRef.current = drag;

  // Track trên cùng của danh sách hiển thị = lớp trên cùng (cuối mảng tracks).
  const rows = useMemo(() => [...p.tracks].reverse(), [p.tracks]);
  const widthSecs = Math.max(secs(view.duration) + 15, 60);
  const contentW = widthSecs * pxPerSec;
  const x = (tk: Ticks) => secs(tk) * pxPerSec;
  const tAtX = (px: number) => ticks(Math.max(0, px / pxPerSec));

  // Các mốc để bắt dính: mép mọi clip (trừ clip đang kéo), đầu phát, 0.
  const snapPoints = (exclude: string) => [
    0,
    props.playhead,
    ...p.clips.filter((c) => c.id !== exclude).flatMap((c) => [c.start, clipEnd(c)]),
  ];
  const snap = (tk: Ticks, exclude: string) => {
    let best = tk;
    let bestPx = SNAP_PX + 1;
    for (const s of snapPoints(exclude)) {
      const d = Math.abs(x(s) - x(tk));
      if (d < bestPx) {
        bestPx = d;
        best = s;
      }
    }
    return best;
  };

  /** Vị trí "bóng" của clip đang kéo (để vẽ và để gửi lệnh). */
  function ghost(d: Drag) {
    const dt = ticks(d.dx / pxPerSec);
    if (d.kind === "move") {
      const len = clipEnd(d.clip) - d.clip.start;
      let start = Math.max(0, d.clip.start + dt);
      const snappedStart = snap(start, d.clip.id);
      const snappedEnd = snap(start + len, d.clip.id);
      if (snappedStart !== start) start = snappedStart;
      else if (snappedEnd !== start + len) start = snappedEnd - len;
      return { start: Math.max(0, start), end: Math.max(0, start) + len };
    }
    if (d.kind === "start") return { start: snap(d.clip.start + dt, d.clip.id), end: clipEnd(d.clip) };
    return { start: d.clip.start, end: snap(clipEnd(d.clip) + dt, d.clip.id) };
  }

  function overlaps(trackId: string, start: Ticks, end: Ticks, exclude: string) {
    return p.clips.some((c) => c.trackId === trackId && c.id !== exclude && c.start < end && start < clipEnd(c));
  }

  function beginDrag(e: ReactMouseEvent, clip: Clip, kind: Drag["kind"]) {
    e.stopPropagation();
    if (e.button !== 0) return;
    props.onSelect(clip.id);
    const track = p.tracks.find((tr) => tr.id === clip.trackId);
    if (track?.locked) return;
    const d: Drag = { kind, clip, x0: e.clientX, y0: e.clientY, dx: 0, trackId: clip.trackId };
    setDrag(d);
    const move = (ev: MouseEvent) => {
      const cur = dragRef.current;
      if (!cur) return;
      let trackId = cur.trackId;
      if (cur.kind === "move") {
        // Track dưới con trỏ (giữa các track có thể có dải zoom, nên không chia theo chiều cao).
        const lane = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>(".tl-lane[data-track-id]");
        const target = lane ? rows.find((r) => r.id === lane.dataset.trackId) : undefined;
        if (target && accepts(target, cur.clip.content.type) && !target.locked) trackId = target.id;
      }
      setDrag({ ...cur, dx: ev.clientX - cur.x0, trackId });
    };
    const up = async () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const cur = dragRef.current;
      setDrag(null);
      if (!cur || (Math.abs(cur.dx) < 2 && cur.trackId === cur.clip.trackId)) return;
      const g = ghost(cur);
      if (cur.kind === "move") {
        await props.apply({
          op: "moveClips",
          clipIds: [cur.clip.id],
          delta: g.start - cur.clip.start,
          trackId: cur.trackId !== cur.clip.trackId ? cur.trackId : null,
        });
      } else {
        await props.apply({
          op: "trimClip",
          clipId: cur.clip.id,
          edge: cur.kind,
          to: cur.kind === "start" ? g.start : g.end,
        });
      }
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  /** E05: kéo khối zoom (dời / đổi mép). Zoom lưu theo thời gian trong media của clip. */
  function dragZoom(e: ReactMouseEvent, clip: Clip, zoomId: string, kind: "move" | "start" | "end") {
    e.stopPropagation();
    if (e.button !== 0) return;
    props.onSelect(clip.id);
    props.onSelectZoom(clip.id, zoomId);
    if (clip.content.type !== "recording" || !clip.content.fx) return;
    const z = clip.content.fx.zoom.segments.find((x) => x.id === zoomId);
    if (!z) return;
    const x0 = e.clientX;
    const el = e.currentTarget as HTMLElement;
    const block = kind === "move" ? el : (el.parentElement as HTMLElement);
    const left0 = block.offsetLeft;
    const width0 = block.offsetWidth;
    let dx = 0;
    const move = (ev: MouseEvent) => {
      dx = ev.clientX - x0;
      if (kind === "move") block.style.left = `${left0 + dx}px`;
      else if (kind === "start") {
        block.style.left = `${left0 + dx}px`;
        block.style.width = `${Math.max(4, width0 - dx)}px`;
      } else block.style.width = `${Math.max(4, width0 + dx)}px`;
    };
    const up = async () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      if (Math.abs(dx) < 2) return;
      // Đổi pixel → thời gian trong media (nhân tốc độ clip).
      const dMedia = Math.round(ticks(dx / pxPerSec) * clip.speed);
      await props.apply({
        op: "updateZoom",
        clipId: clip.id,
        zoomId,
        start: kind === "end" ? null : z.start + dMedia,
        end: kind === "start" ? null : z.end + dMedia,
        amount: null,
        target: null,
      });
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  function scrub(e: ReactMouseEvent) {
    const el = scroller.current!;
    const left = el.getBoundingClientRect().left + HEADER_W;
    const seek = (cx: number) => props.onSeek(tAtX(cx - left + el.scrollLeft));
    seek(e.clientX);
    const move = (ev: MouseEvent) => seek(ev.clientX);
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  const step = rulerStep(pxPerSec);
  const marks: number[] = [];
  for (let s = 0; s <= widthSecs; s += step) marks.push(s);

  return (
    <div className="timeline" ref={scroller}>
      <div className="tl-inner" style={{ width: HEADER_W + contentW }}>
        <div className="tl-ruler" style={{ height: RULER_H }}>
          <div className="tl-corner" style={{ width: HEADER_W }} />
          <div className="tl-ruler-lane" style={{ width: contentW }} onMouseDown={scrub}>
            {props.inOut.start !== null && props.inOut.end !== null && props.inOut.end > props.inOut.start && (
              <div
                className="tl-inout"
                style={{ left: x(props.inOut.start), width: x(props.inOut.end) - x(props.inOut.start) }}
                title={t("export.rangeTitle")}
              />
            )}
            {props.inOut.start !== null && <div className="tl-inout-mark in" style={{ left: x(props.inOut.start) }} />}
            {props.inOut.end !== null && <div className="tl-inout-mark out" style={{ left: x(props.inOut.end) }} />}
            {marks.map((s) => (
              <span key={s} className="tl-mark" style={{ left: s * pxPerSec }}>
                {formatTimecode(ticks(s)).replace(/\.00$/, "")}
              </span>
            ))}
          </div>
        </div>

        {rows.map((track) => {
          const clips = p.clips.filter((c) => c.trackId === track.id || (drag?.clip.id === c.id && drag.trackId === track.id));
          const recClips = p.clips.filter((c) => c.trackId === track.id && c.content.type === "recording");
          return (
            <Fragment key={track.id}>
            <div className={track.locked ? "tl-row locked" : "tl-row"} style={{ height: TRACK_H }}>
              <TrackHeader track={track} clips={p.clips.filter((c) => c.trackId === track.id).length} apply={props.apply} />
              <div
                className="tl-lane"
                style={{ width: contentW }}
                onMouseDown={(e) => {
                  if (e.target === e.currentTarget) props.onSelect(null);
                }}
                data-track-id={track.id}
              >
                {clips.map((c) => {
                  const dragging = drag?.clip.id === c.id;
                  if (dragging && drag!.trackId !== track.id) return null;
                  const g = dragging ? ghost(drag!) : { start: c.start, end: clipEnd(c) };
                  const bad = dragging && overlaps(drag!.trackId, g.start, g.end, c.id);
                  const media = p.media.find((m) => m.id === c.mediaId);
                  const cls = [
                    "tl-clip",
                    `kind-${c.content.type}`,
                    props.selected === c.id ? "selected" : "",
                    dragging ? "dragging" : "",
                    bad ? "bad" : "",
                    media && view.missing.includes(media.id) ? "missing" : "",
                  ].join(" ");
                  return (
                    <div
                      key={c.id}
                      className={cls}
                      style={{ left: x(g.start), width: Math.max(4, x(g.end) - x(g.start)) }}
                      onMouseDown={(e) => beginDrag(e, c, "move")}
                      title={media?.name}
                    >
                      {media && props.thumbs[media.id] && c.content.type !== "audio" && (
                        <img className="tl-thumb" src={props.thumbs[media.id]!} alt="" draggable={false} />
                      )}
                      <span className="tl-clip-name">
                        {c.content.type === "text"
                          ? `T  ${c.content.text}`
                          : c.content.type === "blur"
                            ? `▦ ${c.content.pixelate ? "pixelate" : "blur"}`
                            : media?.name}
                        {c.speed !== 1 ? ` · ${c.speed}×` : ""}
                        {media && view.missing.includes(media.id) ? ` — ⚠ ${t("editor.missing")}` : ""}
                      </span>
                      {c.content.type === "recording" && c.content.fx?.camera.hiddenRanges.length && !dragging ? (
                        // E08: các đoạn ẩn webcam.
                        <div className="tl-camhide-band">
                          {c.content.fx.camera.hiddenRanges.map((r, i) => {
                            const a = Math.max(c.start + (r.start - c.sourceIn) / c.speed, c.start);
                            const b = Math.min(c.start + (r.end - c.sourceIn) / c.speed, clipEnd(c));
                            if (b <= a) return null;
                            return (
                              <div
                                key={i}
                                className="tl-camhide"
                                style={{ left: x(a) - x(c.start), width: Math.max(3, x(b) - x(a)) }}
                                title={t("editor.cameraHiddenRange", { a: formatTimecode(r.start), b: formatTimecode(r.end) })}
                              >
                                <span className="tl-lost-label">◌ {t("device.camera")}</span>
                              </div>
                            );
                          })}
                        </div>
                      ) : null}
                      {c.content.type === "recording" && media?.recording?.lost?.length && !dragging ? (
                        // R07: khoảng mất webcam / mic / âm thanh hệ thống khi đang quay.
                        <div className="tl-lost-band">
                          {(media.recording.lost ?? []).map((l, i) => {
                            const a = Math.max(c.start + (ticks(l.start) - c.sourceIn) / c.speed, c.start);
                            const b = Math.min(c.start + (ticks(l.end) - c.sourceIn) / c.speed, clipEnd(c));
                            if (b <= a) return null;
                            const label = t("editor.lostRange", {
                              source: t(`device.${l.source}`),
                              a: formatTimecode(ticks(l.start)),
                              b: formatTimecode(ticks(l.end)),
                            });
                            return (
                              <div
                                key={i}
                                className="tl-lost"
                                style={{ left: x(a) - x(c.start), width: Math.max(3, x(b) - x(a)) }}
                                title={label}
                              >
                                <span className="tl-lost-label">⚠ {t(`device.${l.source}`)}</span>
                              </div>
                            );
                          })}
                        </div>
                      ) : null}
                      {media && props.waveforms[media.id]?.length ? (
                        <Waveform peaks={props.waveforms[media.id]} clip={c} width={x(g.end) - x(g.start)} />
                      ) : null}
                      <span className="tl-handle start" onMouseDown={(e) => beginDrag(e, c, "start")} />
                      <span className="tl-handle end" onMouseDown={(e) => beginDrag(e, c, "end")} />
                    </div>
                  );
                })}
              </div>
            </div>
            {track.kind === "video" && recClips.length > 0 && (
              // E05: khối zoom nằm trên dải riêng (theo thời gian timeline), kéo / đổi mép được.
              <div className="tl-row tl-zoom-row" style={{ height: ZOOM_ROW_H }}>
                <div className="tl-header tl-zoom-header" style={{ width: HEADER_W }}>
                  🔍 {t("editor.zoomLane")}
                </div>
                <div className="tl-lane tl-zoom-lane" style={{ width: contentW }}>
                  {recClips.flatMap((c) =>
                    c.content.type === "recording" && c.content.fx
                      ? c.content.fx.zoom.segments.map((z) => {
                          const a = Math.max(c.start + (z.start - c.sourceIn) / c.speed, c.start);
                          const b = Math.min(c.start + (z.end - c.sourceIn) / c.speed, clipEnd(c));
                          if (b <= a) return null;
                          return (
                            <div
                              key={z.id}
                              className={props.selectedZoom === z.id ? "tl-zoom selected" : z.auto ? "tl-zoom auto" : "tl-zoom"}
                              style={{ left: x(a), width: Math.max(4, x(b) - x(a)) }}
                              onMouseDown={(e) => dragZoom(e, c, z.id, "move")}
                              title={`${z.amount.toFixed(1)}× · ${z.target.kind === "follow" ? t("polish.follow") : t("polish.fixed")}`}
                            >
                              <span className="tl-zoom-label">{z.amount.toFixed(1)}×</span>
                              <span className="tl-handle start" onMouseDown={(e) => dragZoom(e, c, z.id, "start")} />
                              <span className="tl-handle end" onMouseDown={(e) => dragZoom(e, c, z.id, "end")} />
                            </div>
                          );
                        })
                      : [],
                  )}
                </div>
              </div>
            )}
            </Fragment>
          );
        })}

        <div className="tl-playhead" style={{ left: HEADER_W + x(props.playhead) }} />
      </div>
      <div className="tl-footer muted small">
        {t("editor.zoom")}: {pxPerSec.toFixed(0)} px/s
      </div>
    </div>
  );
}

function TrackHeader({ track, clips, apply }: { track: Track; clips: number; apply: (op: EditOp) => Promise<boolean> }) {
  const { t } = useTranslation();
  // Xóa track: track còn clip → hỏi trước (xóa luôn các clip, một bước hoàn tác).
  const [confirmRemove, setConfirmRemove] = useState(false);
  const remove = () => apply({ op: "removeTrack", trackId: track.id });
  const set = (patch: Partial<Pick<Track, "locked" | "muted" | "hidden" | "volumeDb">>) =>
    apply({
      op: "setTrack",
      trackId: track.id,
      name: null,
      locked: patch.locked ?? null,
      muted: patch.muted ?? null,
      hidden: patch.hidden ?? null,
      volumeDb: patch.volumeDb ?? null,
    });
  // E11: kéo thì chỉ đổi hiển thị, thả chuột mới ghi (một bước undo).
  const [vol, setVol] = useState<number | null>(null);
  const shown = vol ?? track.volumeDb ?? 0;
  return (
    <div className="tl-header" style={{ width: HEADER_W }}>
      <span className="tl-track-name" title={track.name}>
        {track.kind === "audio" ? "♪" : "▣"} {track.name}
      </span>
      <span className="tl-track-btns">
        <button
          className={track.locked ? "tb on" : "tb"}
          title={track.locked ? t("editor.unlock") : t("editor.lock")}
          onClick={() => set({ locked: !track.locked })}
        >
          {track.locked ? "🔒" : "🔓"}
        </button>
        <button
          className={track.muted ? "tb on" : "tb"}
          title={track.muted ? t("editor.unmute") : t("editor.mute")}
          onClick={() => set({ muted: !track.muted })}
        >
          {track.muted ? "🔇" : "🔊"}
        </button>
        {track.kind !== "audio" && (
          <button
            className={track.hidden ? "tb on" : "tb"}
            title={track.hidden ? t("editor.show") : t("editor.hide")}
            onClick={() => set({ hidden: !track.hidden })}
          >
            {track.hidden ? "◌" : "◉"}
          </button>
        )}
        <button
          className="tb"
          title={track.locked ? t("editor.removeTrackLocked") : t("editor.removeTrack")}
          disabled={track.locked}
          onClick={() => (clips > 0 ? setConfirmRemove(true) : remove())}
        >
          ✕
        </button>
      </span>
      {confirmRemove && (
        <Dialog
          title={t("editor.removeTrackTitle", { name: track.name })}
          onClose={() => setConfirmRemove(false)}
          actions={
            <>
              <button onClick={() => setConfirmRemove(false)}>{t("common.cancel")}</button>
              <button
                className="danger"
                onClick={() => {
                  setConfirmRemove(false);
                  remove();
                }}
              >
                {t("editor.removeTrackConfirm")}
              </button>
            </>
          }
        >
          <p>{t("editor.removeTrackBody", { count: clips })}</p>
        </Dialog>
      )}
      {track.kind !== "overlay" && (
        <label className="tl-track-vol" title={t("editor.trackVolume", { db: shown.toFixed(0) })}>
          <input
            type="range"
            min={-60}
            max={12}
            step={1}
            value={shown}
            aria-label={t("editor.trackVolume", { db: shown.toFixed(0) })}
            onChange={(e) => setVol(Number(e.target.value))}
            onPointerUp={() => {
              if (vol !== null && vol !== (track.volumeDb ?? 0)) set({ volumeDb: vol });
              setVol(null);
            }}
            onKeyUp={() => {
              if (vol !== null) set({ volumeDb: vol });
              setVol(null);
            }}
          />
          <span>{shown > 0 ? `+${shown.toFixed(0)}` : shown.toFixed(0)} dB</span>
        </label>
      )}
    </div>
  );
}

/** Waveform của đoạn media mà clip đang dùng (đỉnh mỗi 100 ms). */
function Waveform({ peaks, clip, width }: { peaks: number[]; clip: Clip; width: number }) {
  const from = Math.floor(secs(clip.sourceIn) * 10);
  const to = Math.min(peaks.length, Math.ceil(secs(clip.sourceOut) * 10));
  const slice = peaks.slice(from, Math.max(from + 1, to));
  if (slice.length === 0 || width < 8) return null;
  const h = 26;
  const pts = slice.map((v, i) => {
    const px = (i / Math.max(1, slice.length - 1)) * width;
    const amp = Math.min(1, v * 3) * (h / 2);
    return [px, amp] as const;
  });
  const top = pts.map(([px, a]) => `${px.toFixed(1)},${(h / 2 - a).toFixed(1)}`).join(" ");
  const bottom = [...pts].reverse().map(([px, a]) => `${px.toFixed(1)},${(h / 2 + a).toFixed(1)}`).join(" ");
  return (
    <svg className="tl-wave" width={width} height={h} viewBox={`0 0 ${width} ${h}`} preserveAspectRatio="none">
      <polygon points={`${top} ${bottom}`} />
    </svg>
  );
}
