import type { TFunction } from "i18next";
import type { DeviceKind, RecorderError, RecorderWarning } from "../bindings";

export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
}

function device(t: TFunction, kind: DeviceKind): string {
  return t(`device.${kind}`);
}

/** Lỗi của RecorderService → câu dễ hiểu theo ngôn ngữ đang chọn. */
export function errorMessage(t: TFunction, e: RecorderError): string {
  switch (e.code) {
    case "lowDisk":
      return t("error.lowDisk", {
        free: formatBytes(e.freeBytes),
        required: formatBytes(e.requiredBytes),
      });
    case "micNotFound":
      return t("error.micNotFound", { name: e.name });
    case "permissionDenied":
      return t("error.permissionDenied", { kind: device(t, e.kind) });
    case "deviceFailed":
      return t("error.deviceFailed", { kind: device(t, e.kind), detail: e.detail });
    case "startFailed":
    case "internal":
      return t(`error.${e.code}`, { detail: e.detail });
    default:
      return t(`error.${e.code}`);
  }
}

export function warningMessage(t: TFunction, w: RecorderWarning): string {
  switch (w.code) {
    case "deviceLost":
    case "deviceRestored": {
      const kind = device(t, w.kind);
      return t(`warning.${w.code}`, { kind: kind.charAt(0).toUpperCase() + kind.slice(1) });
    }
    case "diskLow":
      return t("warning.diskLow", { free: formatBytes(w.freeBytes) });
    case "frameDrops":
      return t("warning.frameDrops", { rate: w.ratePct.toFixed(0) });
    case "encoderFallback":
      return t("warning.encoderFallback", { backend: w.backend });
    case "audioGap":
      return t("warning.audioGap", { ms: w.gapMs.toFixed(0) });
    default:
      return t(`warning.${w.code}`);
  }
}

/** Cảnh báo nào cần người dùng chú ý ngay (màu đỏ, giữ lâu hơn). */
export function isSevere(w: RecorderWarning): boolean {
  return ["deviceLost", "stoppedDiskFull", "windowClosed", "displayLost", "areaOutOfBounds", "pipelineFailed"].includes(
    w.code,
  );
}
