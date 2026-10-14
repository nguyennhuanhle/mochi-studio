import { commands, events, type ExportError, type RecorderEvent } from "../bindings";
import { retryExport, type ExportJobView } from "./exports";

/**
 * Tự chạy luồng quay và chụp từng cửa sổ của Mochi (chỉ bản dev, MOCHI_UI_SNAPSHOT).
 * Quay một vùng 64×64 ở góc màn hình để không ghi lại nội dung của người dùng.
 */
let started = false;

export async function runDevSnapshot(navigate: (to: string) => void) {
  // Đặt cờ trước mọi await: StrictMode gọi effect 2 lần liền nhau.
  if (started) return;
  started = true;
  if (!(await commands.devSnapshotEnabled())) return;
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
  const log: string[] = [];
  const seen: RecorderEvent[] = [];
  const un = await events.recorderEventMsg.listen(({ payload }) => {
    if (payload.type !== "stats") seen.push(payload);
  });
  const snap = async (label: string, name: string) => {
    const r = await commands.devSnapshot(label, name);
    log.push(`${name}: ${r.status === "ok" ? "ok" : r.error}`);
  };
  const [mode, outDir] = await commands.devSnapshotMode();
  if (mode === "polish") {
    await polishSnapshot(navigate, outDir, snap, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "jobs") {
    await jobsSnapshot(navigate, outDir, snap, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "home") {
    navigate("/");
    await sleep(2500);
    await snap("main", "0-home");
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "export") {
    await exportSnapshot(navigate, outDir, snap, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "warm") {
    await warmSnapshot(navigate, snap, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "editor") {
    await editorSnapshot(navigate, outDir, snap, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode.startsWith("v1")) {
    const { runV1 } = await import("./devV1");
    await runV1(mode, { navigate, outDir, snap, log, sleep });
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "cdp") {
    await cdpSnapshot(navigate, outDir, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "fix2") {
    await fix2Snapshot(navigate, outDir, snap, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "fix1") {
    await fix1Snapshot(navigate, outDir, snap, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "phase5") {
    await phase5Snapshot(navigate, outDir, snap, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "patch2c") {
    await patch2cSnapshot(navigate, outDir, snap, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "patch2b") {
    await patch2bSnapshot(navigate, outDir, snap, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "patch2a") {
    await patch2aSnapshot(navigate, outDir, snap, log, sleep, seen);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  if (mode === "patch1") {
    await patch1Snapshot(navigate, outDir, snap, log, sleep);
    un();
    await commands.devSnapshotFinish(JSON.stringify({ log }, null, 2));
    return;
  }
  try {
    navigate("/");
    await sleep(2000);
    await snap("main", "1-home");
    navigate("/record");
    await sleep(5000);
    await snap("main", "2-record");

    const [display] = await commands.recorderListDisplays();
    const settings = await commands.settingsGet();
    const res = await commands.recorderStart({
      target: { kind: "area", displayId: display.id, x: 0, y: 0, width: 64, height: 64 },
      mic: null,
      camera: null,
      systemAudio: true, // R07: thanh quay hiện chip "âm thanh hệ thống"
      fps: 30,
      quality: "standard",
      countdownSecs: 3,
      keystrokes: "off",
      outputRoot: settings.outputRoot!,
    });
    log.push(`start: ${JSON.stringify(res)}`);
    await sleep(1200);
    await snap("countdown", "3-countdown");
    await snap("recorder-bar", "4-bar-countdown");
    await sleep(3500);
    await snap("recorder-bar", "5-bar-recording");
    await commands.recorderPause();
    await sleep(1500);
    await snap("recorder-bar", "6-bar-paused");
    await commands.recorderResume();
    await sleep(1500);
    const stop = await commands.recorderStop();
    log.push(`stop: ${JSON.stringify(stop)}`);
    await sleep(2500);
    await snap("main", "7-recorded");
    // R08: dừng quay → app tự mở trình biên tập.
    log.push(`route sau khi dừng: ${location.hash}`);
    if (stop.status === "ok") {
      log.push(`close: ${JSON.stringify(await commands.projectClose(false))}`);
      navigate("/");
      log.push(`discard: ${JSON.stringify(await commands.recorderDiscard(stop.data.projectDir))}`);
    }
  } catch (e) {
    log.push(`error: ${String(e)}`);
  }
  un();
  await commands.devSnapshotFinish(JSON.stringify({ log, events: seen }, null, 2));
}

/** Trình biên tập trên media TỔNG HỢP (env MOCHI_UI_SNAPSHOT_MEDIA: thư mục chứa file thử). */
async function editorSnapshot(
  navigate: (to: string) => void,
  outDir: string,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
) {
  const media = outDir.replace(/[\\/]ui-snap.*$/, "");
  const files = [`${media}/bench/testsrc-1080p60.mp4`, `${media}/bench/logo.png`, `${media}/round2/chuyen-di/nhac-nen.mp3`];
  try {
    const open = await commands.projectOpen(`${outDir}/Snapshot.mochi`);
    log.push(`open: ${open.status}`);
    const imp = await commands.mediaImport(files);
    if (imp.status !== "ok") throw new Error(JSON.stringify(imp.error));
    log.push(`import: ${imp.data.report.imported.length} ok, ${imp.data.report.rejected.length} lỗi`);
    const [video, logo, music] = imp.data.view.project.media;
    const T = 705_600_000;
    await commands.timelineApply({ op: "addClip", mediaId: video.id, trackId: null, start: 0 });
    await commands.timelineApply({ op: "addClip", mediaId: logo.id, trackId: null, start: 1 * T });
    await commands.timelineApply({ op: "addClip", mediaId: music.id, trackId: null, start: Math.round(0.5 * T) });
    let v = await commands.projectView();
    const logoClip = v!.project.clips.find((c) => c.mediaId === logo.id)!;
    await commands.timelineApply({
      op: "setClipTransform",
      clipId: logoClip.id,
      transform: { x: 0.82, y: 0.22, scale: 0.28, opacity: 0.95 },
    });
    const videoClip = v!.project.clips.find((c) => c.mediaId === video.id)!;
    const split = await commands.timelineApply({ op: "splitClip", clipId: videoClip.id, at: 3 * T });
    log.push(`split: ${split.status}`);
    navigate("/editor");
    await sleep(5000);
    await commands.playerSeek(Math.round(2.0 * T));
    await sleep(2500);
    await snap("main", "8-editor");
    v = await commands.projectView();
    log.push(`clips: ${v!.project.clips.length}, tracks: ${v!.project.tracks.length}, duration: ${(v!.duration / T).toFixed(2)}s`);
  } catch (e) {
    log.push(`error: ${String(e)}`);
  }
}

/** Vòng 3: project bản quay TỔNG HỢP (recordings/round3/Fake.mochi, do `mochi-cli polish` tạo). */
async function polishSnapshot(
  navigate: (to: string) => void,
  outDir: string,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  try {
    const open = await commands.projectOpen(`${root}/round3/Fake.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    const v = open.data;
    const rec = v.project.clips.find((c) => c.content.type === "recording")!;
    const zoom = rec.content.type === "recording" ? rec.content.fx?.zoom.segments[0] : undefined;
    log.push(`clips: ${v.project.clips.length}, zoom: ${rec.content.type === "recording" ? rec.content.fx?.zoom.segments.length : 0}`);
    navigate("/editor");
    await sleep(3000);
    await commands.playerSeek(Math.round(2.8 * 705_600_000));
    (window as unknown as { __mochiSelect: (c: string, z: string | null) => void }).__mochiSelect(rec.id, zoom?.id ?? null);
    await sleep(12000); // renderer của Cap mở lần đầu ~5–10 s
    await snap("main", "9-polish-editor");
  } catch (e) {
    log.push(`error: ${String(e)}`);
  }
}

/** Vòng 4: làm ấm preview lúc dừng quay. Quay vùng 64×64 → dừng → chờ làm ấm → mở trình biên tập
 *  và đo tới frame đầu → đóng → bỏ bản quay (file phải xóa được). */
async function warmSnapshot(
  navigate: (to: string) => void,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
) {
  try {
    const [display] = await commands.recorderListDisplays();
    const settings = await commands.settingsGet();
    await commands.recorderStart({
      target: { kind: "area", displayId: display.id, x: 0, y: 0, width: 64, height: 64 },
      mic: null,
      camera: null,
      systemAudio: false,
      fps: 30,
      quality: "standard",
      countdownSecs: 0,
      keystrokes: "off",
      outputRoot: settings.outputRoot!,
    });
    await sleep(3000);
    const stop = await commands.recorderStop();
    if (stop.status !== "ok") throw new Error(JSON.stringify(stop.error));
    const dir = stop.data.projectDir;
    log.push(`stop ok`);
    await sleep(12000); // làm ấm chạy nền (~7 s ở bản debug)

    const t0 = performance.now();
    const open = await commands.projectOpen(dir);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    const firstFrame = new Promise<number>((resolve) => {
      const unl = events.playerTime.listen(() => {
        unl.then((f) => f());
        resolve(performance.now() - t0);
      });
    });
    navigate("/editor");
    const ms = await Promise.race([firstFrame, sleep(20000).then(() => -1)]);
    log.push(`mở trình biên tập → frame đầu: ${ms < 0 ? "quá 20 s" : `${Math.round(ms)} ms`}`);
    await sleep(800);
    await snap("main", "warm-editor");

    navigate("/");
    await sleep(1500);
    log.push(`close: ${JSON.stringify(await commands.projectClose(false))}`);
    log.push(`discard: ${JSON.stringify(await commands.recorderDiscard(dir))}`);
  } catch (e) {
    log.push(`error: ${String(e)}`);
  }
}

/** Vòng 4: xuất qua giao diện trên project TỔNG HỢP (recordings/round4/Export.mochi, do
 *  `mochi-cli export-scenario` tạo). Đặt in/out bằng phím I/O → Ctrl+E → Xuất → chờ xong. */
async function exportSnapshot(
  navigate: (to: string) => void,
  outDir: string,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  const key =(k: string, ctrl = false) => window.dispatchEvent(new KeyboardEvent("keydown", { key: k, ctrlKey: ctrl, bubbles: true }));
  const T = 705_600_000;
  try {
    const open = await commands.projectOpen(`${root}/round4/Export.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    navigate("/editor");
    await sleep(12000); // renderer của Cap mở lần đầu ~7 s
    await commands.playerSeek(Math.round(1.0 * T));
    await sleep(600);
    key("i");
    await commands.playerSeek(Math.round(4.0 * T));
    await sleep(600);
    key("o");
    await sleep(300);
    key("e", true);
    await sleep(2500);
    await snap("main", "10-export-dialog");
    // K-X1: lần chạy trước đã tạo file → hộp thoại báo trùng, tick "Ghi đè" rồi mới xuất được.
    const over = [...document.querySelectorAll<HTMLLabelElement>(".export-est label.check")].find((l) =>
      /ghi đè|overwrite/i.test(l.textContent ?? ""),
    );
    if (over) {
      log.push(`K-X1: ${document.querySelector(".export-est")?.textContent}`);
      over.querySelector("input")?.click();
      await sleep(1500);
      await snap("main", "10b-export-overwrite");
    }
    log.push(`ước tính: ${document.querySelector(".export-est")?.textContent}`);
    const caps = await commands.exportCapabilities();
    log.push(`caps: ${caps.map((c) => `${c.codec}=${c.encoder ?? "không"}`).join(", ")}`);
    // Bấm "Xuất" trong hộp thoại.
    const btn = [...document.querySelectorAll<HTMLButtonElement>(".dialog-actions button.primary")][0];
    if (!btn || btn.disabled) throw new Error("nút Xuất bị khóa");
    const done = new Promise<string>((resolve) => {
      const un = events.exportEvent.listen(({ payload }) => {
        if (payload.msg.type === "child" && (payload.msg.msg.type === "done" || payload.msg.msg.type === "failed")) {
          un.then((f) => f());
          resolve(JSON.stringify(payload.msg.msg));
        }
      });
    });
    btn.click();
    await sleep(1500);
    // K-X4: lần xuất thứ hai của CÙNG project khi lần đầu đang chạy → phải vào hàng đợi.
    const settings = await commands.exportDefaultDir().then((dir) => ({
      format: "gif" as const,
      codec: "h264" as const,
      shortSide: 360,
      fps: 10,
      quality: "standard" as const,
      range: { start: Math.round(1 * T), end: Math.round(3 * T) },
      output: `${dir}/Export-2.gif`,
      overwrite: true,
      allowBlackFill: false,
    }));
    const second = await commands.exportStart(settings);
    log.push(`K-X4 lần 2: ${second.status === "ok" ? second.data.state : JSON.stringify(second.error)}`);
    const secondDone = new Promise<string>((resolve) => {
      if (second.status !== "ok") return resolve("không chạy");
      const un = events.exportEvent.listen(({ payload }) => {
        if (payload.jobId !== second.data.jobId) return;
        if (payload.msg.type === "running") log.push("K-X4 lần 2: bắt đầu chạy (lần 1 đã xong)");
        if (payload.msg.type === "child" && (payload.msg.msg.type === "done" || payload.msg.msg.type === "failed")) {
          un.then((f) => f());
          resolve(JSON.stringify(payload.msg.msg));
        }
      });
    });
    await sleep(4500);
    await snap("main", "11-export-progress");
    const result = await Promise.race([done, sleep(180000).then(() => "quá 3 phút")]);
    log.push(`kết quả lần 1: ${result}`);
    log.push(`kết quả lần 2: ${await Promise.race([secondDone, sleep(180000).then(() => "quá 3 phút")])}`);
    await sleep(800);
    await snap("main", "12-export-done");
  } catch (e) {
    log.push(`error: ${String(e)}`);
  }
}

/**
 * Mở `recordings/fix1/Bg.mochi`, chọn clip đầu rồi chờ script ngoài điều khiển qua DevTools
 * (WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port) tới khi nó đặt `window.__cdpDone`.
 */
async function cdpSnapshot(navigate: (to: string) => void, outDir: string, log: string[], sleep: (ms: number) => Promise<unknown>) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  const w = window as unknown as { __cdpDone?: boolean; __cdpLog?: string[]; __mochiSelect?: (c: string | null, z: string | null) => void };
  try {
    const open = await commands.projectOpen(`${root}/fix1/Bg.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    navigate("/editor");
    await sleep(6000);
    w.__mochiSelect?.(open.data.project.clips[0].id, null);
    (window as unknown as { __cdpReady?: boolean }).__cdpReady = true;
    for (let i = 0; i < 600 && !w.__cdpDone; i++) await sleep(500);
    const v = await commands.projectView();
    const c = v?.project.clips[0];
    log.push(`fx: ${c && c.content.type === "recording" ? JSON.stringify(c.content.fx?.background) : "?"}`);
    log.push(...(w.__cdpLog ?? []));
    await commands.projectClose(true);
    navigate("/");
  } catch (e) {
    log.push(`cdp error: ${String(e)}`);
  }
  const s = await commands.settingsGet();
  await commands.settingsSet({ ...s, recentProjects: s.recentProjects.filter((p) => !/[\\/]recordings[\\/]/i.test(p)) });
}

/**
 * Lượt sửa sau khi người dùng thử installer: tắt "Nền & khung" lúc quay, clip cắt đôi dùng chung
 * hiệu ứng, xóa track (kể cả track còn chữ / làm mờ). Quay 64×64 góc màn hình, bỏ sau khi thử.
 */
async function fix2Snapshot(
  navigate: (to: string) => void,
  outDir: string,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
) {
  void outDir;
  void snap;
  const T = 705_600_000;
  const original = (await commands.settingsGet()).lastRecording;
  const kinds = async () =>
    ((await commands.projectView())?.project.clips ?? [])
      .filter((c) => c.content.type === "recording")
      .map((c) => (c.content.type === "recording" ? c.content.fx?.background.kind : "?"));
  const byText = <E extends Element>(sel: string, rx: RegExp) => [...document.querySelectorAll<E>(sel)].find((e) => rx.test(e.textContent ?? ""));
  const select = (c: string | null, z: string | null) =>
    (window as unknown as { __mochiSelect?: (c: string | null, z: string | null) => void }).__mochiSelect?.(c, z);
  let projectDir: string | null = null;
  try {
    // ---- màn thiết lập quay: ô "Nền & khung" ----
    navigate("/record");
    for (let i = 0; i < 40 && !byText("label.check", /nền & khung|background & frame/i); i++) await sleep(250);
    const frameBox = byText<HTMLLabelElement>("label.check", /nền & khung|background & frame/i)?.querySelector("input");
    log.push(`màn quay có ô nền & khung: ${frameBox ? `có (đang ${frameBox.checked ? "bật" : "tắt"})` : "KHÔNG"}`);
    if (frameBox?.checked) frameBox.click();
    await sleep(300);
    log.push(`sau khi bấm: ${frameBox?.checked ? "bật" : "tắt"}`);
    navigate("/");
    await sleep(800);
    const [display] = await commands.recorderListDisplays();
    const settings = await commands.settingsGet();
    const res = await commands.recorderStart({
      target: { kind: "area", displayId: display.id, x: 0, y: 0, width: 64, height: 64 },
      mic: null,
      camera: null,
      systemAudio: false,
      fps: 30,
      quality: "standard",
      countdownSecs: 0,
      keystrokes: "off",
      outputRoot: settings.outputRoot!,
      cursorHighlight: { enabled: false, color: "#FFD54A", size: 160, opacity: 45 },
      frame: false,
    });
    log.push(`start: ${res.status}`);
    await sleep(3000);
    const stop = await commands.recorderStop();
    if (stop.status === "ok") projectDir = stop.data.projectDir;
    for (let i = 0; i < 40 && !(await commands.projectView()); i++) await sleep(250);
    await sleep(2500);
    log.push(`clip mới (tắt nền & khung lúc quay): kind = ${JSON.stringify(await kinds())}`);
    log.push(`lastRecording.frame = ${(await commands.settingsGet()).lastRecording?.frame}`);

    // ---- cắt đôi clip rồi bật lại nền & khung trên một nửa ----
    const v0 = await commands.projectView();
    const clip0 = v0!.project.clips.find((c) => c.content.type === "recording")!;
    select(clip0.id, null);
    await commands.playerSeek(Math.round(1.0 * T));
    await sleep(800);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "s", bubbles: true }));
    await sleep(1200);
    const halves = (await commands.projectView())!.project.clips.filter((c) => c.content.type === "recording");
    log.push(`sau khi cắt: ${halves.length} clip bản quay`);
    select(halves[0].id, null);
    await sleep(1200);
    const toggle = byText<HTMLLabelElement>(".inspector label.check, label.check", /nền & khung \(|background & frame \(/i)?.querySelector("input");
    log.push(`ô nền & khung ở bảng thuộc tính: ${toggle ? `có (đang ${toggle.checked ? "bật" : "tắt"})` : "KHÔNG"}`);
    toggle?.click();
    await sleep(1500);
    log.push(`bật trên nửa đầu → kind cả hai nửa = ${JSON.stringify(await kinds())}`);

    // ---- xóa track ----
    const tracksNow = async () => (await commands.projectView())!.project.tracks.map((t) => `${t.kind}:${t.name}`);
    byText<HTMLButtonElement>("button", /^\+ (chữ|text)$/i)?.click();
    await sleep(800);
    byText<HTMLButtonElement>("button", /^\+ (làm mờ|blur)$/i)?.click();
    await sleep(800);
    byText<HTMLButtonElement>("button", /^\+ (track hình|video track)$/i)?.click();
    await sleep(800);
    log.push(`track sau khi thêm chữ, làm mờ, track hình: ${JSON.stringify(await tracksNow())}`);
    const headerOf = (rx: RegExp) => [...document.querySelectorAll<HTMLElement>(".tl-header")].find((h) => rx.test(h.querySelector(".tl-track-name")?.textContent ?? ""));
    const xOf = (h?: HTMLElement) => [...(h?.querySelectorAll<HTMLButtonElement>("button") ?? [])].find((b) => b.textContent === "✕");
    xOf(headerOf(/Video 2/))?.click();
    await sleep(800);
    log.push(`✕ trên track hình rỗng: ${document.querySelector(".dialog") ? "HIỆN HỘP THOẠI (sai)" : "xóa ngay"} → ${JSON.stringify(await tracksNow())}`);
    xOf(headerOf(/Overlay/))?.click();
    await sleep(800);
    log.push(`✕ trên track overlay: hộp thoại "${document.querySelector(".dialog")?.textContent ?? "(không có)"}"`);
    document.querySelector<HTMLButtonElement>(".dialog button.danger")?.click();
    await sleep(1000);
    log.push(`sau khi xác nhận: ${JSON.stringify(await tracksNow())}; clip chữ/làm mờ còn: ${(await commands.projectView())!.project.clips.filter((c) => c.content.type === "text" || c.content.type === "blur").length}`);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }));
    await sleep(1000);
    log.push(`Ctrl+Z: ${JSON.stringify(await tracksNow())}`);
    await commands.projectClose(true);
    navigate("/");
  } catch (e) {
    log.push(`fix2 error: ${String(e)}`);
  }
  if (projectDir) log.push(`bỏ bản quay thử: ${(await commands.recorderDiscard(projectDir)).status}`);
  const s = await commands.settingsGet();
  const dir = projectDir?.toLowerCase();
  await commands.settingsSet({ ...s, lastRecording: original, recentProjects: s.recentProjects.filter((p) => !dir || !p.toLowerCase().startsWith(dir)) });
}

/**
 * Sửa lỗi người dùng báo (2026-10-14): chỉnh nền / khoảng đệm / bo góc rồi xuất → video vẫn mặc định.
 * Quay 64×64 → trình biên tập → đổi nền → chờ → xuất vào thư mục snapshot.
 */
async function fix1Snapshot(
  navigate: (to: string) => void,
  outDir: string,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
) {
  const T = 705_600_000;
  const fxOf = async () => {
    const v = await commands.projectView();
    const c = v?.project.clips.find((x) => x.content.type === "recording");
    return { id: c?.id, fx: c && c.content.type === "recording" ? c.content.fx : null };
  };
  let projectDir: string | null = null;
  try {
    navigate("/");
    await sleep(1500);
    const [display] = await commands.recorderListDisplays();
    const settings = await commands.settingsGet();
    const res = await commands.recorderStart({
      target: { kind: "area", displayId: display.id, x: 0, y: 0, width: 64, height: 64 },
      mic: null,
      camera: null,
      systemAudio: false,
      fps: 30,
      quality: "standard",
      countdownSecs: 0,
      keystrokes: "off",
      outputRoot: settings.outputRoot!,
      cursorHighlight: settings.lastRecording?.cursorHighlight ?? { enabled: false, color: "#FFD54A", size: 160, opacity: 45 },
    });
    log.push(`start: ${res.status}`);
    await sleep(3000);
    const stop = await commands.recorderStop();
    if (stop.status === "ok") projectDir = stop.data.projectDir;
    for (let i = 0; i < 40 && !(await commands.projectView()); i++) await sleep(250);
    log.push(`route: ${location.hash}`);
    const first = await fxOf();
    log.push(`fx lúc mở: ${JSON.stringify(first.fx?.background)}`);
    // Chỉnh qua thanh trượt Khoảng đệm của bảng thuộc tính (đúng đường UI).
    (window as unknown as { __mochiSelect?: (c: string | null, z: string | null) => void }).__mochiSelect?.(first.id ?? null, null);
    await sleep(1500);
    const pad = [...document.querySelectorAll<HTMLLabelElement>("label.field")].find((l) => /khoảng đệm|padding/i.test(l.textContent ?? ""));
    const input = pad?.querySelector<HTMLInputElement>("input[type=range]");
    if (input) {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      setter.call(input, "0");
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await sleep(200);
      input.dispatchEvent(new PointerEvent("pointerup", { bubbles: true }));
    }
    log.push(`thanh trượt padding: ${input ? "có" : "KHÔNG THẤY"}`);
    await sleep(1000);
    log.push(`fx sau 1 s: ${JSON.stringify((await fxOf()).fx?.background)}`);
    await sleep(10000);
    log.push(`fx sau 11 s: ${JSON.stringify((await fxOf()).fx?.background)}`);
    await snap("main", "fix1-editor");
    const done = new Promise<string>((resolve) => {
      const unl = events.exportEvent.listen(({ payload }) => {
        if (payload.msg.type === "child" && (payload.msg.msg.type === "done" || payload.msg.msg.type === "failed")) {
          unl.then((f) => f());
          resolve(JSON.stringify(payload.msg.msg));
        }
      });
    });
    const ex = await commands.exportStart({
      format: "mp4",
      codec: "h264",
      shortSide: 360,
      fps: 30,
      quality: "standard",
      range: { start: 0, end: Math.round(1.5 * T) },
      output: `${outDir}/fix1.mp4`,
      overwrite: true,
      allowBlackFill: false,
    });
    log.push(`export: ${ex.status === "ok" ? ex.data.state : JSON.stringify(ex.error)}`);
    log.push(`kết quả: ${await Promise.race([done, sleep(120000).then(() => "quá 2 phút")])}`);
    await commands.projectClose(true);
    navigate("/");
  } catch (e) {
    log.push(`fix1 error: ${String(e)}`);
  }
  if (projectDir) log.push(`bỏ bản quay thử: ${(await commands.recorderDiscard(projectDir)).status}`);
  const s = await commands.settingsGet();
  const dir = projectDir?.toLowerCase();
  await commands.settingsSet({ ...s, recentProjects: s.recentProjects.filter((p) => !dir || !p.toLowerCase().startsWith(dir)) });
}

/**
 * Phase 5 (R13 / E06): highlight con trỏ. Chuẩn bị bên ngoài: `recordings/phase5/Hl.mochi`
 * (do `mochi-cli phase5-highlight` tạo, clip đã bật highlight).
 */
async function phase5Snapshot(
  navigate: (to: string) => void,
  outDir: string,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  const hideScreens = () => document.querySelectorAll(".thumb img, .display-card img").forEach((i) => i.remove());
  const original = (await commands.settingsGet()).lastRecording;
  try {
    // ---- R13: màn thiết lập quay ----
    navigate("/record");
    for (let i = 0; i < 40 && !document.querySelector(".highlight-controls"); i++) await sleep(250);
    const box = document.querySelector<HTMLInputElement>(".highlight-controls input[type=checkbox]");
    if (box && !box.checked) box.click();
    await sleep(800);
    log.push(`R13 màn thiết lập: ${document.querySelector(".highlight-controls")?.textContent}`);
    hideScreens();
    await snap("main", "p5-1-record-highlight");

    // ---- quay 64×64 với highlight → dừng → clip mới có highlight ----
    navigate("/");
    await sleep(800);
    const [display] = await commands.recorderListDisplays();
    const settings = await commands.settingsGet();
    await commands.recorderStart({
      target: { kind: "area", displayId: display.id, x: 0, y: 0, width: 64, height: 64 },
      mic: null,
      camera: null,
      systemAudio: false,
      fps: 30,
      quality: "standard",
      countdownSecs: 0,
      keystrokes: "off",
      outputRoot: settings.outputRoot!,
      cursorHighlight: { enabled: true, color: "#4FC3F7", size: 220, opacity: 50 },
    });
    await sleep(3000);
    const stop = await commands.recorderStop();
    await sleep(4000);
    const v = await commands.projectView();
    const rec = v?.project.clips.find((c) => c.content.type === "recording");
    log.push(
      `R13 dừng quay → trang ${location.hash}; highlight của clip mới: ${rec && rec.content.type === "recording" ? JSON.stringify(rec.content.fx?.cursor.highlight) : "(không có clip)"}`,
    );
    log.push(`lastRecording nhớ highlight: ${JSON.stringify((await commands.settingsGet()).lastRecording?.cursorHighlight)}`);
    await commands.projectClose(true);
    navigate("/");
    if (stop.status === "ok") log.push(`bỏ bản quay thử: ${(await commands.recorderDiscard(stop.data.projectDir)).status}`);

    // ---- E06: preview + bảng thuộc tính trên bản quay tổng hợp ----
    const open = await commands.projectOpen(`${root}\\phase5\\Hl.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    navigate("/editor");
    await sleep(9000);
    const clip = open.data.project.clips[0];
    await commands.playerSeek(Math.round(1.0 * 705_600_000));
    (window as unknown as { __mochiSelect?: (c: string | null, z: string | null) => void }).__mochiSelect?.(clip.id, null);
    await sleep(2500);
    log.push(`E06 bảng thuộc tính: ${document.querySelector(".highlight-controls")?.textContent ?? "(không thấy)"}`);
    document.querySelector(".highlight-controls")?.scrollIntoView();
    await sleep(500);
    await snap("main", "p5-2-editor-highlight");
    await commands.projectClose(true);
    navigate("/");
  } catch (e) {
    log.push(`phase5 error: ${String(e)}`);
  }
  // Trả lại cấu hình quay lần trước của người dùng + dọn danh sách gần đây.
  const s = await commands.settingsGet();
  const kept = s.recentProjects.filter((p) => !/[\\/]recordings[\\/]/i.test(p));
  await commands.settingsSet({ ...s, lastRecording: original, recentProjects: kept });
}

/**
 * Phase 4 lượt 2, đợt C (hệ thống). Chuẩn bị bên ngoài: `recordings/phase4/thumb/{good,bad}.mp4`
 * (bad: đọc được thông tin nhưng hỏng dữ liệu hình); app chạy với `MOCHI_TEST_GPU_DRIVER` = driver cũ.
 */
async function patch2cSnapshot(
  navigate: (to: string) => void,
  outDir: string,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  const hideScreens = () => document.querySelectorAll(".thumb img, .display-card img").forEach((i) => i.remove());
  try {
    // ---- GPU / driver cũ: báo ở trang chủ và màn thiết lập quay ----
    const gpu = await commands.systemGpuCheck();
    log.push(`GPU: ok=${gpu.ok}, oldDriver=${gpu.oldDriver}, driver="${gpu.driver}"`);
    navigate("/record");
    await sleep(3500);
    log.push(`trang thiết lập quay: ${document.querySelector(".warn-box")?.textContent ?? "(không có cảnh báo)"}`);
    hideScreens();
    await snap("main", "p2c-1-record-gpu");
    navigate("/");
    await sleep(1500);

    // ---- cache: đổi giới hạn ----
    const before = await commands.cacheInfo();
    const r = await commands.cacheSetLimit(5);
    log.push(`cache: giới hạn ${before.limitGb} → ${r.status === "ok" ? r.data.limitGb : r.error} GB`);
    await commands.cacheSetLimit(before.limitGb);

    // ---- S03: đang quay → hoãn; quay xong → làm nốt; lỗi → đánh dấu + thử lại ----
    const open = await commands.projectOpen(`${root}\\phase4\\Thumb.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    navigate("/editor");
    await sleep(4000);
    const [display] = await commands.recorderListDisplays();
    const settings = await commands.settingsGet();
    await commands.recorderStart({
      target: { kind: "area", displayId: display.id, x: 0, y: 0, width: 64, height: 64 },
      mic: null,
      camera: null,
      systemAudio: false,
      fps: 30,
      quality: "standard",
      countdownSecs: 0,
      keystrokes: "off",
      outputRoot: settings.outputRoot!,
    });
    await sleep(1500);
    const imp = (window as unknown as { __mochiImport: (p: string[]) => Promise<void> }).__mochiImport;
    await imp([`${root}\\phase4\\thumb\\good.mp4`, `${root}\\phase4\\thumb\\bad.mp4`]);
    await sleep(2500);
    const v = (await commands.projectView())!;
    const good = v.project.media.find((m) => m.name === "good.mp4")!;
    const direct = await commands.mediaThumbnail(good.id);
    log.push(`đang quay: ảnh thu nhỏ good.mp4 trong media bin=${document.querySelectorAll(".media-thumb img").length} · gọi trực tiếp → ${JSON.stringify(direct)}`);
    await commands.recorderCancel();
    await sleep(5000);
    log.push(`quay xong (hủy): ảnh thu nhỏ hiện=${document.querySelectorAll(".media-thumb img").length} · huy hiệu lỗi: ${[...document.querySelectorAll(".media-item .missing-badge")].map((b) => b.textContent).join(" | ") || "(không)"}`);
    await snap("main", "p2c-2-media-tasks");
    const retry = [...document.querySelectorAll<HTMLButtonElement>(".media-item .missing-badge button")][0];
    retry?.click();
    await sleep(3000);
    log.push(`bấm Thử lại (file vẫn hỏng): huy hiệu lỗi còn ${document.querySelectorAll(".media-item .missing-badge").length}`);
    await commands.projectClose(true);
    navigate("/");
  } catch (e) {
    log.push(`patch2c error: ${String(e)}`);
  }
  const s = await commands.settingsGet();
  const kept = s.recentProjects.filter((p) => !/[\\/]recordings[\\/]/i.test(p));
  await commands.settingsSet({ ...s, recentProjects: kept });
}

/**
 * Phase 4 lượt 2, đợt B (giao diện biên tập). Chuẩn bị bên ngoài: `recordings/phase4/Lost.mochi`
 * (bản quay tổng hợp) có `.lock` ghi "state=open" như sau crash.
 */
async function patch2bSnapshot(
  navigate: (to: string) => void,
  outDir: string,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  const lost = `${root}\\phase4\\Lost.mochi`;
  const button = (re: RegExp) => [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) => re.test(b.textContent ?? ""));
  try {
    // ---- S02: trang chủ báo project chưa đóng sạch ----
    const unclean = await commands.projectsUnclean();
    log.push(`S02 projectsUnclean: ${JSON.stringify(unclean.map((d) => d.split(/[\\/]/).pop()))}`);
    navigate("/");
    await sleep(2500);
    log.push(`S02 khung trên trang chủ: ${document.querySelector(".warn-box")?.textContent ?? "(không có)"}`);

    // ---- mở lại sau crash → hộp thoại hỏi ----
    const open = await commands.projectOpen(lost);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    log.push(`mở: recovered=${open.data.recovered}`);
    navigate("/editor");
    await sleep(8000);
    log.push(`hộp thoại khôi phục: ${document.querySelector(".dialog")?.textContent ?? "(không có)"}`);
    await snap("main", "p2b-1-recovered");
    button(/Giữ bản tự lưu|Keep the autosave/)?.click();
    await sleep(800);

    // ---- dữ liệu cho timeline: zoom, ẩn webcam, vị trí tùy chỉnh ----
    let v = (await commands.projectView())!;
    const rec = v.project.clips.find((c) => c.content.type === "recording")!;
    const T = 705_600_000;
    if (rec.content.type === "recording" && rec.content.fx) {
      const fx = structuredClone(rec.content.fx);
      fx.camera.hiddenRanges = [{ start: Math.round(1 * T), end: Math.round(2.5 * T) }];
      fx.camera.position = { x: 0.3, y: 0.7 };
      await commands.timelineApply({ op: "setRecordingFx", clipId: rec.id, fx });
    }
    await commands.timelineApply({ op: "addZoom", clipId: rec.id, start: Math.round(3 * T), end: Math.round(4 * T), amount: 2.5 });
    v = (await commands.projectView())!;
    // Lệnh gửi thẳng không đi qua Editor → mở lại trang để nạp project mới.
    navigate("/");
    await sleep(600);
    navigate("/editor");
    await sleep(4000);
    (window as unknown as { __mochiSelect?: (c: string | null, z: string | null) => void }).__mochiSelect?.(rec.id, null);
    await sleep(2500);
    const zoomRow = document.querySelector(".tl-zoom-row");
    log.push(
      `E05 dải zoom riêng: ${zoomRow ? `có, ${zoomRow.querySelectorAll(".tl-zoom").length} khối` : "KHÔNG"}; khối zoom còn trong clip: ${document.querySelectorAll(".tl-clip .tl-zoom").length}`,
    );
    log.push(`E08 dải ẩn webcam: ${document.querySelectorAll(".tl-camhide").length}; danh sách sửa được: ${document.querySelectorAll(".hidden-ranges li").length} dòng`);
    log.push(`E08 ô vị trí: ${[...document.querySelectorAll("select")].map((s) => s.value).find((x) => x === "custom") ?? "(không thấy custom)"}`);
    log.push(`E11 thanh âm lượng track: ${document.querySelectorAll(".tl-track-vol input").length}`);
    log.push(`Thanh trượt có hiển thị giới hạn: ${document.querySelector(".slider-range")?.textContent ?? "(không)"}`);
    await snap("main", "p2b-2-editor");

    // ---- S03: tắt / bật proxy ----
    const box = [...document.querySelectorAll<HTMLLabelElement>("label.check")].find((l) => /Proxy/.test(l.textContent ?? ""));
    box?.querySelector("input")?.click();
    await sleep(800);
    log.push(`S03 tắt proxy → settings.useProxies=${(await commands.settingsGet()).useProxies}`);
    box?.querySelector("input")?.click();
    await sleep(800);
    log.push(`S03 bật lại → settings.useProxies=${(await commands.settingsGet()).useProxies}`);

    await commands.projectClose(true);
    navigate("/");
  } catch (e) {
    log.push(`patch2b error: ${String(e)}`);
  }
  // Dọn: bỏ các project thử khỏi danh sách "gần đây" của người dùng.
  const s = await commands.settingsGet();
  const kept = s.recentProjects.filter((p) => !/[\\/]recordings[\\/]/i.test(p));
  await commands.settingsSet({ ...s, recentProjects: kept });
  log.push(`dọn danh sách gần đây: ${s.recentProjects.length} → ${kept.length}`);
}

/**
 * Phase 4 lượt 2, đợt A (Người quay). Chuẩn bị bên ngoài: một tiến trình giữ phím F9 (giả lập app
 * khác chiếm phím); `recordings/phase4/Lost.mochi` do `mochi-cli fake-recording` tạo (có sidecar).
 * Không chụp ảnh nào có màn hình thật: ảnh thu nhỏ màn hình bị gỡ khỏi trang trước khi chụp.
 */
async function patch2aSnapshot(
  navigate: (to: string) => void,
  outDir: string,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
  seen: RecorderEvent[],
) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  const hideScreens = () => document.querySelectorAll(".thumb img, .display-card img").forEach((i) => i.remove());
  const button = (re: RegExp) => [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) => re.test(b.textContent ?? ""));
  const original = (await commands.settingsGet()).hotkeys;
  try {
    // ---- R02: vạch âm lượng âm thanh hệ thống ----
    navigate("/record");
    await sleep(3000);
    const lv = await commands.systemAudioLevel();
    log.push(`R02 âm thanh hệ thống: ${lv.status === "ok" ? `thiết bị="${lv.data.device}", peak=${lv.data.peak.toFixed(4)}` : lv.error}`);
    log.push(`R02 dòng thiết bị trên trang: ${[...document.querySelectorAll(".muted.small")].map((e) => e.textContent).find((s) => /Thiết bị phát|Playback device/.test(s ?? "")) ?? "(không thấy)"}`);

    // ---- R01: nhập tay vùng quay ----
    button(/^(Vùng|Area)$/)!.click();
    await sleep(800);
    const inputs = [...document.querySelectorAll<HTMLInputElement>(".area-inputs input")];
    const setInput = (i: number, v: number) => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(inputs[i], String(v));
      inputs[i].dispatchEvent(new Event("input", { bubbles: true }));
      inputs[i].dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
      inputs[i].blur();
    };
    log.push(`R01 ô nhập: ${inputs.length} ô, mặc định ${inputs.map((i) => i.value).join(" / ")}`);
    inputs[2].focus();
    setInput(2, 1920);
    await sleep(300);
    const after = [...document.querySelectorAll<HTMLInputElement>(".area-inputs input")];
    after[3].focus();
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(after[3], "99999");
    after[3].dispatchEvent(new Event("input", { bubbles: true }));
    after[3].dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    await sleep(500);
    log.push(`R01 sau khi nhập W=1920, H=99999 (kẹp theo màn hình): ${[...document.querySelectorAll<HTMLInputElement>(".area-inputs input")].map((i) => i.value).join(" / ")}`);
    log.push(`R01 dòng tóm tắt: ${[...document.querySelectorAll("p.muted")].map((p) => p.textContent).find((s) => /×/.test(s ?? ""))}`);
    log.push(`R01 nút Bắt đầu quay: ${button(/Bắt đầu quay|Start recording/)?.disabled ? "khóa" : "mở"}`);
    hideScreens();
    await snap("main", "p2a-1-area-inputs");

    // ---- vùng nằm ngoài màn hình → từ chối ----
    const [display] = await commands.recorderListDisplays();
    const settings = await commands.settingsGet();
    const out = await commands.recorderStart({
      target: { kind: "area", displayId: display.id, x: display.logicalWidth - 20, y: 0, width: 200, height: 100 },
      mic: null,
      camera: null,
      systemAudio: false,
      fps: 30,
      quality: "standard",
      countdownSecs: 0,
      keystrokes: "off",
      outputRoot: settings.outputRoot!,
    });
    log.push(`Vùng tràn ra ngoài màn hình: ${out.status === "error" ? `từ chối (${out.error.code})` : "ĐÃ QUAY (sai)"}`);

    // ---- R04: phím bị app khác giữ → gợi ý; phím trùng / sai → từ chối ----
    log.push(`R04 đăng ký lại: ${JSON.stringify(await commands.hotkeysApply())}`);
    const dup = await commands.hotkeysSet({ recordPause: "F10", stop: "F10", cancel: "Shift+F10" });
    log.push(`R04 hai thao tác cùng F10: ${dup.status === "error" ? `từ chối (${dup.error})` : "ĐÃ LƯU (sai)"}`);
    const bad = await commands.hotkeysSet({ recordPause: "Ctrl+Banana", stop: "F10", cancel: "Shift+F10" });
    log.push(`R04 phím sai cú pháp: ${bad.status === "error" ? `từ chối (${bad.error})` : "ĐÃ LƯU (sai)"}`);
    log.push(`R04 settings sau 2 lần từ chối: ${JSON.stringify((await commands.settingsGet()).hotkeys)}`);
    navigate("/");
    await sleep(500);
    navigate("/record");
    for (let i = 0; i < 40 && !document.querySelector(".record-footer .warn"); i++) await sleep(250);
    log.push(`R04 dòng báo trùng: ${document.querySelector(".record-footer .warn")?.textContent ?? "(không có)"}`);
    button(/Chọn phím khác|Pick another key|Đổi phím tắt|Change shortcuts/)?.click();
    await sleep(1000);
    hideScreens();
    log.push(`R04 hộp thoại: ${document.querySelector(".dialog")?.textContent}`);
    await snap("main", "p2a-2-hotkeys-conflict");
    const use = button(/^(Dùng|Use) /);
    log.push(`R04 nút gợi ý: ${use?.textContent ?? "(không có)"}`);
    use?.click();
    await sleep(1500);
    log.push(`R04 sau khi dùng gợi ý: hộp thoại ${document.querySelector(".dialog") ? "còn" : "đã đóng"}, settings=${JSON.stringify((await commands.settingsGet()).hotkeys)}, trùng=${JSON.stringify(await commands.hotkeysApply())}`);
  } catch (e) {
    log.push(`R01/R02/R04 error: ${String(e)}`);
  }
  const restored = await commands.hotkeysSet(original);
  log.push(`khôi phục phím tắt gốc: ${JSON.stringify((await commands.settingsGet()).hotkeys)} (${restored.status})`);

  // ---- R08 + R03: quay 64×64 → dừng → mở thẳng trình biên tập; cửa sổ active được ghi ----
  try {
    navigate("/");
    await sleep(800);
    const [display] = await commands.recorderListDisplays();
    const settings = await commands.settingsGet();
    seen.length = 0;
    await commands.recorderStart({
      target: { kind: "area", displayId: display.id, x: 0, y: 0, width: 64, height: 64 },
      mic: null,
      camera: null,
      systemAudio: true,
      fps: 30,
      quality: "standard",
      countdownSecs: 0,
      keystrokes: "off",
      outputRoot: settings.outputRoot!,
    });
    await sleep(3500);
    const stop = await commands.recorderStop();
    await sleep(4000);
    log.push(`R08 dừng: ${stop.status} → trang: ${location.hash}`);
    log.push(`cảnh báo khi quay: ${JSON.stringify(seen.filter((e) => e.type === "warning"))}`);
    const v = await commands.projectView();
    const rec = v?.project.media[0]?.recording;
    log.push(
      `R03 cửa sổ active ghi được: ${rec?.activeWindows?.length ?? 0} lần đổi; app: ${[...new Set((rec?.activeWindows ?? []).map((w) => w.app))].join(", ")} (tiêu đề không ghi vào báo cáo); mất thiết bị: ${JSON.stringify(rec?.lost ?? [])}`,
    );
    log.push(`close: ${(await commands.projectClose(false)).status}`);
    navigate("/");
    if (stop.status === "ok") log.push(`discard: ${(await commands.recorderDiscard(stop.data.projectDir)).status}`);
  } catch (e) {
    log.push(`R08 error: ${String(e)}`);
  }

  // ---- R07: khoảng mất webcam hiện trên clip (bản quay tổng hợp) ----
  try {
    const open = await commands.projectOpen(`${root}/phase4/Lost.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    log.push(`R07 lost: ${JSON.stringify(open.data.project.media[0]?.recording?.lost)}`);
    navigate("/editor");
    await sleep(9000);
    const band = document.querySelector(".tl-lost");
    log.push(`R07 dải trên timeline: ${band ? `có, title="${(band as HTMLElement).title}"` : "KHÔNG"}`);
    await snap("main", "p2a-3-lost-band");
    await commands.projectClose(false);
    navigate("/");
  } catch (e) {
    log.push(`R07 error: ${String(e)}`);
  }
}

/**
 * Phase 4 lượt 1. Chuẩn bị bên ngoài (script kiểm thử):
 *  - `<Videos>\Mochi Studio\Export.mp4` và `Export-B.mp4` có sẵn (file giả) cho K-X1;
 *  - một tiến trình khóa `<outDir>\SaveFail.mochi\project.json` ~1 s sau khi file xuất hiện, giữ 30 s (S01).
 */
async function patch1Snapshot(
  navigate: (to: string) => void,
  outDir: string,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  const key = (k: string, ctrl = false) => window.dispatchEvent(new KeyboardEvent("keydown", { key: k, ctrlKey: ctrl, bubbles: true }));
  const est = () => document.querySelector(".export-est")?.textContent?.trim() ?? "(không có)";
  const overwriteBox = () =>
    [...document.querySelectorAll<HTMLLabelElement>(".export-est label")].find((l) => /ghi đè|overwrite/i.test(l.textContent ?? ""));
  const button = (re: RegExp) => [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) => re.test(b.textContent ?? ""));
  const setPath = (value: string) => {
    const input = document.querySelector<HTMLInputElement>(".export-path input")!;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const dir = await commands.exportDefaultDir();

  // ---- K-X1 (1): đồng ý ghi đè gắn với đúng một đường dẫn ----
  try {
    const open = await commands.projectOpen(`${root}/round4/Export.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    navigate("/editor");
    await sleep(8000);
    key("e", true);
    await sleep(2500);
    log.push(`K-X1 đường dẫn A: ${document.querySelector<HTMLInputElement>(".export-path input")?.value}`);
    log.push(`K-X1 A, chưa tick: ${est()}`);
    overwriteBox()?.querySelector("input")?.click();
    await sleep(1500);
    log.push(`K-X1 A, đã tick: ô ghi đè=${overwriteBox()?.querySelector("input")?.checked} · ${est()}`);
    await snap("main", "p1-1-kx1-A-ticked");
    setPath(`${dir}\\Export-B.mp4`);
    await sleep(1500);
    log.push(`K-X1 gõ tay sang B (đã có): ô ghi đè=${overwriteBox()?.querySelector("input")?.checked ?? "ẩn"} · nút Xuất khóa=${
      [...document.querySelectorAll<HTMLButtonElement>(".dialog-actions button.primary")][0]?.disabled
    } · ${est()}`);
    await snap("main", "p1-2-kx1-B-retyped");
    setPath(`${dir}\\Export.mp4`);
    await sleep(1500);
    log.push(`K-X1 quay lại A: ô ghi đè=${overwriteBox()?.querySelector("input")?.checked ?? "ẩn"} · ${est()}`);
    key("Escape");
    await sleep(500);

    // ---- K-X1 (2): "Thử lại" không tự ghi đè ----
    const settings = {
      format: "mp4" as const,
      codec: "h264" as const,
      shortSide: 360,
      fps: 30,
      quality: "standard" as const,
      range: null,
      output: `${dir}\\Export-B.mp4`,
      overwrite: false,
      allowBlackFill: false,
    };
    const failedJob: ExportJobView = {
      ...(null as unknown as ExportJobView),
      jobId: "gia-lap-loi",
      name: "Export",
      output: settings.output,
      settings,
      state: "failed",
      error: { code: "crashed", log: "" } as unknown as ExportError,
    };
    const r1 = await retryExport(failedJob);
    log.push(`K-X1 Thử lại (job cũ không tick ghi đè, file B đã có): ${r1 ? `bị chặn: ${r1.code}` : "ĐÃ CHẠY (sai)"}`);
    const started = new Promise<string>((resolve) => {
      const unl = events.exportEvent.listen(({ payload }) => {
        unl.then((f) => f());
        resolve(payload.jobId);
      });
      setTimeout(() => resolve(""), 4000);
    });
    const r2 = await retryExport(failedJob, true);
    const jobId = await started;
    log.push(`K-X1 "Ghi đè và thử lại" (xác nhận rõ): ${r2 ? `lỗi ${r2.code}` : "đã chạy"} → hủy ngay ${jobId}`);
    if (jobId) await commands.exportCancel(jobId);
    await sleep(1500);
    const close = await commands.projectClose(false);
    log.push(`đóng Export.mochi: ${close.status}`);
    navigate("/");
    await sleep(1000);
  } catch (e) {
    log.push(`K-X1 error: ${String(e)}`);
  }

  // ---- S01: đóng khi không lưu được (project.json bị khóa 30 s) ----
  try {
    const t0 = performance.now();
    const at = () => `${((performance.now() - t0) / 1000).toFixed(1)}s`;
    const open = await commands.projectOpen(`${outDir}/SaveFail.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    await sleep(3500); // tiến trình ngoài đã khóa project.json
    await commands.timelineApply({ op: "addTrack", kind: "video" });
    navigate("/editor");
    await sleep(4000);
    document.querySelector<HTMLButtonElement>(".editor-top button.icon-link")!.click();
    await sleep(1500);
    log.push(`[${at()}] bấm ← khi file bị khóa: route=${location.hash} · hộp thoại="${document.querySelector(".dialog h3")?.textContent ?? "không"}"`);
    log.push(`[${at()}] nội dung: ${document.querySelector(".dialog .err")?.textContent}`);
    await snap("main", "p1-3-s01-close-failed");
    button(/Thử lưu lại|Try saving/)!.click();
    await sleep(1500);
    log.push(`[${at()}] Thử lưu lại (vẫn khóa): hộp thoại còn=${!!document.querySelector(".dialog")} · project còn mở=${(await commands.projectView()) !== null}`);
    button(/^Ở lại$|^Stay$/)!.click();
    await sleep(800);
    log.push(`[${at()}] Ở lại: route=${location.hash} · chưa lưu=${(await commands.projectView())?.dirty}`);

    await commands.devSnapshotCloseMain();
    await sleep(1500);
    log.push(`[${at()}] bấm × cửa sổ khi file bị khóa: app vẫn chạy · hộp thoại="${document.querySelector(".dialog h3")?.textContent ?? "không"}" · nút="${button(/thoát|quit/i)?.textContent}"`);
    await snap("main", "p1-4-s01-exit-failed");
    button(/^Ở lại$|^Stay$/)!.click();
    await sleep(800);

    while (performance.now() - t0 < 36000) await sleep(500); // khóa được nhả ở ~31 s
    document.querySelector<HTMLButtonElement>(".editor-top button.icon-link")!.click();
    await sleep(1500);
    log.push(`[${at()}] bấm ← sau khi nhả khóa: route=${location.hash} · project đang mở=${(await commands.projectView()) !== null}`);
    const reopen = await commands.projectOpen(`${outDir}/SaveFail.mochi`);
    log.push(`mở lại: số track=${reopen.status === "ok" ? reopen.data.project.tracks.length : reopen.status} (thêm 1 track trước khi khóa → phải còn)`);
    await commands.projectClose(false);
  } catch (e) {
    log.push(`S01 error: ${String(e)}`);
  }
}

/** Vòng 5 / K-S4: mở project có video 4K (recordings/round5/Jobs.mochi, do `mochi-cli jobs-scenario`
 *  tạo) → proxy bắt đầu → quay vùng 64×64 → proxy phải tạm hoãn → dừng + bỏ bản quay → chạy tiếp. */
async function jobsSnapshot(
  navigate: (to: string) => void,
  outDir: string,
  snap: (label: string, name: string) => Promise<void>,
  log: string[],
  sleep: (ms: number) => Promise<unknown>,
) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  const t0 = performance.now();
  const seen: string[] = [];
  const un = await events.jobsUpdate.listen(({ payload }) => {
    const s = payload.status;
    const label = s.state === "running" ? `running ${Math.round(s.progress * 100)}%` : s.state;
    if (seen[seen.length - 1]?.endsWith(label.split(" ")[0]) && s.state === "running") return;
    seen.push(`${((performance.now() - t0) / 1000).toFixed(1)}s ${label}`);
  });
  try {
    const open = await commands.projectOpen(`${root}/round5/Jobs.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    navigate("/editor");
    await sleep(4000);
    await snap("main", "13-proxy-running");
    const [display] = await commands.recorderListDisplays();
    const settings = await commands.settingsGet();
    const start = await commands.recorderStart({
      target: { kind: "area", displayId: display.id, x: 0, y: 0, width: 64, height: 64 },
      mic: null,
      camera: null,
      systemAudio: false,
      fps: 30,
      quality: "standard",
      countdownSecs: 0,
      keystrokes: "off",
      outputRoot: settings.outputRoot!,
    });
    log.push(`bắt đầu quay: ${start.status}`);
    await sleep(4000);
    log.push(`trong lúc quay: ${JSON.stringify(Object.values(await commands.jobsStatus()).map((s) => s.state))}`);
    const stop = await commands.recorderStop();
    if (stop.status === "ok") log.push(`bỏ bản quay: ${(await commands.recorderDiscard(stop.data.projectDir)).status}`);
    navigate("/editor");
    for (let i = 0; i < 120; i++) {
      await sleep(1000);
      if (Object.values(await commands.jobsStatus()).every((s) => s.state === "ready" || s.state === "failed")) break;
    }
    await sleep(1500);
    await snap("main", "14-proxy-ready");
    log.push(`kết thúc: ${JSON.stringify(await commands.jobsStatus())}`);
  } catch (e) {
    log.push(`error: ${String(e)}`);
  }
  un();
  log.push(...seen.map((x) => `  ${x}`));
}
