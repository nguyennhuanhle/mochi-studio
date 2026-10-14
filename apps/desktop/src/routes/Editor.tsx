import { useCallback, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  commands,
  events,
  type Duplicate,
  type EditOp,
  type EditorError,
  type MediaItem,
  type ProjectView,
  type Rejected,
  type Ticks,
} from "../bindings";
import Dialog from "../components/Dialog";
import Inspector from "../components/Inspector";
import MediaBin from "../components/MediaBin";
import PreviewPane from "../components/PreviewPane";
import Timeline from "../components/Timeline";
import { editorErrorMessage, formatTimecode, TICKS_PER_SECOND } from "../lib/editor";
import ExportDialog from "../components/ExportDialog";
import MarkersDialog from "../components/MarkersDialog";
import BrandDialog from "../components/BrandDialog";
import { brandFont } from "../lib/brand";
import SaveFailedDialog from "../components/SaveFailedDialog";
import RecoveredDialog from "../components/RecoveredDialog";

/** Project đã hỏi "khôi phục sau crash" trong phiên này. */
const recoveryPrompted = new Set<string>();

/** Trình biên tập (E01–E04): media bin · preview native · thuộc tính · timeline nhiều track. */
export default function Editor() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [view, setView] = useState<ProjectView | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [selectedZoom, setSelectedZoom] = useState<string | null>(null);
  const [showRecovered, setShowRecovered] = useState(false);
  const [playhead, setPlayhead] = useState<Ticks>(0);
  const [playing, setPlaying] = useState(false);
  const [fps, setFps] = useState(0);
  /** S03: việc nền của media bị hoãn (đang quay) / lỗi. */
  const [deferred, setDeferred] = useState<string[]>([]);
  const [taskErrors, setTaskErrors] = useState<Record<string, string>>({});
  const [retryTick, setRetryTick] = useState(0);
  /** S03: preview dùng proxy (khi có). */
  const [useProxies, setUseProxies] = useState(true);
  useEffect(() => {
    commands.settingsGet().then((s) => setUseProxies(s.useProxies));
  }, []);
  /** Chưa nhận frame nào từ trình phát (renderer của Cap mở lần đầu mất vài giây). */
  const [previewReady, setPreviewReady] = useState(false);
  const [pxPerSec, setPxPerSec] = useState(60);
  const [waveforms, setWaveforms] = useState<Record<string, number[]>>({});
  const [thumbs, setThumbs] = useState<Record<string, string | null>>({});
  const [toast, setToast] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [saveState, setSaveState] = useState<{ ok: boolean; at: string } | null>(null);
  const [rejected, setRejected] = useState<Rejected[]>([]);
  const [duplicates, setDuplicates] = useState<Duplicate[]>([]);
  const [removing, setRemoving] = useState<{ media: MediaItem; clips: number } | null>(null);
  const [mediaDrag, setMediaDrag] = useState<{ media: MediaItem; x: number; y: number } | null>(null);
  const [inOut, setInOut] = useState<{ start: Ticks | null; end: Ticks | null }>({ start: null, end: null });
  const [exporting, setExporting] = useState(false);
  const [showMarkers, setShowMarkers] = useState(false);
  const [showBrand, setShowBrand] = useState(false);
  const [packing, setPacking] = useState<{ done: number; total: number } | null>(null);
  const [closeError, setCloseError] = useState<EditorError | null>(null);
  const viewRef = useRef(view);
  viewRef.current = view;
  const playheadRef = useRef(playhead);
  playheadRef.current = playhead;

  const showError = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast((m) => (m === msg ? null : m)), 4000);
  }, []);
  const showToast = useCallback((msg: string) => {
    setInfo(msg);
    window.setTimeout(() => setInfo((m) => (m === msg ? null : m)), 4000);
  }, []);

  useEffect(() => {
    commands.projectView().then((v) => {
      if (!v) return navigate("/");
      setView(v);
      // Hỏi khôi phục một lần cho mỗi project trong phiên (mở lại trang editor không hỏi lại).
      if (v.recovered && !recoveryPrompted.has(v.dir)) {
        recoveryPrompted.add(v.dir);
        setShowRecovered(true);
      }
    });
    const subs = [
      events.playerTime.listen(({ payload }) => {
        setPreviewReady(true);
        setPlayhead(payload.t);
        setPlaying(payload.playing);
      }),
      events.playerStats.listen(({ payload }) => setFps(payload.fps)),
      events.autosaveStatus.listen(({ payload }) => {
        setSaveState({ ok: payload.ok, at: payload.at });
        if (payload.ok) commands.projectView().then((v) => v && setView(v));
      }),
    ];
    return () => subs.forEach((s) => s.then((f) => f()));
  }, [navigate]);

  // Bản dev: cho bài tự chụp (devSnapshot) chọn sẵn clip / vùng zoom.
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    (window as unknown as { __mochiSelect?: unknown }).__mochiSelect = (clip: string | null, zoom: string | null) => {
      setSelected(clip);
      setSelectedZoom(zoom);
    };
  }, []);

  // Waveform + ảnh thu nhỏ cho media mới (S03). Đang quay → hoãn, hỏi lại khi quay xong;
  // lỗi → đánh dấu trên media, có nút thử lại (không nuốt im lặng).
  const loadWaveform = useCallback((id: string) => {
    setWaveforms((w) => ({ ...w, [id]: [] }));
    commands.mediaWaveform(id).then((r) => {
      if (r.status === "ok") setWaveforms((w) => ({ ...w, [id]: r.data }));
      else if (r.error.code === "deferred") setDeferred((d) => [...new Set([...d, id])]);
      else setTaskErrors((e) => ({ ...e, [id]: r.error.code === "failed" ? r.error.message : "" }));
    });
  }, []);
  const loadThumb = useCallback((id: string) => {
    setThumbs((x) => ({ ...x, [id]: null }));
    commands.mediaThumbnail(id).then((r) => {
      if (r.status === "ok") setThumbs((x) => ({ ...x, [id]: r.data }));
      else if (r.error.code === "deferred") setDeferred((d) => [...new Set([...d, id])]);
      else setTaskErrors((e) => ({ ...e, [id]: r.error.code === "failed" ? r.error.message : "" }));
    });
  }, []);
  useEffect(() => {
    if (!view) return;
    for (const m of view.project.media) {
      if (view.missing.includes(m.id)) continue;
      if (!(m.id in waveforms) && m.kind !== "image") loadWaveform(m.id);
      if (!(m.id in thumbs) && m.kind !== "audio") loadThumb(m.id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view?.project.media, view?.missing]);
  // Quay xong (về Idle) → làm nốt các việc đã hoãn.
  useEffect(() => {
    const un = events.recorderEventMsg.listen(({ payload }) => {
      if (payload.type === "stateChanged" && payload.state === "idle") setRetryTick((n) => n + 1);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);
  useEffect(() => {
    if (retryTick === 0 || deferred.length === 0 || !view) return;
    const ids = deferred;
    setDeferred([]);
    for (const id of ids) {
      const m = view.project.media.find((x) => x.id === id);
      if (!m) continue;
      if (m.kind !== "image") loadWaveform(id);
      if (m.kind !== "audio") loadThumb(id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [retryTick]);
  function retryTasks(id: string) {
    setTaskErrors((e) => {
      const { [id]: _, ...rest } = e;
      return rest;
    });
    const m = view?.project.media.find((x) => x.id === id);
    if (m && m.kind !== "image") loadWaveform(id);
    if (m && m.kind !== "audio") loadThumb(id);
  }

  const apply = useCallback(
    async (op: EditOp) => {
      const r = await commands.timelineApply(op);
      if (r.status === "ok") {
        setView(r.data);
        return true;
      }
      showError(editorErrorMessage(t, r.error));
      return false;
    },
    [showError, t],
  );

  const importPaths = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return;
      const r = await commands.mediaImport(paths);
      if (r.status === "error") return showError(editorErrorMessage(t, r.error));
      setView(r.data.view);
      setRejected(r.data.report.rejected);
      setDuplicates(r.data.report.duplicates);
    },
    [showError, t],
  );

  // Kéo thả file từ Explorer vào cửa sổ → nhập media.
  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "drop") importPaths(e.payload.paths);
    });
    return () => {
      un.then((f) => f());
    };
  }, [importPaths]);
  // Bản dev: bài tự chụp import qua đúng đường của UI (để effect ảnh thu nhỏ / waveform chạy).
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    (window as unknown as { __mochiImport?: unknown }).__mochiImport = importPaths;
  }, [importPaths]);

  const undo = async () => {
    const r = await commands.timelineUndo();
    if (r.status === "ok") setView(r.data);
  };
  const redo = async () => {
    const r = await commands.timelineRedo();
    if (r.status === "ok") setView(r.data);
  };
  const save = async () => {
    const r = await commands.projectSave();
    if (r.status === "ok") {
      setView(r.data);
      setSaveState({ ok: true, at: new Date().toLocaleTimeString() });
    } else showError(editorErrorMessage(t, r.error));
  };

  /** Clip để cắt/xóa: clip đang chọn, hoặc clip dưới đầu phát trên track trên cùng. */
  const targetClip = () => {
    const v = viewRef.current;
    if (!v) return null;
    if (selected) return v.project.clips.find((c) => c.id === selected) ?? null;
    const ph = playheadRef.current;
    const tracks = [...v.project.tracks].reverse();
    for (const tr of tracks) {
      const c = v.project.clips.find(
        (c) => c.trackId === tr.id && c.start <= ph && ph < c.start + (c.sourceOut - c.sourceIn) / c.speed,
      );
      if (c) return c;
    }
    return null;
  };

  // Phím tắt.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;
      const ctrl = e.ctrlKey || e.metaKey;
      if (e.code === "Space") {
        e.preventDefault();
        commands.playerToggle();
      } else if (ctrl && e.key.toLowerCase() === "z") {
        e.preventDefault();
        (e.shiftKey ? redo : undo)();
      } else if (ctrl && e.key.toLowerCase() === "y") {
        e.preventDefault();
        redo();
      } else if (ctrl && e.key.toLowerCase() === "s") {
        e.preventDefault();
        save();
      } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        commands.playerStep((e.key === "ArrowLeft" ? -1 : 1) * (e.shiftKey ? 10 : 1));
      } else if (e.key === "Home") {
        commands.playerSeek(0);
      } else if (e.key.toLowerCase() === "s" && !ctrl) {
        const c = targetClip();
        if (c) apply({ op: "splitClip", clipId: c.id, at: playheadRef.current });
      } else if (e.key === "Delete" || e.key === "Backspace") {
        const c = selected;
        if (c) {
          apply({ op: "deleteClips", clipIds: [c], ripple: e.shiftKey });
          setSelected(null);
        }
      } else if (ctrl && e.key.toLowerCase() === "e") {
        e.preventDefault();
        setExporting(true);
      } else if (e.key.toLowerCase() === "i" && !ctrl) {
        const at = playheadRef.current;
        setInOut((r) => ({ start: at, end: r.end !== null && r.end > at ? r.end : null }));
      } else if (e.key.toLowerCase() === "o" && !ctrl) {
        const at = playheadRef.current;
        setInOut((r) => ({ start: r.start !== null && r.start < at ? r.start : null, end: at }));
      } else if (e.key.toLowerCase() === "x" && !ctrl) {
        setInOut({ start: null, end: null });
      } else if (e.key.toLowerCase() === "m" && !ctrl) {
        // E14: thêm marker tại đầu phát.
        const n = (viewRef.current?.project.markers?.length ?? 0) + 1;
        apply({ op: "addMarker", at: playheadRef.current, label: `Marker ${n}` });
      } else if (e.key === "+" || e.key === "=") {
        setPxPerSec((z) => Math.min(800, z * 1.25));
      } else if (e.key === "-") {
        setPxPerSec((z) => Math.max(4, z / 1.25));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected, apply]);

  async function removeMedia(m: MediaItem) {
    const r = await commands.mediaRemove(m.id, false);
    if (r.status === "ok") return setView(r.data);
    if (r.error.code === "inUse") setRemoving({ media: m, clips: r.error.clipIds.length });
    else showError(editorErrorMessage(t, r.error));
  }

  async function relink(m: MediaItem) {
    // Bản quay là một thư mục → chọn thư mục; media thường → chọn file.
    const path = m.kind === "recording" ? await commands.pickFolder() : (await commands.pickMediaFiles())[0];
    if (!path) return;
    const r = await commands.mediaRelink(m.id, path);
    if (r.status === "ok") setView(r.data);
    else showError(editorErrorMessage(t, r.error));
  }

  async function relinkFolder() {
    const folder = await commands.pickFolder();
    if (!folder) return;
    const r = await commands.mediaRelinkFolder(folder);
    const v = await commands.projectView();
    if (v) setView(v);
    if (r.status === "ok" && v) {
      // Báo kết quả, kể cả những media vẫn còn thiếu (không bỏ qua im lặng).
      const names = r.data.stillMissing.map((id) => v.project.media.find((m) => m.id === id)?.name ?? id);
      showError(
        names.length
          ? t("editor.relinkPartial", { fixed: r.data.fixed.length, names: names.join(", ") })
          : t("editor.relinkDone", { fixed: r.data.fixed.length }),
      );
    }
  }

  /** Kéo media từ media bin thả lên một track: tìm lane dưới con trỏ (data-track-id). */
  function startMediaDrag(media: MediaItem, e: ReactMouseEvent) {
    e.preventDefault();
    setMediaDrag({ media, x: e.clientX, y: e.clientY });
    const move = (ev: MouseEvent) => setMediaDrag({ media, x: ev.clientX, y: ev.clientY });
    const up = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      setMediaDrag(null);
      const lane = (document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null)?.closest<HTMLElement>(
        ".tl-lane",
      );
      if (!lane) return;
      const rect = lane.getBoundingClientRect();
      const start = Math.max(0, Math.round(((ev.clientX - rect.left) / pxPerSec) * TICKS_PER_SECOND));
      const trackId = lane.dataset.trackId ?? null;
      const track = viewRef.current?.project.tracks.find((tr) => tr.id === trackId);
      const fits =
        track &&
        !track.locked &&
        (track.kind === "audio" ? media.kind === "audio" : track.kind === "video" && media.kind !== "audio");
      apply({ op: "addClip", mediaId: media.id, trackId: fits ? trackId : null, start });
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  if (!view) return <div className="page muted">…</div>;
  const duration = view.duration;

  return (
    <div className="editor">
      <header className="editor-top">
        <button
          className="icon-link"
          onClick={async () => {
            const r = await commands.projectClose(false);
            if (r.status === "error") setCloseError(r.error);
            else navigate("/");
          }}
          aria-label={t("common.back")}
        >
          ←
        </button>
        <strong className="ellipsis">{view.project.name}</strong>
        <span className={saveState && !saveState.ok ? "save-state bad" : "save-state"}>
          {saveState && !saveState.ok
            ? t("editor.autosaveFailed")
            : view.dirty
              ? `● ${t("editor.unsaved")}`
              : saveState
                ? t("editor.saved", { at: saveState.at })
                : ""}
        </span>
        <span style={{ flex: 1 }} />
        <button disabled={!view.canUndo} onClick={undo} title={t("editor.undo")}>
          ↶
        </button>
        <button disabled={!view.canRedo} onClick={redo} title={t("editor.redo")}>
          ↷
        </button>
        <button onClick={save}>{t("editor.save")}</button>
        <button
          disabled={packing !== null}
          title={t("pack.hint")}
          onClick={async () => {
            await save();
            setPacking({ done: 0, total: 0 });
            const un = await events.packProgress.listen(({ payload }) => setPacking(payload));
            const r = await commands.projectPack();
            un();
            setPacking(null);
            if (r.status === "error") showError(t("pack.failed", { detail: r.error }));
            else if (r.data)
              showToast(
                t("pack.done", { path: r.data.path, mb: (r.data.bytes / 1e6).toFixed(0) }) +
                  (r.data.missing.length ? ` ${t("pack.missing", { names: r.data.missing.join(", ") })}` : ""),
              );
          }}
        >
          {packing ? t("pack.progress", { pct: packing.total ? Math.round((packing.done / packing.total) * 100) : 0 }) : `📦 ${t("pack.button")}`}
        </button>
        <button className="primary" onClick={() => setExporting(true)} title="Ctrl+E">
          {t("export.button")}
        </button>
      </header>

      <div className="editor-main">
        <MediaBin
          view={view}
          onImport={async () => importPaths(await commands.pickMediaFiles())}
          onRemove={removeMedia}
          onRelink={relink}
          onRelinkFolder={relinkFolder}
          onDragStart={startMediaDrag}
          thumbs={thumbs}
          taskErrors={taskErrors}
          onRetryTasks={retryTasks}
        />
        <section className="preview-col">
          <PreviewPane />
          <div className="transport">
            <button onClick={() => commands.playerStep(-1)} title={t("editor.stepBack")}>
              ⏮
            </button>
            <button className="primary" onClick={() => commands.playerToggle()} title={playing ? t("editor.pause") : t("editor.play")}>
              {playing ? "❚❚" : "▶"}
            </button>
            <button onClick={() => commands.playerStep(1)} title={t("editor.stepFwd")}>
              ⏭
            </button>
            <span className="timecode">
              {formatTimecode(playhead)} / {formatTimecode(duration)}
            </span>
            <button
              title={t("editor.saveFrame")}
              onClick={async () => {
                const name = `${view.project.name}-${formatTimecode(playhead).replace(/[:.]/g, "-")}.png`;
                const r = await commands.playerSaveFrame(name);
                if (r.status === "error") showError(r.error);
                else if (r.data) showToast(t("editor.frameSaved", { path: r.data }));
              }}
            >
              🖼
            </button>
            <span className="muted small">
              {/* Bộ đếm fps chỉ ở bản dev (số đo cho người phát triển). */}
              {!previewReady ? t("polish.preparing") : import.meta.env.DEV && playing && fps > 0 ? t("editor.fps", { fps: fps.toFixed(0) }) : ""}
            </span>
            <span style={{ flex: 1 }} />
            <label className="check small" title={t("editor.useProxiesHint")}>
              <input
                type="checkbox"
                checked={useProxies}
                onChange={async (e) => {
                  const on = e.target.checked;
                  setUseProxies(on);
                  await commands.previewUseProxies(on);
                }}
              />
              <span>{t("editor.useProxies")}</span>
            </label>
            <span className="muted small">
              {view.project.canvas.width}×{view.project.canvas.height} · {view.project.canvas.fps} fps
            </span>
          </div>
        </section>
        <Inspector
          view={view}
          clipId={selected}
          apply={apply}
          playhead={playhead}
          selectedZoom={selectedZoom}
          onSelectZoom={setSelectedZoom}
          onRegenerateZoom={async (clipId) => {
            const r = await commands.timelineRegenerateZoom(clipId);
            if (r.status === "ok") setView(r.data);
            else showError(editorErrorMessage(t, r.error));
          }}
        />
      </div>

      <div className="editor-toolbar">
        <button
          onClick={() => {
            const c = targetClip();
            if (c) apply({ op: "splitClip", clipId: c.id, at: playhead });
          }}
          title={t("editor.split")}
        >
          ✂
        </button>
        <button disabled={!selected} onClick={() => selected && apply({ op: "deleteClips", clipIds: [selected], ripple: false })} title={t("editor.delete")}>
          🗑
        </button>
        <button disabled={!selected} onClick={() => selected && apply({ op: "deleteClips", clipIds: [selected], ripple: true })} title={t("editor.rippleDelete")}>
          ⇤🗑
        </button>
        <span className="sep" />
        <button
          onClick={() =>
            apply({
              op: "addOverlay",
              content: { type: "text", text: t("polish.defaultText"), font: brandFont(), sizePx: 56, color: "#FFFFFF", background: "#1D1B26CC" },
              start: playhead,
              duration: 3 * TICKS_PER_SECOND,
              trackId: null,
            })
          }
        >
          {t("polish.addText")}
        </button>
        <button
          onClick={() =>
            apply({
              op: "addOverlay",
              content: { type: "blur", x: 0.35, y: 0.35, width: 0.3, height: 0.3, pixelate: true, strength: 0.6 },
              start: playhead,
              duration: 3 * TICKS_PER_SECOND,
              trackId: null,
            })
          }
        >
          {t("polish.addBlur")}
        </button>
        {/* E10 (v1): hình khối, callout, spotlight. */}
        <select
          className="add-shape"
          value=""
          aria-label={t("shape.add")}
          onChange={(e) => {
            const shape = e.target.value as "rectangle" | "ellipse" | "arrow" | "line";
            if (!shape) return;
            const line = shape === "arrow" || shape === "line";
            apply({
              op: "addOverlay",
              content: {
                type: "shape",
                shape,
                x0: line ? 0.3 : 0.4,
                y0: line ? 0.7 : 0.35,
                x1: line ? 0.5 : 0.6,
                y1: line ? 0.5 : 0.65,
                stroke: "#E2557B",
                strokeWidth: 8,
                fill: null,
              },
              start: playhead,
              duration: 3 * TICKS_PER_SECOND,
              trackId: null,
            });
          }}
        >
          <option value="">{t("shape.add")}</option>
          {(["rectangle", "ellipse", "arrow", "line"] as const).map((k) => (
            <option key={k} value={k}>
              {t(`shape.${k}`)}
            </option>
          ))}
        </select>
        <button
          onClick={() =>
            apply({
              op: "addOverlay",
              content: { type: "callout", text: t("callout.default"), font: brandFont(), sizePx: 40, color: "#1D1B26", background: "#FFD54A", tailX: 0.35, tailY: 0.65 },
              start: playhead,
              duration: 3 * TICKS_PER_SECOND,
              trackId: null,
            })
          }
        >
          {t("callout.add")}
        </button>
        <button
          onClick={() =>
            apply({
              op: "addOverlay",
              content: { type: "spotlight", x: 0.3, y: 0.25, width: 0.4, height: 0.5, ellipse: true, dim: 0.65 },
              start: playhead,
              duration: 3 * TICKS_PER_SECOND,
              trackId: null,
            })
          }
        >
          {t("spotlight.add")}
        </button>
        <span className="sep" />
        <button onClick={() => apply({ op: "addTrack", kind: "video" })}>{t("editor.addVideoTrack")}</button>
        <button onClick={() => apply({ op: "addTrack", kind: "audio" })}>{t("editor.addAudioTrack")}</button>
        <span className="sep" />
        <button onClick={() => setShowMarkers(true)} title={t("marker.hint")}>
          🔖 {view.project.markers?.length ?? 0}
        </button>
        <button onClick={() => setShowBrand(true)} title={t("brand.title")}>
          🎨
        </button>
        <span style={{ flex: 1 }} />
        <label className="row small">
          {t("polish.aspect")}
          <select
            className="aspect-select"
            value={`${view.project.canvas.width}x${view.project.canvas.height}`}
            onChange={(e) => {
              const [w, h] = e.target.value.split("x").map(Number);
              apply({ op: "setCanvas", width: w, height: h });
            }}
          >
            {[
              ["1920x1080", "16:9"],
              ["1080x1920", "9:16"],
              ["1080x1080", "1:1"],
              ["1440x1080", "4:3"],
            ].map(([v, l]) => (
              <option key={v} value={v}>
                {l}
              </option>
            ))}
            {!["1920x1080", "1080x1920", "1080x1080", "1440x1080"].includes(
              `${view.project.canvas.width}x${view.project.canvas.height}`,
            ) && (
              <option value={`${view.project.canvas.width}x${view.project.canvas.height}`}>
                {view.project.canvas.width}×{view.project.canvas.height}
              </option>
            )}
          </select>
        </label>
        <label className="row small">
          {t("editor.zoom")}
          <input
            type="range"
            min={Math.log(4)}
            max={Math.log(800)}
            step={0.01}
            value={Math.log(pxPerSec)}
            onChange={(e) => setPxPerSec(Math.exp(Number(e.target.value)))}
          />
        </label>
      </div>

      <Timeline
        view={view}
        playhead={playhead}
        selected={selected}
        pxPerSec={pxPerSec}
        waveforms={waveforms}
        thumbs={thumbs}
        onSelect={(id) => {
          setSelected(id);
          if (id !== selected) setSelectedZoom(null);
        }}
        selectedZoom={selectedZoom}
        onSelectZoom={(clipId, zoomId) => {
          setSelected(clipId);
          setSelectedZoom(zoomId);
        }}
        onSeek={(tk) => {
          setPlayhead(tk);
          commands.playerSeek(Math.max(0, Math.min(tk, duration)));
        }}
        apply={apply}
        inOut={inOut}
      />

      {exporting && <ExportDialog view={view} inOut={inOut} onClose={() => setExporting(false)} />}
      {showBrand && <BrandDialog onClose={() => setShowBrand(false)} />}
      {showMarkers && (
        <MarkersDialog
          markers={view.project.markers ?? []}
          apply={apply}
          onSeek={(at) => commands.playerSeek(at)}
          onClose={() => setShowMarkers(false)}
        />
      )}
      {closeError && (
        <SaveFailedDialog
          error={closeError}
          mode="close"
          onRetry={async () => {
            const r = await commands.projectClose(false);
            if (r.status === "error") return r.error;
            navigate("/");
            return null;
          }}
          onDiscard={async () => {
            await commands.projectClose(true);
            navigate("/");
          }}
          onStay={() => setCloseError(null)}
        />
      )}

      {mediaDrag && (
        <div className="drag-ghost" style={{ left: mediaDrag.x + 12, top: mediaDrag.y + 8 }}>
          {mediaDrag.media.name}
        </div>
      )}

      {showRecovered && (
        <RecoveredDialog
          dir={view.dir}
          onDone={(v) => {
            setShowRecovered(false);
            if (v) setView(v);
          }}
        />
      )}

      {toast && (
        <div className="toast error-box" role="alert">
          {toast}
        </div>
      )}
      {info && !toast && (
        <div className="toast info-box" role="status">
          {info}
        </div>
      )}

      {rejected.length > 0 && (
        <Dialog title={t("editor.rejectedTitle")} onClose={() => setRejected([])} actions={<button onClick={() => setRejected([])}>{t("common.close")}</button>}>
          <ul className="plain-list">
            {rejected.map((r) => (
              <li key={r.path}>
                <strong>{r.path.split(/[\\/]/).pop()}</strong> — <span className="muted">{r.reason}</span>
              </li>
            ))}
          </ul>
        </Dialog>
      )}

      {duplicates.length > 0 && rejected.length === 0 && (
        <Dialog title={t("editor.duplicatesTitle")} onClose={() => setDuplicates([])} actions={<button onClick={() => setDuplicates([])}>{t("common.close")}</button>}>
          <p>{t("editor.duplicatesBody")}</p>
          <ul className="plain-list">
            {duplicates.map((d) => (
              <li key={d.path} className="row">
                <span style={{ flex: 1 }}>{d.path.split(/[\\/]/).pop()}</span>
                <button onClick={() => setDuplicates((x) => x.filter((y) => y.path !== d.path))}>{t("editor.reuse")}</button>
                <button
                  onClick={async () => {
                    const r = await commands.mediaImportDuplicate(d.path);
                    if (r.status === "ok") setView(r.data);
                    setDuplicates((x) => x.filter((y) => y.path !== d.path));
                  }}
                >
                  {t("editor.addNew")}
                </button>
              </li>
            ))}
          </ul>
        </Dialog>
      )}

      {removing && (
        <Dialog
          title={t("editor.removeInUseTitle")}
          onClose={() => setRemoving(null)}
          actions={
            <>
              <button onClick={() => setRemoving(null)}>{t("common.cancel")}</button>
              <button
                className="danger"
                onClick={async () => {
                  const r = await commands.mediaRemove(removing.media.id, true);
                  if (r.status === "ok") setView(r.data);
                  setRemoving(null);
                  setSelected(null);
                }}
              >
                {t("editor.removeAnyway")}
              </button>
            </>
          }
        >
          <p>
            <strong>{removing.media.name}</strong>
          </p>
          <p>{t("editor.removeInUseBody", { n: removing.clips })}</p>
        </Dialog>
      )}
    </div>
  );
}

export { TICKS_PER_SECOND };
