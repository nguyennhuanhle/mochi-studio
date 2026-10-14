import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router";
import {
  commands,
  events,
  type CaptureTarget,
  type DeviceList,
  type DisplayInfo,
  type CursorHighlight,
  type HotkeyConflict,
  type KeystrokeMode,
  type RecorderError,
  type RecordingQuality,
  type Settings,
  type WindowInfo,
} from "../bindings";
import { errorMessage } from "../lib/recorder";
import { useCameraPreview, useMicLevel, useSystemAudioLevel } from "../lib/media";
import Dialog from "../components/Dialog";
import HotkeysDialog from "../components/HotkeysDialog";
import GpuWarning from "../components/GpuWarning";
import HighlightControls, { DEFAULT_HIGHLIGHT } from "../components/HighlightControls";

type AreaField = "x" | "y" | "width" | "height";

type Tab = "display" | "window" | "area";

/** Màn thiết lập quay: R01 (vùng quay), R02 (nguồn), R04 (đếm ngược, phím tắt), R06 (chất lượng). */
export default function Record() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [displays, setDisplays] = useState<DisplayInfo[]>([]);
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  const [devices, setDevices] = useState<DeviceList>({ mics: [], cameras: [] });
  const [thumbs, setThumbs] = useState<Record<string, string>>({});

  const [tab, setTab] = useState<Tab>("display");
  const [target, setTarget] = useState<CaptureTarget | null>(null);
  const [areaDisplay, setAreaDisplay] = useState<string>("");
  const [mic, setMic] = useState<string | null>(null);
  const [camera, setCamera] = useState<string | null>(null);
  const [systemAudio, setSystemAudio] = useState(true);
  const [fps, setFps] = useState(60);
  const [quality, setQuality] = useState<RecordingQuality>("high");
  const [countdown, setCountdown] = useState(3);
  const [keystrokes, setKeystrokes] = useState<KeystrokeMode>("off");
  /** R13: highlight con trỏ (chỉ có trong preview / video xuất, không vẽ lên màn hình lúc quay). */
  const [highlight, setHighlight] = useState<CursorHighlight>(DEFAULT_HIGHLIGHT);
  const [confirmAllKeys, setConfirmAllKeys] = useState(false);

  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<RecorderError | null>(null);
  const [conflicts, setConflicts] = useState<HotkeyConflict[]>([]);
  const [editHotkeys, setEditHotkeys] = useState(false);

  const cameraName = devices.cameras.find((c) => c.id === camera)?.name ?? null;
  const cam = useCameraPreview(cameraName);
  const micLevel = useMicLevel(mic);
  const sysLevel = useSystemAudioLevel(systemAudio && !starting);

  // R01: nhập tay vùng quay theo pixel thật; lưu theo đơn vị logic (như khung chọn vùng).
  const areaDisp = displays.find((d) => d.id === areaDisplay);
  const scale = areaDisp && areaDisp.logicalWidth > 0 ? areaDisp.physicalWidth / areaDisp.logicalWidth : 1;
  const area =
    target?.kind === "area" && target.displayId === areaDisplay
      ? target
      : areaDisp
        ? {
            kind: "area" as const,
            displayId: areaDisplay,
            x: 0,
            y: 0,
            width: Math.min(1280 / scale, areaDisp.logicalWidth),
            height: Math.min(720 / scale, areaDisp.logicalHeight),
          }
        : null;
  function setAreaField(field: AreaField, physical: number) {
    if (!area || !areaDisp || !Number.isFinite(physical)) return;
    const W = areaDisp.logicalWidth;
    const H = areaDisp.logicalHeight;
    const next = { ...area, [field]: physical / scale };
    next.width = Math.min(Math.max(next.width, 16), W);
    next.height = Math.min(Math.max(next.height, 16), H);
    next.x = Math.min(Math.max(next.x, 0), W - next.width);
    next.y = Math.min(Math.max(next.y, 0), H - next.height);
    setTarget(next);
  }

  // Nạp dữ liệu + khôi phục cấu hình lần trước.
  useEffect(() => {
    (async () => {
      const [s, ds, ws, dv, hk] = await Promise.all([
        commands.settingsGet(),
        commands.recorderListDisplays(),
        commands.recorderListWindows(),
        commands.recorderListDevices(),
        commands.hotkeysApply(),
      ]);
      setSettings(s);
      setDisplays(ds);
      setWindows(ws);
      setDevices(dv);
      setConflicts(hk);
      setAreaDisplay(ds[0]?.id ?? "");
      const last = s.lastRecording;
      if (last) {
        setTab(last.target.kind);
        if (last.target.kind === "area") setAreaDisplay(last.target.displayId);
        setTarget(last.target);
        setMic(last.mic && dv.mics.includes(last.mic) ? last.mic : null);
        setCamera(last.camera && dv.cameras.some((c) => c.id === last.camera) ? last.camera : null);
        setSystemAudio(last.systemAudio);
        setFps(last.fps);
        setQuality(last.quality);
        setCountdown(last.countdownSecs);
        setKeystrokes(last.keystrokes);
        if (last.cursorHighlight) setHighlight(last.cursorHighlight);
      } else if (ds[0]) {
        setTarget({ kind: "display", id: ds[0].id });
        setCountdown(s.countdownSecs);
      }
      for (const d of ds) {
        commands.recorderThumbnail({ kind: "display", id: d.id }).then((r) => {
          if (r.status === "ok") setThumbs((prev) => ({ ...prev, [d.id]: r.data }));
        });
      }
    })();
  }, []);

  useEffect(() => {
    const un = events.regionSelected.listen((e) => {
      if (e.payload) setTarget(e.payload);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  async function refreshWindows() {
    setWindows(await commands.recorderListWindows());
  }

  const targetValid = useMemo(() => {
    if (!target) return false;
    if (tab === "display") return target.kind === "display";
    if (tab === "window") return target.kind === "window" && windows.some((w) => w.id === target.id);
    return target.kind === "area";
  }, [target, tab, windows]);

  async function start() {
    if (!target || !settings?.outputRoot) return;
    setError(null);
    setStarting(true);
    // Nhả webcam/mic của phần xem trước để pipeline quay mở được thiết bị.
    cam.release();
    micLevel.release();
    const res = await commands.recorderStart({
      target,
      mic,
      camera,
      systemAudio,
      fps,
      quality,
      countdownSecs: countdown,
      keystrokes,
      outputRoot: settings.outputRoot,
      cursorHighlight: highlight,
    });
    setStarting(false);
    if (res.status === "error") setError(res.error);
  }

  function chooseKeystrokes(mode: KeystrokeMode) {
    if (mode === "all") setConfirmAllKeys(true);
    else setKeystrokes(mode);
  }

  const hk = settings?.hotkeys;

  return (
    <div className="page record-page">
      <header className="row">
        <Link to="/" className="icon-link" aria-label={t("common.back")}>
          ←
        </Link>
        <h2 style={{ margin: 0 }}>{t("record.title")}</h2>
      </header>

      {/* GPU / driver dưới mức tối thiểu: báo cả ở màn thiết lập quay (vẫn cho quay). */}
      <GpuWarning />
      <div className="record-grid">
        <section className="panel">
          <h3>{t("record.what")}</h3>
          <div className="tabs" role="tablist">
            {(["display", "window", "area"] as Tab[]).map((k) => (
              <button
                key={k}
                role="tab"
                aria-selected={tab === k}
                className={tab === k ? "tab active" : "tab"}
                onClick={() => {
                  setTab(k);
                  if (k === "display" && displays[0]) setTarget({ kind: "display", id: displays[0].id });
                  if (k === "window") {
                    refreshWindows();
                    setTarget(null);
                  }
                  if (k === "area") setTarget(null);
                }}
              >
                {t(`record.tab${k[0].toUpperCase()}${k.slice(1)}`)}
              </button>
            ))}
          </div>

          {tab === "display" && (
            <div className="display-grid">
              {displays.map((d) => {
                const selected = target?.kind === "display" && target.id === d.id;
                return (
                  <button
                    key={d.id}
                    className={selected ? "display-card selected" : "display-card"}
                    onClick={() => setTarget({ kind: "display", id: d.id })}
                  >
                    <div className="thumb">{thumbs[d.id] && <img src={thumbs[d.id]} alt="" />}</div>
                    <div className="card-label">
                      {d.name} · {d.physicalWidth}×{d.physicalHeight} · {d.refreshRate} Hz
                    </div>
                  </button>
                );
              })}
            </div>
          )}

          {tab === "window" && (
            <>
              <div className="row" style={{ justifyContent: "flex-end" }}>
                <button onClick={refreshWindows}>{t("record.refresh")}</button>
              </div>
              <ul className="window-list">
                {windows.length === 0 && <li className="muted">{t("record.noWindows")}</li>}
                {windows.map((w) => {
                  const selected = target?.kind === "window" && target.id === w.id;
                  return (
                    <li key={w.id}>
                      <button
                        className={selected ? "window-item selected" : "window-item"}
                        onClick={() => setTarget({ kind: "window", id: w.id })}
                      >
                        {w.icon ? <img src={w.icon} alt="" width={20} height={20} /> : <span className="icon-ph" />}
                        <span className="window-title">{w.title}</span>
                        <span className="muted">{w.app}</span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </>
          )}

          {tab === "area" && (
            <div className="stack">
              <label className="row">
                <span>{t("record.areaOn")}</span>
                <select value={areaDisplay} onChange={(e) => setAreaDisplay(e.target.value)}>
                  {displays.map((d) => (
                    <option key={d.id} value={d.id}>
                      {d.name} · {d.physicalWidth}×{d.physicalHeight}
                    </option>
                  ))}
                </select>
              </label>
              <button onClick={() => commands.regionPickerOpen(areaDisplay)}>{t("record.pickArea")}</button>
              {area && areaDisp && (
                <div className="area-inputs">
                  {(["x", "y", "width", "height"] as AreaField[]).map((f) => {
                    const value = Math.round(area[f] * scale);
                    return (
                      <label key={f} className="field inline">
                        <span>{t(`record.area_${f}`)}</span>
                        <input
                          key={`${f}-${value}`}
                          type="number"
                          defaultValue={value}
                          min={f === "x" || f === "y" ? 0 : 16}
                          onBlur={(e) => setAreaField(f, Number(e.target.value))}
                          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
                        />
                      </label>
                    );
                  })}
                  <span className="muted small">
                    {t("record.areaUnits", { w: areaDisp.physicalWidth, h: areaDisp.physicalHeight })}
                  </span>
                </div>
              )}
              <p className="muted">
                {target?.kind === "area"
                  ? t("record.areaSelected", {
                      w: Math.round(target.width * scale),
                      h: Math.round(target.height * scale),
                      x: Math.round(target.x * scale),
                      y: Math.round(target.y * scale),
                    })
                  : t("record.areaNone")}
              </p>
            </div>
          )}
        </section>

        <section className="panel">
          <h3>{t("record.sources")}</h3>
          <label className="field">
            <span>{t("record.camera")}</span>
            <select value={camera ?? ""} onChange={(e) => setCamera(e.target.value || null)}>
              <option value="">{t("common.none")}</option>
              {devices.cameras.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </label>
          <div className="cam-preview">
            {camera && !cam.error ? (
              <video ref={cam.videoRef} autoPlay muted playsInline />
            ) : camera && cam.error === "denied" ? (
              <span className="warn small">
                {t("record.previewDenied", { kind: t("device.camera") })}{" "}
                <button className="link-btn" onClick={() => commands.openPrivacySettings("camera")}>
                  {t("common.openSettings")}
                </button>
              </span>
            ) : (
              <span className="muted">{camera ? t("record.previewFailed") : t("record.previewOff")}</span>
            )}
          </div>

          <label className="field">
            <span>{t("record.mic")}</span>
            <select value={mic ?? ""} onChange={(e) => setMic(e.target.value || null)}>
              <option value="">{t("common.none")}</option>
              {devices.mics.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          </label>
          <div className="meter" aria-hidden>
            <div className="meter-fill" style={{ width: `${Math.round(micLevel.level * 100)}%` }} />
          </div>
          {mic && micLevel.error === "denied" && (
            <span className="warn small">
              {t("record.previewDenied", { kind: t("device.microphone") })}{" "}
              <button className="link-btn" onClick={() => commands.openPrivacySettings("microphone")}>
                {t("common.openSettings")}
              </button>
            </span>
          )}

          <label className="check">
            <input type="checkbox" checked={systemAudio} onChange={(e) => setSystemAudio(e.target.checked)} />
            <span>
              {t("record.systemAudio")}
              <small className="muted"> — {t("record.systemAudioHint")}</small>
            </span>
          </label>
          {systemAudio && (
            <>
              <div className="meter" aria-hidden>
                <div className="meter-fill" style={{ width: `${Math.round(sysLevel.level * 100)}%` }} />
              </div>
              <span className="muted small">
                {sysLevel.error
                  ? t("record.systemAudioError", { detail: sysLevel.error })
                  : sysLevel.device
                    ? t("record.systemAudioDevice", { name: sysLevel.device })
                    : "…"}
              </span>
            </>
          )}

          <h3>{t("record.quality")}</h3>
          <div className="row wrap">
            <label className="field inline">
              <span>{t("record.fps")}</span>
              <select value={fps} onChange={(e) => setFps(Number(e.target.value))}>
                <option value={30}>30</option>
                <option value={60}>60</option>
              </select>
            </label>
            <label className="field inline">
              <span>{t("record.quality")}</span>
              <select value={quality} onChange={(e) => setQuality(e.target.value as RecordingQuality)}>
                <option value="standard">{t("record.qualityStandard")}</option>
                <option value="high">{t("record.qualityHigh")}</option>
                <option value="max">{t("record.qualityMax")}</option>
              </select>
            </label>
            <label className="field inline">
              <span>{t("record.countdown")}</span>
              <select value={countdown} onChange={(e) => setCountdown(Number(e.target.value))}>
                {[0, 3, 5, 10].map((n) => (
                  <option key={n} value={n}>
                    {n === 0 ? t("record.keystrokesOff") : t("record.seconds", { n })}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <p className="muted small">{t("record.qualityHint")}</p>

          <label className="field">
            <span>{t("record.keystrokes")}</span>
            <select value={keystrokes} onChange={(e) => chooseKeystrokes(e.target.value as KeystrokeMode)}>
              <option value="off">{t("record.keystrokesOff")}</option>
              <option value="modifiersOnly">{t("record.keystrokesModifiers")}</option>
              <option value="all">{t("record.keystrokesAll")}</option>
            </select>
          </label>
          <p className="muted small">{t("record.keystrokesHint")}</p>

          <h3>{t("highlight.title")}</h3>
          <HighlightControls value={highlight} onChange={setHighlight} />
          <p className="muted small">{t("highlight.recordHint")}</p>

          <p className="muted small">
            {t("record.saveTo")}: {settings?.outputRoot}
          </p>
        </section>
      </div>

      <footer className="record-footer">
        <div className="stack">
          {hk && (
            <span className="muted small">
              {t("record.hotkeys", { record: hk.recordPause, stop: hk.stop, cancel: hk.cancel })}{" "}
              <button className="link-btn" onClick={() => setEditHotkeys(true)}>
                {t("record.changeHotkeys")}
              </button>
            </span>
          )}
          {conflicts.length > 0 && (
            <span className="warn small">
              {t("record.hotkeyConflict", { keys: conflicts.map((c) => c.key).join(", ") })}{" "}
              <button className="link-btn" onClick={() => setEditHotkeys(true)}>
                {conflicts.some((c) => c.suggestion) ? t("record.hotkeyFix") : t("record.changeHotkeys")}
              </button>
            </span>
          )}
          {error && (
            <div className="error-box" role="alert">
              {errorMessage(t, error)}
              {error.code === "permissionDenied" && (
                <button onClick={() => commands.openPrivacySettings(error.kind)}>{t("common.openSettings")}</button>
              )}
            </div>
          )}
        </div>
        <button className="primary big" disabled={!targetValid || starting} onClick={start}>
          {starting ? t("record.starting") : `● ${t("record.start")}`}
        </button>
      </footer>

      {editHotkeys && settings && (
        <HotkeysDialog
          initial={settings.hotkeys}
          conflicts={conflicts}
          onClose={(saved) => {
            setEditHotkeys(false);
            if (saved) {
              setSettings({ ...settings, hotkeys: saved.hotkeys });
              setConflicts(saved.conflicts);
            }
          }}
        />
      )}

      {confirmAllKeys && (
        <Dialog
          title={t("record.keystrokesAllTitle")}
          onClose={() => setConfirmAllKeys(false)}
          actions={
            <>
              <button onClick={() => setConfirmAllKeys(false)}>{t("common.cancel")}</button>
              <button
                className="danger"
                onClick={() => {
                  setKeystrokes("all");
                  setConfirmAllKeys(false);
                }}
              >
                {t("record.keystrokesAllConfirm")}
              </button>
            </>
          }
        >
          <p>{t("record.keystrokesAllBody")}</p>
        </Dialog>
      )}
    </div>
  );
}
