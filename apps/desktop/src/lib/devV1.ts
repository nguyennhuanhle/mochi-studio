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
