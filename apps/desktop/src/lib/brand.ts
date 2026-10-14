import { useSyncExternalStore } from "react";
import { commands, type BrandTheme } from "../bindings";

/**
 * E14: theme thương hiệu (màu gợi ý + font cho chữ mới), lưu trong cài đặt.
 * Kho nhỏ dùng chung cho mọi ô màu: nạp một lần, cập nhật khi người dùng sửa.
 */
let brand: BrandTheme = { colors: [], font: "" };
const listeners = new Set<() => void>();
let loaded = false;

function load() {
  if (loaded) return;
  loaded = true;
  commands.settingsGet().then((s) => setLocal(s.brand));
}

function setLocal(next: BrandTheme) {
  brand = next;
  listeners.forEach((l) => l());
}

export function useBrand(): BrandTheme {
  load();
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => brand,
  );
}

export async function saveBrand(next: BrandTheme) {
  const colors = next.colors.filter((c) => /^#[0-9A-Fa-f]{6}$/.test(c)).slice(0, 8);
  const clean = { colors, font: next.font.trim().slice(0, 80) };
  const s = await commands.settingsGet();
  await commands.settingsSet({ ...s, brand: clean });
  setLocal(clean);
}

/** Font cho chữ mới: font thương hiệu nếu có, không thì Segoe UI. */
export function brandFont(): string {
  load();
  return brand.font || "Segoe UI";
}
