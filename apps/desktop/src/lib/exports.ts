import { useSyncExternalStore } from "react";
import type { TFunction } from "i18next";
import { commands, events, type ExportError, type ExportSettings } from "../bindings";
import { formatBytes } from "./recorder";

/** Trạng thái một lần xuất, giữ ở cấp module để còn khi chuyển trang (X04). */
export type ExportJobView = {
  jobId: string;
  name: string;
  output: string;
  settings: ExportSettings;
  state: "queued" | "running" | "done" | "failed" | "cancelled";
  frame: number;
  frames: number;
  fps: number;
  etaSecs: number;
  encoder: string | null;
  hardware: boolean;
  fallback: string | null;
  error: ExportError | null;
  bytes: number;
  secs: number;
};

let jobs: ExportJobView[] = [];
const subs = new Set<() => void>();
let listening = false;

function set(next: ExportJobView[]) {
  jobs = next;
  subs.forEach((f) => f());
}

function patch(jobId: string, p: Partial<ExportJobView>) {
  if (!jobs.some((j) => j.jobId === jobId)) {
    // Sự kiện có thể tới trước khi export_start trả về.
    jobs = [...jobs, blank(jobId)];
  }
  set(jobs.map((j) => (j.jobId === jobId ? { ...j, ...p } : j)));
}

function blank(jobId: string): ExportJobView {
  return {
    jobId,
    name: "",
    output: "",
    settings: null as unknown as ExportSettings,
    state: "queued",
    frame: 0,
    frames: 0,
    fps: 0,
    etaSecs: 0,
    encoder: null,
    hardware: false,
    fallback: null,
    error: null,
    bytes: 0,
    secs: 0,
  };
}

function listen() {
  if (listening) return;
  listening = true;
  events.exportEvent.listen(({ payload }) => {
    const { jobId, msg } = payload;
    if (msg.type === "running") return patch(jobId, { state: "running" });
    const m = msg.msg;
    switch (m.type) {
      case "started":
        return patch(jobId, { state: "running", encoder: m.encoder, hardware: m.hardware, frames: m.frames, frame: 0 });
      case "progress":
        return patch(jobId, { frame: m.frame, frames: m.frames, fps: m.fps, etaSecs: m.etaSecs });
      case "fallback":
        return patch(jobId, { fallback: m.to });
      case "done":
        return patch(jobId, { state: "done", output: m.path, bytes: m.bytes, secs: m.secs, encoder: m.encoder, frame: jobs.find((j) => j.jobId === jobId)?.frames ?? 0 });
      case "failed":
        return patch(jobId, { state: m.error.code === "cancelled" ? "cancelled" : "failed", error: m.error });
    }
  });
}

export function useExportJobs(): ExportJobView[] {
  listen();
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => jobs,
  );
}

/** Bắt đầu xuất (đã qua preflight ở phía Rust). Trả lỗi để hộp thoại hiện. */
export async function startExport(settings: ExportSettings): Promise<ExportError | null> {
  listen();
  const r = await commands.exportStart(settings);
  if (r.status === "error") return r.error;
  const { jobId, state, output, projectName } = r.data;
  const existing = jobs.find((j) => j.jobId === jobId);
  const base = existing ?? blank(jobId);
  set([
    ...jobs.filter((j) => j.jobId !== jobId),
    {
      ...base,
      name: projectName,
      output,
      settings,
      state: existing && existing.state !== "queued" ? existing.state : state === "running" ? "running" : "queued",
    },
  ]);
  return null;
}

export function dismissExport(jobId: string) {
  set(jobs.filter((j) => j.jobId !== jobId));
}

/**
 * Thử lại với đúng thiết lập cũ (K-X1): chỉ ghi đè khi người dùng đã đồng ý cho đường dẫn này lúc bắt đầu,
 * hoặc vừa bấm "Ghi đè và thử lại" (`confirmOverwrite`). Lần xuất lỗi không để lại file (ghi vào file tạm).
 */
export async function retryExport(job: ExportJobView, confirmOverwrite = false): Promise<ExportError | null> {
  const settings = { ...job.settings!, overwrite: job.settings!.overwrite || confirmOverwrite };
  const err = await startExport(settings);
  if (!err) dismissExport(job.jobId);
  return err;
}

export function exportErrorMessage(t: TFunction, e: ExportError): string {
  switch (e.code) {
    case "fileExists":
      return t("export.err.fileExists");
    case "missingMedia":
      return t("export.err.missingMedia", { names: e.names.join(", ") });
    case "unsupportedEncoder":
      return t("export.err.unsupportedEncoder", { reason: e.reason });
    case "notWritable":
      return t("export.err.notWritable", { dir: e.dir });
    case "diskFull":
      return t("export.err.diskFull", { needed: formatBytes(e.neededBytes), free: formatBytes(e.freeBytes) });
    case "invalidRange":
      return t("export.err.invalidRange");
    case "emptyProject":
      return t("export.err.emptyProject");
    case "encoder":
      return t("export.err.encoder", { message: e.message });
    case "cancelled":
      return t("export.err.cancelled");
    case "crashed":
      return t("export.err.crashed", { log: e.log });
    case "internal":
      return e.message;
  }
}

export function formatEta(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}
