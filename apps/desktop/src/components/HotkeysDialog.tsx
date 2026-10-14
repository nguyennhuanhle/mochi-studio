import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type HotkeyConflict, type Hotkeys } from "../bindings";
import Dialog from "./Dialog";

type Action = keyof Hotkeys;
const ACTIONS: Action[] = ["recordPause", "stop", "cancel"];

/** KeyboardEvent → accelerator của Tauri ("Ctrl+Shift+F9"). Chỉ bấm phím bổ trợ → null. */
export function accelerator(e: KeyboardEvent): string | null {
  const key = e.key;
  if (["Control", "Shift", "Alt", "Meta"].includes(key)) return null;
  let main: string;
  if (/^F\d{1,2}$/.test(key)) main = key;
  else if (/^Key[A-Z]$/.test(e.code)) main = e.code.slice(3);
  else if (/^Digit\d$/.test(e.code)) main = e.code.slice(5);
  else if (e.code === "Space") main = "Space";
  else if (["Home", "End", "PageUp", "PageDown", "Insert", "Delete", "Pause", "ScrollLock"].includes(e.code)) main = e.code;
  else return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
  // Phím chữ / số / Space không có phím bổ trợ sẽ chặn việc gõ bình thường ở mọi app → bắt buộc có.
  if (mods.length === 0 && !/^F\d/.test(main) && !["Pause", "ScrollLock", "Insert"].includes(main)) return null;
  return [...mods, main].join("+");
}

/**
 * R04: đổi phím tắt toàn cục. Trong lúc hộp thoại mở, phím tắt toàn cục bị nhả để bấm F9… không
 * kích hoạt quay. Phím bị app khác giữ → báo + gợi ý phím còn trống.
 */
export default function HotkeysDialog(props: {
  initial: Hotkeys;
  conflicts: HotkeyConflict[];
  onClose: (saved: { hotkeys: Hotkeys; conflicts: HotkeyConflict[] } | null) => void;
}) {
  const { t } = useTranslation();
  const [keys, setKeys] = useState<Hotkeys>(props.initial);
  const [listening, setListening] = useState<Action | null>(null);
  const [conflicts, setConflicts] = useState<HotkeyConflict[]>(props.conflicts);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    commands.hotkeysSuspend();
  }, []);

  useEffect(() => {
    if (!listening) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") return setListening(null);
      const acc = accelerator(e);
      if (!acc) return;
      setKeys((k) => ({ ...k, [listening]: acc }));
      setListening(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [listening]);

  async function save(next = keys) {
    setError(null);
    const r = await commands.hotkeysSet(next);
    if (r.status === "error") {
      const [kind, key] = r.error.split(":");
      setError(kind === "duplicate" ? t("hotkeys.duplicate", { key }) : t("hotkeys.invalid", { key }));
      return;
    }
    setConflicts(r.data);
    if (r.data.length === 0) props.onClose({ hotkeys: next, conflicts: [] });
  }

  async function cancel() {
    const c = await commands.hotkeysApply();
    props.onClose({ hotkeys: props.initial, conflicts: c });
  }

  return (
    <Dialog
      title={t("hotkeys.title")}
      onClose={() => (listening ? setListening(null) : cancel())}
      actions={
        <>
          <button onClick={cancel}>{t("common.cancel")}</button>
          <button className="primary" onClick={() => save()}>
            {t("hotkeys.save")}
          </button>
        </>
      }
    >
      <div className="hotkey-grid">
        {ACTIONS.map((a) => {
          const conflict = conflicts.find((c) => c.key === keys[a]);
          return (
            <div key={a} className="hotkey-row">
              <span>{t(`hotkeys.${a}`)}</span>
              <button className={listening === a ? "hotkey-key listening" : "hotkey-key"} onClick={() => setListening(a)}>
                {listening === a ? t("hotkeys.press") : keys[a] || t("hotkeys.none")}
              </button>
              {conflict && (
                <div className="warn small">
                  {t("hotkeys.taken", { key: conflict.key })}
                  {conflict.suggestion && (
                    <button
                      className="link-btn"
                      onClick={() => {
                        const next = { ...keys, [a]: conflict.suggestion! };
                        setKeys(next);
                        save(next);
                      }}
                    >
                      {t("hotkeys.useSuggestion", { key: conflict.suggestion })}
                    </button>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
      <p className="muted small">{t("hotkeys.hint")}</p>
      {error && <p className="err">{error}</p>}
    </Dialog>
  );
}
