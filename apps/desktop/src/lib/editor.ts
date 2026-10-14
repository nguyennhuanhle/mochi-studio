import type { TFunction } from "i18next";
import type { EditorError, Ticks } from "../bindings";

export const TICKS_PER_SECOND = 705_600_000;
export const secs = (t: Ticks) => t / TICKS_PER_SECOND;
export const ticks = (s: number): Ticks => Math.round(s * TICKS_PER_SECOND);

/** 00:12.40 (hoặc 1:02:03.25) */
export function formatTimecode(t: Ticks): string {
  const total = Math.max(0, secs(t));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = s.toFixed(2).padStart(5, "0");
  const mm = String(m).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

export function editorErrorMessage(t: TFunction, e: EditorError): string {
  switch (e.code) {
    case "timeline":
      return t(`timelineError.${e.error.code}`, { detail: "detail" in e.error ? e.error.detail : "" });
    case "project":
      return t(`projectError.${e.error.code}`, { detail: "detail" in e.error ? e.error.detail : "" });
    case "inUse":
      return t("editor.removeInUseBody", { n: e.clipIds.length });
    case "nothingToUndo":
      return "";
  }
}
