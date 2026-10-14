import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commands,
  type CodecSupport,
  type ExportError,
  type ExportEstimate,
  type ExportFormat,
  type ExportQuality,
  type ExportSettings,
  type ProjectView,
  type Ticks,
  type VideoCodec,
} from "../bindings";
import { formatBytes } from "../lib/recorder";
import { exportErrorMessage, startExport } from "../lib/exports";
import { formatTimecode, ticks } from "../lib/editor";
import Dialog from "./Dialog";
import { Check } from "./controls";

type Preset = "youtube" | "uhd" | "social" | "gif" | "custom";

const SHORT_SIDES = [360, 480, 720, 1080, 1440, 2160];
const MP4_FPS = [24, 25, 30, 50, 60];
const GIF_FPS = [10, 12, 15, 20, 25];

/** X01–X03: hộp thoại xuất. Mỗi lần đổi thiết lập → preflight + ước tính ở phía Rust. */
export default function ExportDialog(props: {
  view: ProjectView;
  inOut: { start: Ticks | null; end: Ticks | null };
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { view } = props;
  const canvas = view.project.canvas;
  const portrait = canvas.height > canvas.width;
  const hasRange = props.inOut.start !== null && props.inOut.end !== null && props.inOut.end > props.inOut.start;

  const [caps, setCaps] = useState<CodecSupport[] | null>(null);
  const [preset, setPreset] = useState<Preset>(portrait ? "social" : "youtube");
  const [format, setFormat] = useState<ExportFormat>("mp4");
  const [codec, setCodec] = useState<VideoCodec>("h264");
  const [shortSide, setShortSide] = useState(1080);
  const [fps, setFps] = useState(Math.min(60, canvas.fps));
  const [quality, setQuality] = useState<ExportQuality>("high");
  const [useRange, setUseRange] = useState(hasRange);
  const [output, setOutput] = useState("");
  // K-X1: đồng ý ghi đè gắn với đúng một đường dẫn; đổi đường dẫn (gõ tay, chọn file, đổi định dạng) là phải hỏi lại.
  const [overwriteFor, setOverwriteFor] = useState<string | null>(null);
  const overwrite = overwriteFor !== null && overwriteFor === output;
  useEffect(() => setOverwriteFor(null), [output]);
  const [blackFill, setBlackFill] = useState(false);
  // X05 (v1): kèm file phụ đề .srt cạnh video.
  const hasCaptions = (props.view.project.captions?.length ?? 0) > 0;
  const [withSubtitles, setWithSubtitles] = useState(true);
  const [est, setEst] = useState<{ ok: ExportEstimate } | { err: ExportError } | null>(null);
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    commands.exportCapabilities().then(setCaps);
    commands.exportDefaultDir().then((dir) => setOutput(`${dir}\\${view.project.name}.mp4`));
  }, [view.project.name]);

  // Đuôi file theo định dạng.
  useEffect(() => {
    setOutput((o) => (o ? o.replace(/\.(mp4|gif|m4a|mp3|webm)$/i, "") + `.${format}` : o));
  }, [format]);

  const audioOnly = format === "m4a" || format === "mp3";

  function applyPreset(p: Preset) {
    setPreset(p);
    const best = (c: VideoCodec) => caps?.find((x) => x.codec === c)?.encoder != null;
    if (p === "youtube") {
      setFormat("mp4"), setCodec("h264"), setShortSide(1080), setFps(Math.min(60, canvas.fps)), setQuality("high");
    } else if (p === "uhd") {
      setFormat("mp4"), setCodec(best("hevc") ? "hevc" : "h264"), setShortSide(2160), setFps(Math.min(60, canvas.fps)), setQuality("high");
    } else if (p === "social") {
      setFormat("mp4"), setCodec("h264"), setShortSide(1080), setFps(30), setQuality("standard");
    } else if (p === "gif") {
      setFormat("gif"), setShortSide(480), setFps(15), setQuality("standard");
    }
  }

  const settings: ExportSettings | null = useMemo(
    () =>
      output
        ? {
            format,
            codec,
            shortSide,
            fps,
            quality,
            range: useRange && hasRange ? { start: props.inOut.start!, end: props.inOut.end! } : null,
            output,
            overwrite,
            allowBlackFill: blackFill,
            subtitles: withSubtitles && hasCaptions,
          }
        : null,
    [format, codec, shortSide, fps, quality, useRange, hasRange, props.inOut.start, props.inOut.end, output, overwrite, blackFill, withSubtitles, hasCaptions],
  );

  useEffect(() => {
    if (!settings) return;
    let alive = true;
    const h = setTimeout(async () => {
      const r = await commands.exportEstimate(settings);
      if (alive) setEst(r.status === "ok" ? { ok: r.data } : { err: r.error });
    }, 200);
    return () => {
      alive = false;
      clearTimeout(h);
    };
  }, [settings]);

  // X06 (v1): mỗi đoạn giữa hai marker một file (đoạn trước marker đầu nếu dài hơn 1 giây).
  const markers = [...(props.view.project.markers ?? [])].sort((a, b) => a.at - b.at);
  const markerRanges = (() => {
    const end = props.view.duration;
    const cuts = markers.map((m) => ({ at: m.at, label: m.label })).filter((m) => m.at < end);
    const out: { start: number; end: number; label: string }[] = [];
    if (cuts.length && cuts[0].at > ticks(1)) out.push({ start: 0, end: cuts[0].at, label: "intro" });
    cuts.forEach((c, i) => out.push({ start: c.at, end: i + 1 < cuts.length ? cuts[i + 1].at : end, label: c.label }));
    return out.filter((r) => r.end - r.start > ticks(0.1));
  })();
  const [byMarkers, setByMarkers] = useState(false);

  async function start() {
    if (!settings) return;
    setStarting(true);
    if (byMarkers && markerRanges.length) {
      const base = settings.output.replace(/\.[^.\\/]+$/, "");
      const ext = settings.output.slice(base.length);
      for (const [i, r] of markerRanges.entries()) {
        const safe = r.label.replace(/[\\/:*?"<>|]/g, "").trim().slice(0, 40) || `${i + 1}`;
        const err = await startExport({ ...settings, range: { start: r.start, end: r.end }, output: `${base}-${String(i + 1).padStart(2, "0")}-${safe}${ext}` });
        if (err) {
          setStarting(false);
          return setEst({ err });
        }
      }
      setStarting(false);
      return props.onClose();
    }
    const err = await startExport(settings);
    setStarting(false);
    if (err) setEst({ err });
    else props.onClose();
  }

  const codecLabel: Record<VideoCodec, string> = { h264: "H.264", hevc: "HEVC (H.265)", av1: "AV1" };
  const err = est && "err" in est ? est.err : null;
  const ok = est && "ok" in est ? est.ok : null;

  return (
    <Dialog
      title={t("export.title")}
      onClose={props.onClose}
      actions={
        <>
          <button onClick={props.onClose}>{t("common.cancel")}</button>
          <button className="primary" disabled={!ok || starting} onClick={start}>
            {t("export.start")}
          </button>
        </>
      }
    >
      <div className="export-form">
        <span>{t("export.preset")}</span>
        <select value={preset} onChange={(e) => applyPreset(e.target.value as Preset)}>
          <option value="youtube">{t("export.presetYoutube")}</option>
          <option value="uhd">{t("export.presetUhd")}</option>
          <option value="social" disabled={!portrait}>
            {t("export.presetSocial")}
            {!portrait ? ` — ${t("export.needPortrait")}` : ""}
          </option>
          <option value="gif">{t("export.presetGif")}</option>
          <option value="custom">{t("export.presetCustom")}</option>
        </select>

        <span>{t("export.format")}</span>
        <select value={format} onChange={(e) => (setFormat(e.target.value as ExportFormat), setPreset("custom"))}>
          <option value="mp4">MP4</option>
          <option value="gif">GIF</option>
          {/* X02 (v1): chỉ âm thanh. */}
          <option value="webm">WebM ({t("export.webmAlpha")})</option>
          <option value="m4a">M4A ({t("export.audioOnly")})</option>
          <option value="mp3">MP3 ({t("export.audioOnly")})</option>
        </select>

        {format === "mp4" && (
          <>
            <span>{t("export.codec")}</span>
            <select value={codec} onChange={(e) => (setCodec(e.target.value as VideoCodec), setPreset("custom"))}>
              {(["h264", "hevc", "av1"] as VideoCodec[]).map((c) => {
                const s = caps?.find((x) => x.codec === c);
                const supported = s?.encoder != null;
                return (
                  <option key={c} value={c} disabled={!supported}>
                    {codecLabel[c]}
                    {s ? (supported ? ` — ${s.hardware ? t("export.hardware") : t("export.software")}` : ` — ${t("export.unsupported")}`) : " …"}
                  </option>
                );
              })}
            </select>
          </>
        )}

        {!audioOnly && (
          <>
            <span>{t("export.resolution")}</span>
            <select value={shortSide} onChange={(e) => (setShortSide(Number(e.target.value)), setPreset("custom"))}>
              {SHORT_SIDES.map((s) => (
                <option key={s} value={s}>
                  {s}p
                </option>
              ))}
            </select>

            <span>{t("export.fps")}</span>
            <select value={fps} onChange={(e) => (setFps(Number(e.target.value)), setPreset("custom"))}>
              {(format === "gif" ? GIF_FPS : MP4_FPS).map((f) => (
                <option key={f} value={f}>
                  {f} fps
                </option>
              ))}
            </select>
          </>
        )}

        <span>{t("export.quality")}</span>
        <select value={quality} onChange={(e) => (setQuality(e.target.value as ExportQuality), setPreset("custom"))}>
          <option value="standard">{t("export.qStandard")}</option>
          <option value="high">{t("export.qHigh")}</option>
          <option value="max">{t("export.qMax")}</option>
        </select>

        <span>{t("export.range")}</span>
        <select
          value={byMarkers ? "markers" : useRange && hasRange ? "range" : "all"}
          onChange={(e) => {
            setByMarkers(e.target.value === "markers");
            setUseRange(e.target.value === "range");
          }}
        >
          <option value="markers" disabled={!markerRanges.length}>
            {t("export.byMarkers", { n: markerRanges.length })}
          </option>
          <option value="all">{t("export.rangeAll", { d: formatTimecode(view.duration) })}</option>
          <option value="range" disabled={!hasRange}>
            {hasRange
              ? t("export.rangeInOut", { a: formatTimecode(props.inOut.start!), b: formatTimecode(props.inOut.end!) })
              : t("export.rangeNone")}
          </option>
        </select>

        <span>{t("export.saveTo")}</span>
        <div className="export-path">
          <input type="text" value={output} onChange={(e) => setOutput(e.target.value)} spellCheck={false} />
          <button
            onClick={async () => {
              const p = await commands.exportPickPath(output, format);
              if (p) setOutput(p);
            }}
          >
            {t("export.browse")}
          </button>
        </div>
      </div>

      <div className={err ? "export-est bad" : "export-est"}>
        {!est && <span className="muted">…</span>}
        {ok && (
          <>
            <div>
              {audioOnly
                ? t("export.summaryAudio", { secs: ok.durationSecs.toFixed(1), size: formatBytes(ok.bytes) })
                : t("export.summary", {
                    w: ok.width,
                    h: ok.height,
                    frames: ok.frames,
                    secs: ok.durationSecs.toFixed(1),
                    size: formatBytes(ok.bytes),
                  })}
              {ok.encoder && <span className="muted"> · {ok.encoder}</span>}
            </div>
            {ok.warnings.length > 0 && (
              <ul>
                {ok.warnings.map((w, i) => (
                  <li key={i}>
                    {w.kind === "gifTooLarge" && t("export.warnGif", { size: formatBytes(w.bytes) })}
                    {w.kind === "fileNameChanged" && t("export.warnName", { to: w.to })}
                    {w.kind === "blackFill" && t("export.warnBlack", { names: w.names.join(", ") })}
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
        {err && <div>{exportErrorMessage(t, err)}</div>}
        {(err?.code === "fileExists" || overwrite) && (
          <Check label={t("export.overwrite")} checked={overwrite} onChange={(v) => setOverwriteFor(v ? output : null)} />
        )}
        {hasCaptions && !audioOnly && (
          <Check label={t("export.subtitles")} checked={withSubtitles} onChange={setWithSubtitles} />
        )}
        {(err?.code === "missingMedia" || blackFill) && (
          <Check label={t("export.allowBlack")} checked={blackFill} onChange={setBlackFill} />
        )}
      </div>
    </Dialog>
  );
}
