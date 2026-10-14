import { commands, events } from "../bindings";

/**
 * Bài tự kiểm Phase 6 (use case v1) trong app — chỉ bản dev (MOCHI_UI_SNAPSHOT_MODE=v1a…).
 * Dùng project tổng hợp `recordings/fix1/Bg.mochi` (bản quay giả, không có màn hình thật).
 * Hộp thoại lưu / mở được bỏ qua bằng MOCHI_TEST_DIALOG_DIR / MOCHI_TEST_DIALOG_PICK.
 */
type Ctx = {
  navigate: (to: string) => void;
  outDir: string;
  snap: (label: string, name: string) => Promise<void>;
  log: string[];
  sleep: (ms: number) => Promise<unknown>;
};

const T = 705_600_000;
const byText = <E extends Element>(sel: string, rx: RegExp) => [...document.querySelectorAll<E>(sel)].find((e) => rx.test(e.textContent ?? ""));
const select = (c: string | null) =>
  (window as unknown as { __mochiSelect?: (c: string | null, z: string | null) => void }).__mochiSelect?.(c, null);
const key = (k: string, ctrl = false) => window.dispatchEvent(new KeyboardEvent("keydown", { key: k, ctrlKey: ctrl, bubbles: true }));
const setInput = (el: HTMLInputElement, v: string) => {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, v);
  el.dispatchEvent(new Event("input", { bubbles: true }));
};

export async function runV1(mode: string, c: Ctx) {
  if (mode === "v1a") await roundA(c);
  if (mode === "v1r") await presetDebug(c);
  if (mode === "v1b") await roundB(c);
  if (mode === "v1c") await roundC(c);
  if (mode === "v1d") await roundD(c);
  if (mode === "v1e") await roundE(c);
}

/** Kéo một thanh trượt (theo nhãn) tới giá trị `v` rồi "thả" (sự kiện change). */
function slide(label: RegExp, v: number, scope: ParentNode = document) {
  const l = [...scope.querySelectorAll<HTMLLabelElement>("label.field")].find((x) => label.test(x.querySelector("span")?.textContent ?? ""));
  const input = l?.querySelector<HTMLInputElement>("input[type=range]");
  if (!input) return false;
  setInput(input, String(v));
  input.dispatchEvent(new Event("change", { bubbles: true }));
  return true;
}

function choose(label: RegExp, value: string, scope: ParentNode = document) {
  const l = [...scope.querySelectorAll<HTMLLabelElement>("label.field")].find((x) => label.test(x.querySelector("span")?.textContent ?? ""));
  const sel = l?.querySelector<HTMLSelectElement>("select");
  if (!sel) return false;
  sel.value = value;
  sel.dispatchEvent(new Event("change", { bubbles: true }));
  return true;
}

function openSection(title: RegExp) {
  const s = [...document.querySelectorAll<HTMLDetailsElement>("details.insp-section")].find((d) => title.test(d.querySelector("summary")?.textContent ?? ""));
  if (s) s.open = true;
  return s ?? null;
}

async function roundB({ navigate, outDir, snap, log, sleep }: Ctx) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  try {
    const open = await commands.projectOpen(`${root}/phase6/render/V1.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    navigate("/editor");
    await sleep(6000);
    const view = async () => (await commands.projectView())!;
    const p0 = (await view()).project;
    const first = p0.clips.find((c) => c.content.type === "video" && c.start === 0)!;
    select(first.id);
    await commands.playerSeek(Math.round(0.5 * T));
    await sleep(1200);
    // ---- E12: xoay tĩnh → keyframe ----
    const motion = openSection(/chuyển động|motion/i);
    await sleep(300);
    log.push(`E12 mục chuyển động: ${motion ? "có" : "KHÔNG"}`);
    slide(/xoay|rotation/i, 12, motion ?? document);
    await sleep(900);
    log.push(`E12 xoay tĩnh: ${JSON.stringify((await view()).project.clips.find((c) => c.id === first.id)?.transform)}`);
    byText<HTMLButtonElement>("button", /thêm keyframe|add keyframe/i)?.click();
    await sleep(900);
    await commands.playerSeek(Math.round(3.0 * T));
    await sleep(900);
    slide(/^(ngang|horizontal)/i, 0.3, openSection(/chuyển động|motion/i) ?? document);
    slide(/xoay|rotation/i, -20, openSection(/chuyển động|motion/i) ?? document);
    await sleep(900);
    const kf = (await view()).project.clips.find((c) => c.id === first.id)?.keyframes ?? [];
    log.push(`E12 keyframe: ${JSON.stringify(kf.map((k) => [+(k.at / T).toFixed(2), +k.x.toFixed(2), Math.round(k.rotation), k.easing]))}`);
    log.push(`E12 ◆ trên timeline: ${document.querySelectorAll(".tl-kf").length}`);
    // ---- E12: chuyển cảnh qua ô chọn ----
    const trs = openSection(/chuyển cảnh|transitions/i);
    await sleep(300);
    choose(/^vào$|^in$/i, "zoom", trs ?? document);
    await sleep(900);
    log.push(`E12 chuyển cảnh vào: ${JSON.stringify((await view()).project.clips.find((c) => c.id === first.id)?.transitionIn)}`);
    // ---- E15: khung thiết bị qua ô chọn ----
    const look = openSection(/màu & khung|color & frame/i);
    await sleep(300);
    choose(/khung thiết bị|device frame/i, "phone", look ?? document);
    await sleep(900);
    slide(/độ bão hòa|saturation/i, -60, openSection(/màu & khung|color & frame/i) ?? document);
    await sleep(900);
    log.push(`E15 look: ${JSON.stringify((await view()).project.clips.find((c) => c.id === first.id)?.look)}`);
    await commands.playerSeek(Math.round(2.0 * T));
    await sleep(1800);
    await snap("main", "v1b-1-keyframe-phone");
    // ---- E10: thêm hình / callout / spotlight từ thanh công cụ ----
    await commands.playerSeek(Math.round(5.0 * T));
    await sleep(500);
    const shapeSel = document.querySelector<HTMLSelectElement>("select.add-shape");
    if (shapeSel) {
      shapeSel.value = "arrow";
      shapeSel.dispatchEvent(new Event("change", { bubbles: true }));
    }
    await sleep(900);
    byText<HTMLButtonElement>(".editor-toolbar button", /\+ callout/i)?.click();
    await sleep(900);
    byText<HTMLButtonElement>(".editor-toolbar button", /\+ spotlight/i)?.click();
    await sleep(900);
    const types = (await view()).project.clips.map((c) => c.content.type);
    log.push(`E10 overlay sau khi thêm: ${JSON.stringify(types)}`);
    const arrow = (await view()).project.clips.filter((c) => c.content.type === "shape").pop();
    if (arrow) {
      select(arrow.id);
      await sleep(900);
      slide(/độ dày nét|stroke width/i, 20);
      await sleep(900);
      const a2 = (await view()).project.clips.find((c) => c.id === arrow.id)!;
      log.push(`E10 mũi tên: ${JSON.stringify(a2.content)}; nhãn timeline: ${[...document.querySelectorAll(".tl-clip-name")].map((e) => e.textContent?.trim()).filter((x) => /➚|💬|◎/.test(x ?? "")).join(" | ")}`);
    }
    await commands.playerSeek(Math.round(5.5 * T));
    await sleep(1800);
    await snap("main", "v1b-2-shapes");
    await commands.projectClose(true);
    navigate("/");
  } catch (e) {
    log.push(`v1b error: ${String(e)}`);
  }
  const s = await commands.settingsGet();
  await commands.settingsSet({ ...s, recentProjects: s.recentProjects.filter((p) => !/[\\/]recordings[\\/]/i.test(p)) });
}

async function roundC({ navigate, outDir, snap, log, sleep }: Ctx) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  try {
    const open = await commands.projectOpen(`${root}/phase6/recording/Rec.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    navigate("/editor");
    await sleep(7000);
    const view = async () => (await commands.projectView())!;
    const clip = (await view()).project.clips[0];
    select(clip.id);
    await commands.playerSeek(Math.round(2.15 * T));
    await sleep(1500);
    const check = (rx: RegExp) => byText<HTMLLabelElement>("label.check", rx)?.querySelector("input");
    check(/spotlight quanh con trỏ|cursor spotlight/i)?.click();
    await sleep(900);
    check(/kính lúp|cursor magnifier/i)?.click();
    await sleep(900);
    check(/hiện phím tắt trong video|show keyboard shortcuts/i)?.click();
    await sleep(900);
    choose(/^bố cục$|^layout$/i, "floating");
    await sleep(900);
    choose(/kiểu con trỏ|cursor style/i, "macOs");
    await sleep(900);
    check(/tiếng click|click sound/i)?.click();
    await sleep(900);
    const fx = (await view()).project.clips[0].content;
    if (fx.type === "recording" && fx.fx) {
      const c = fx.fx;
      log.push(`E06: spotlight ${c.cursor.spotlight?.enabled}, kính lúp ${c.cursor.magnifier?.enabled}, kiểu ${c.cursor.style}, tiếng click ${c.cursor.clickSound?.enabled}`);
      log.push(`E08: bố cục ${c.camera.layout}; R09 phím: ${c.keys?.enabled}`);
    }
    await sleep(2000);
    await snap("main", "v1c-1-cursor-keys-floating");
    await commands.projectClose(true);
    navigate("/");
  } catch (e) {
    log.push(`v1c error: ${String(e)}`);
  }
  const s = await commands.settingsGet();
  await commands.settingsSet({ ...s, recentProjects: s.recentProjects.filter((p) => !/[\\/]recordings[\\/]/i.test(p)) });
}

async function roundD({ navigate, outDir, snap, log, sleep }: Ctx) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  try {
    const open = await commands.projectOpen(`${root}/phase6/audio/Am.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    navigate("/editor");
    await sleep(4000);
    const view = async () => (await commands.projectView())!;
    const clips = (await view()).project.clips;
    select(clips[0].id);
    await sleep(1200);
    openSection(/âm thanh nâng cao|advanced audio/i);
    await sleep(300);
    byText<HTMLLabelElement>("label.check", /khử ồn|noise reduction/i)?.querySelector("input")?.click();
    await sleep(800);
    byText<HTMLLabelElement>("label.check", /chuẩn hóa độ to|normalize loudness/i)?.querySelector("input")?.click();
    await sleep(800);
    log.push(`E11 audioFx: ${JSON.stringify((await view()).project.clips.find((c) => c.id === clips[0].id)?.audioFx)}`);
    // E14: Ctrl+bấm clip thứ hai rồi Nhóm.
    const second = [...document.querySelectorAll<HTMLElement>(".tl-clip")].find((el) => !el.classList.contains("selected"));
    second?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, ctrlKey: true, button: 0 }));
    await sleep(500);
    byText<HTMLButtonElement>(".editor-toolbar button", /🔗/)?.click();
    await sleep(800);
    log.push(`E14 nhóm: ${JSON.stringify((await view()).project.clips.map((c) => c.group != null))}`);
    // Track nam châm (🧲) trên track đầu.
    const magnet = [...document.querySelectorAll<HTMLButtonElement>(".tl-header button")].find((b) => b.textContent?.includes("🧲"));
    magnet?.click();
    await sleep(800);
    log.push(`E14 nam châm: ${JSON.stringify((await view()).project.tracks.map((t) => [t.name, t.magnetic, t.duck]))}`);
    await snap("main", "v1d-1-audio-group");
    await commands.projectClose(true);
    navigate("/");
  } catch (e) {
    log.push(`v1d error: ${String(e)}`);
  }
  const s = await commands.settingsGet();
  await commands.settingsSet({ ...s, recentProjects: s.recentProjects.filter((p) => !/[\\/]recordings[\\/]/i.test(p)) });
}

async function roundE({ navigate, outDir, snap, log, sleep }: Ctx) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  const doneJobs: string[] = [];
  const un = await events.exportEvent.listen(({ payload }) => {
    if (payload.msg.type === "child" && (payload.msg.msg.type === "done" || payload.msg.msg.type === "failed")) {
      doneJobs.push(JSON.stringify(payload.msg.msg).slice(0, 200));
    }
  });
  try {
    const open = await commands.projectOpen(`${root}/phase6/render/V1.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    navigate("/editor");
    await sleep(5000);
    const view = async () => (await commands.projectView())!;
    // ---- E13: hộp thoại phụ đề ----
    byText<HTMLButtonElement>(".editor-toolbar button", /💬/)?.click();
    await sleep(1200);
    log.push(`E13 hộp thoại: ${document.querySelectorAll(".caption-list li").length} dòng; model: ${[...document.querySelectorAll<HTMLOptionElement>(".captions-dialog select option")].slice(0, 3).map((o) => o.textContent).join(" | ")}`);
    await snap("main", "v1e-1-captions");
    byText<HTMLLabelElement>(".captions-dialog label.check", /phụ đề động|animated captions/i)?.querySelector("input")?.click();
    await sleep(800);
    log.push(`E13 kiểu: ${JSON.stringify((await view()).project.captionStyle)}`);
    key("Escape");
    await sleep(500);
    // ---- X06: mỗi marker một file ----
    await commands.timelineApply({ op: "addMarker", at: Math.round(2 * T), label: "Phần A" });
    await commands.timelineApply({ op: "addMarker", at: Math.round(6 * T), label: "Phần B" });
    navigate("/");
    await sleep(300);
    navigate("/editor");
    await sleep(2500);
    key("e", true);
    await sleep(1500);
    const rangeSel = byText<HTMLElement>(".dialog span", /phạm vi|range/i)?.nextElementSibling as HTMLSelectElement | null;
    if (rangeSel) {
      rangeSel.value = "markers";
      rangeSel.dispatchEvent(new Event("change", { bubbles: true }));
    }
    const fmt = byText<HTMLElement>(".dialog span", /định dạng|format/i)?.nextElementSibling as HTMLSelectElement | null;
    if (fmt) {
      fmt.value = "webm";
      fmt.dispatchEvent(new Event("change", { bubbles: true }));
    }
    const res = byText<HTMLElement>(".dialog span", /độ phân giải|resolution/i)?.nextElementSibling as HTMLSelectElement | null;
    if (res) {
      res.value = "360";
      res.dispatchEvent(new Event("change", { bubbles: true }));
    }
    const pathBox = document.querySelector<HTMLInputElement>(".export-path input");
    if (pathBox) setInput(pathBox, `${outDir}\markers.webm`);
    await sleep(1500);
    const over = byText<HTMLLabelElement>(".export-est label.check", /ghi đè|overwrite/i)?.querySelector("input");
    if (over && !over.checked) over.click();
    await sleep(1200);
    log.push(`X06 hộp thoại: ${rangeSel?.selectedOptions[0]?.textContent}; ${document.querySelector(".export-est")?.textContent}`);
    await snap("main", "v1e-2-export-markers");
    document.querySelector<HTMLButtonElement>(".dialog-actions button.primary")?.click();
    for (let i = 0; i < 240 && doneJobs.length < 3; i++) await sleep(500);
    log.push(`X06 job xong: ${doneJobs.length} → ${doneJobs.join(" || ")}`);
    await commands.projectClose(true);
    navigate("/");
    await sleep(800);
    // ---- X06: xuất một project khác (không mở) ----
    const before = doneJobs.length;
    const r = await commands.exportProjectFile(`${root}/phase6/audio/Am.mochi`, {
      format: "mp3", codec: "h264", shortSide: 720, fps: 30, quality: "high", range: null,
      output: `${outDir}\batch-Am.mp3`, overwrite: true, allowBlackFill: false, subtitles: false,
    });
    log.push(`X06 project khác: ${r.status === "ok" ? r.data.state : JSON.stringify(r.error)}`);
    for (let i = 0; i < 120 && doneJobs.length === before; i++) await sleep(500);
    log.push(`X06 kết quả: ${doneJobs.slice(before).join(" || ")}`);
  } catch (e) {
    log.push(`v1e error: ${String(e)}`);
  }
  un();
  const s = await commands.settingsGet();
  await commands.settingsSet({ ...s, recentProjects: s.recentProjects.filter((p) => !/[\\/]recordings[\\/]/i.test(p)) });
}

async function presetDebug({ navigate, log, sleep }: Ctx) {
  const before = await commands.settingsGet();
  navigate("/record");
  for (let i = 0; i < 40 && !document.querySelector(".preset-bar"); i++) await sleep(250);
  await sleep(2000);
  byText<HTMLButtonElement>(".preset-bar button", /lưu preset|save preset/i)?.click();
  await sleep(400);
  const input = document.querySelector<HTMLInputElement>(".dialog input");
  log.push(`dialog: ${!!document.querySelector(".dialog")}, input: ${!!input}`);
  if (input) setInput(input, "Thử");
  await sleep(300);
  const btn = document.querySelector<HTMLButtonElement>(".dialog button.primary");
  log.push(`input value: ${input?.value}; nút: "${btn?.textContent}" disabled=${btn?.disabled}`);
  btn?.click();
  await sleep(1000);
  log.push(`sau khi lưu: ${JSON.stringify((await commands.settingsGet()).recordingPresets.map((p) => p.name))}; còn dialog: ${!!document.querySelector(".dialog")}`);
  log.push(`target text: ${document.querySelector(".record-grid")?.textContent?.slice(0, 200)}`);
  await commands.settingsSet({ ...(await commands.settingsGet()), recordingPresets: before.recordingPresets });
}

async function roundA({ navigate, outDir, snap, log, sleep }: Ctx) {
  const root = outDir.replace(/[\\/]ui-snap.*$/, "");
  const before = await commands.settingsGet();
  try {
    // ---- R11: preset cấu hình quay ----
    navigate("/record");
    for (let i = 0; i < 40 && !document.querySelector(".preset-bar"); i++) await sleep(250);
    await sleep(1500);
    byText<HTMLButtonElement>(".preset-bar button", /lưu preset|save preset/i)?.click();
    await sleep(400);
    const nameInput = document.querySelector<HTMLInputElement>(".dialog input");
    if (nameInput) setInput(nameInput, "Hướng dẫn 60fps");
    await sleep(200);
    document.querySelector<HTMLButtonElement>(".dialog button.primary")?.click();
    await sleep(800);
    const s1 = await commands.settingsGet();
    log.push(`R11 lưu preset: ${JSON.stringify(s1.recordingPresets.map((p) => [p.name, p.config.fps, p.config.countdownSecs]))}`);
    // đổi đếm ngược rồi áp preset → trở lại giá trị trong preset
    const cd = byText<HTMLLabelElement>("label", /đếm ngược|countdown/i)?.querySelector("select") ?? null;
    log.push(`R11 ô đếm ngược: ${cd ? cd.value : "không thấy"}`);
    const presetSelect = document.querySelector<HTMLSelectElement>(".preset-bar select");
    if (presetSelect) {
      presetSelect.value = "Hướng dẫn 60fps";
      presetSelect.dispatchEvent(new Event("change", { bubbles: true }));
    }
    await sleep(500);
    await snap("main", "v1a-1-record-preset");

    // ---- mở project tổng hợp ----
    const open = await commands.projectOpen(`${root}/fix1/Bg.mochi`);
    if (open.status !== "ok") throw new Error(JSON.stringify(open.error));
    navigate("/editor");
    await sleep(7000);
    const view = async () => (await commands.projectView())!;
    const clip0 = (await view()).project.clips.find((x) => x.content.type === "recording")!;

    // ---- E04: freeze frame ----
    select(clip0.id);
    await commands.playerSeek(Math.round(2 * T));
    await sleep(1200);
    byText<HTMLElement>("summary", /freeze frame/i)?.click();
    await sleep(300);
    byText<HTMLButtonElement>("button", /freeze tại đầu phát|freeze at playhead/i)?.click();
    await sleep(1200);
    const v1 = await view();
    const hold = v1.project.clips.find((x) => x.hold != null);
    log.push(
      `E04 freeze: ${hold ? `clip đứng yên ${(hold.start / T).toFixed(2)}–${((hold.start + (hold.sourceOut - hold.sourceIn)) / T).toFixed(2)} s, giữ khung ${(hold.hold! / T).toFixed(2)} s` : "KHÔNG CÓ"}; clip bản quay: ${v1.project.clips.filter((x) => x.content.type === "recording").map((x) => `${(x.start / T).toFixed(1)}+${((x.sourceOut - x.sourceIn) / x.speed / T).toFixed(1)}`).join(", ")}`,
    );
    await commands.playerSeek(Math.round(3 * T));
    await sleep(1500);
    await snap("main", "v1a-2-freeze");

    // ---- E14: marker ----
    await commands.playerSeek(Math.round(1 * T));
    await sleep(300);
    key("m");
    await sleep(500);
    await commands.playerSeek(Math.round(4 * T));
    await sleep(300);
    key("m");
    await sleep(800);
    log.push(`E14 marker: ${JSON.stringify((await view()).project.markers?.map((m) => [m.label, +(m.at / T).toFixed(2)]))}`);
    byText<HTMLButtonElement>(".editor-toolbar button", /🔖/)?.click();
    await sleep(500);
    const firstLabel = document.querySelector<HTMLInputElement>(".marker-list input");
    if (firstLabel) {
      setInput(firstLabel, "Mở đầu");
      firstLabel.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
      firstLabel.blur();
    }
    await sleep(800);
    await snap("main", "v1a-3-markers");
    log.push(`E14 đổi tên: ${JSON.stringify((await view()).project.markers?.map((m) => m.label))}`);
    document.querySelectorAll<HTMLButtonElement>(".marker-list button[title]").forEach((b) => {
      if (/xóa|delete/i.test(b.title)) b.dataset.del = "1";
    });
    document.querySelector<HTMLButtonElement>(".marker-list button[data-del]")?.click();
    await sleep(600);
    log.push(`E14 xóa một marker → còn ${(await view()).project.markers?.length}`);
    key("Escape");
    await sleep(300);

    // ---- E14: theme thương hiệu + preset hiệu ứng ----
    byText<HTMLButtonElement>(".editor-toolbar button", /🎨/)?.click();
    await sleep(500);
    byText<HTMLButtonElement>(".dialog button", /\+ (màu|color)/i)?.click();
    await sleep(200);
    const colors = document.querySelectorAll<HTMLInputElement>(".brand-colors input[type=color]");
    const last = colors[colors.length - 1];
    if (last) setInput(last, "#00aa88");
    const font = byText<HTMLLabelElement>(".dialog label.field", /font/i)?.querySelector("input");
    if (font) setInput(font, "Segoe Print");
    await sleep(200);
    document.querySelector<HTMLButtonElement>(".dialog button.primary")?.click();
    await sleep(800);
    log.push(`E14 theme: ${JSON.stringify((await commands.settingsGet()).brand)}`);
    select(clip0.id);
    await sleep(1200);
    log.push(`E14 ô màu có màu thương hiệu: ${document.querySelectorAll(".swatch[title='#00AA88']").length} ô`);
    byText<HTMLButtonElement>(".inspector .preset-bar button, .preset-bar button", /lưu preset|save preset/i)?.click();
    await sleep(400);
    const fxName = document.querySelector<HTMLInputElement>(".dialog input");
    if (fxName) setInput(fxName, "Nền xanh");
    document.querySelector<HTMLButtonElement>(".dialog button.primary")?.click();
    await sleep(800);
    log.push(`E14 preset hiệu ứng: ${JSON.stringify((await commands.settingsGet()).fxPresets.map((p) => [p.name, p.fx.background.kind, p.fx.zoom.segments.length]))}`);
    await snap("main", "v1a-4-brand-swatches");

    // ---- X08: lưu khung PNG ----
    await commands.playerSeek(Math.round(1.0 * T));
    await sleep(800);
    const frame = await commands.playerSaveFrame("v1a-frame.png");
    log.push(`X08 PNG: ${JSON.stringify(frame)}`);

    // ---- X02: xuất chỉ âm thanh qua hộp thoại ----
    key("e", true);
    await sleep(1500);
    const fmt = byText<HTMLElement>(".dialog span", /định dạng|format/i)?.nextElementSibling as HTMLSelectElement | null;
    if (fmt) {
      fmt.value = "mp3";
      fmt.dispatchEvent(new Event("change", { bubbles: true }));
    }
    await sleep(1200);
    log.push(`X02 hộp thoại MP3: ${document.querySelector(".export-est")?.textContent}; còn ô độ phân giải: ${!!byText(".dialog span", /độ phân giải|resolution/i)}`);
    await snap("main", "v1a-5-export-mp3");
    key("Escape");
    await sleep(300);

    // ---- X07: đóng gói + mở gói ----
    const pack = await commands.projectPack();
    log.push(`X07 đóng gói: ${JSON.stringify(pack)}`);
    await commands.projectClose(true);
    navigate("/");
    await sleep(1000);
    log.push(`S08 trang chủ: "${document.querySelector("p.muted.small.row")?.textContent}"`);
    await snap("main", "v1a-6-home");
  } catch (e) {
    log.push(`v1a error: ${String(e)}`);
  }
  // Trả lại cài đặt của người dùng.
  const s = await commands.settingsGet();
  await commands.settingsSet({
    ...s,
    recordingPresets: before.recordingPresets,
    fxPresets: before.fxPresets,
    brand: before.brand,
    lastRecording: before.lastRecording,
    recentProjects: s.recentProjects.filter((p) => !/[\\/]recordings[\\/]/i.test(p)),
  });
  void events;
}
