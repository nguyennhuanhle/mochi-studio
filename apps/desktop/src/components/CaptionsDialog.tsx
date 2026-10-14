import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, events, type Caption, type CaptionModel, type CaptionStyle, type EditOp, type ProjectView, type Ticks } from "../bindings";
import { formatTimecode, secs, ticks } from "../lib/editor";
import { Check, ColorField, Slider } from "./controls";
import Dialog from "./Dialog";

const LANGS = ["auto", "vi", "en", "ja", "ko", "zh", "fr", "de", "es"];
const DEFAULT_STYLE: CaptionStyle = {
  visible: true,
  font: "",
  size: 4.5,
  color: "#FFFFFF",
  background: "#000000B0",
  y: 0.88,
  animated: false,
  highlight: "#FFD54A",
};

/**
 * E13 / S07 / X05 (v1): phụ đề — tạo bằng Whisper chạy trên máy (model tải khi người dùng đồng ý),
 * sửa chữ + thời gian, kiểu chữ, phụ đề động, nhập / xuất SRT / VTT.
 */
export default function CaptionsDialog(props: { view: ProjectView; playhead: Ticks; apply: (op: EditOp) => Promise<boolean>; onClose: () => void; onSeek: (t: Ticks) => void }) {
  const { t } = useTranslation();
  const caps = props.view.project.captions ?? [];
  const style: CaptionStyle = { ...DEFAULT_STYLE, ...(props.view.project.captionStyle ?? {}) };
  const [models, setModels] = useState<CaptionModel[]>([]);
  const [model, setModel] = useState("base");
  const [lang, setLang] = useState("auto");
  const [busy, setBusy] = useState<{ stage: string; pct: number } | null>(null);
  const [confirmDownload, setConfirmDownload] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cpu, setCpu] = useState<string | null>(null);

  useEffect(() => {
    commands.captionsModels().then(setModels);
    commands.captionsCpuCheck().then((r) => setCpu(r.status === "error" ? r.error : null));
    const un = events.captionsProgress.listen(({ payload }) => setBusy(payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const setCaps = (captions: Caption[]) => props.apply({ op: "setCaptions", captions });
  const setStyle = (patch: Partial<CaptionStyle>) => props.apply({ op: "setCaptionStyle", style: { ...style, ...patch } });
  const chosen = models.find((m) => m.id === model);

  async function generate() {
    setError(null);
    setBusy({ stage: "audio", pct: 0 });
    const r = await commands.captionsGenerate(model, lang === "auto" ? null : lang);
    setBusy(null);
    if (r.status === "error") return setError(r.error);
    if (r.data.length === 0) return setError(t("captions.nothing"));
    await setCaps(r.data);
  }

  async function download() {
    setConfirmDownload(false);
    setError(null);
    setBusy({ stage: "download", pct: 0 });
    const r = await commands.captionsDownload(model);
    setBusy(null);
    setModels(await commands.captionsModels());
    if (r.status === "error") return setError(t("captions.downloadFailed", { detail: r.error }));
    await generate();
  }

  const mb = (b: number) => `${Math.round(b / 1e6)} MB`;
  const update = (i: number, patch: Partial<Caption>) => setCaps(caps.map((c, j) => (j === i ? { ...c, ...patch, words: patch.text !== undefined ? [] : c.words } : c)));

  return (
    <Dialog title={t("captions.title")} onClose={busy ? () => {} : props.onClose} actions={<button disabled={!!busy} onClick={props.onClose}>{t("common.close")}</button>}>
      <div className="stack captions-dialog">
        <section className="stack">
          <strong>{t("captions.generate")}</strong>
          {cpu && <p className="error-box small">{cpu}</p>}
          <div className="row wrap">
            <label className="row small">
              {t("captions.model")}
              <select value={model} onChange={(e) => setModel(e.target.value)} disabled={!!busy}>
                {models.map((m) => (
                  <option key={m.id} value={m.id}>
                    {t(`captions.model_${m.id}`)} · {mb(m.bytes)} {m.installed ? "✓" : ""}
                  </option>
                ))}
              </select>
            </label>
            <label className="row small">
              {t("captions.language")}
              <select value={lang} onChange={(e) => setLang(e.target.value)} disabled={!!busy}>
                {LANGS.map((l) => (
                  <option key={l} value={l}>
                    {t(`captions.lang_${l}`, { defaultValue: l })}
                  </option>
                ))}
              </select>
            </label>
            <button
              className="primary"
              disabled={!!busy || !!cpu}
              onClick={() => (chosen?.installed ? generate() : setConfirmDownload(true))}
            >
              {caps.length ? t("captions.regenerate") : t("captions.start")}
            </button>
            {busy && <button onClick={() => commands.captionsCancel()}>{t("common.cancel")}</button>}
          </div>
          {busy && (
            <div className="stack">
              <span className="small muted">{t(`captions.stage_${busy.stage}`, { pct: Math.round(busy.pct) })}</span>
              <progress max={100} value={busy.pct} />
            </div>
          )}
          {error && <p className="error-box small">{error}</p>}
          <p className="muted small">{t("captions.privacy")}</p>
        </section>

        <section className="stack">
          <span className="row space">
            <strong>{t("captions.lines", { n: caps.length })}</strong>
            <span className="row">
              <button onClick={async () => {
                const r = await commands.captionsImport();
                if (r.status === "error") setError(r.error);
                else if (r.data) await setCaps(r.data);
              }}>{t("captions.import")}</button>
              <button disabled={!caps.length} onClick={() => commands.captionsExport(false)}>{t("captions.exportSrt")}</button>
              <button disabled={!caps.length} onClick={() => commands.captionsExport(true)}>{t("captions.exportVtt")}</button>
            </span>
          </span>
          <ul className="plain-list caption-list">
            {caps.map((c, i) => (
              <li key={`${c.id}-${i}`} className="row">
                <button className="tb" title={t("marker.goto")} onClick={() => props.onSeek(c.start)}>
                  ▶
                </button>
                <TimeBox value={c.start} onCommit={(v) => update(i, { start: v })} />
                <TimeBox value={c.end} onCommit={(v) => update(i, { end: v })} />
                <input
                  key={c.text}
                  defaultValue={c.text}
                  className="caption-text"
                  onBlur={(e) => e.target.value !== c.text && update(i, { text: e.target.value })}
                  onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
                />
                <button className="tb" title={t("captions.delete")} onClick={() => setCaps(caps.filter((_, j) => j !== i))}>
                  ✕
                </button>
              </li>
            ))}
          </ul>
          <button
            onClick={() =>
              setCaps([...caps, { id: `cap-${Date.now()}`, start: props.playhead, end: props.playhead + ticks(2), text: t("captions.newLine"), words: [] }])
            }
          >
            {t("captions.add", { at: formatTimecode(props.playhead) })}
          </button>
        </section>

        <section className="stack">
          <strong>{t("captions.style")}</strong>
          <Check label={t("captions.visible")} checked={style.visible} onChange={(visible) => setStyle({ visible })} />
          <Check label={t("captions.animated")} checked={style.animated} onChange={(animated) => setStyle({ animated })} />
          {style.animated && <ColorField label={t("captions.highlight")} value={style.highlight} onCommit={(highlight) => setStyle({ highlight })} />}
          <Slider label={t("captions.size")} value={style.size} min={2} max={10} step={0.5} format={(v) => `${v.toFixed(1)}%`} onCommit={(size) => setStyle({ size })} />
          <Slider label={t("captions.position")} value={style.y} min={0.05} max={0.95} step={0.01} format={(v) => `${Math.round(v * 100)}%`} onCommit={(y) => setStyle({ y })} />
          <ColorField label={t("captions.color")} value={style.color} onCommit={(color) => setStyle({ color })} />
          <Check label={t("captions.background")} checked={!!style.background} onChange={(on) => setStyle({ background: on ? "#000000B0" : null })} />
          {style.background && <ColorField label={t("captions.background")} value={style.background} onCommit={(bg) => setStyle({ background: `${bg}${style.background!.slice(7, 9) || "B0"}` })} />}
        </section>
      </div>
      {confirmDownload && chosen && (
        <Dialog
          title={t("captions.downloadTitle")}
          onClose={() => setConfirmDownload(false)}
          actions={
            <>
              <button onClick={() => setConfirmDownload(false)}>{t("common.cancel")}</button>
              <button className="primary" onClick={download}>
                {t("captions.downloadConfirm", { size: mb(chosen.bytes) })}
              </button>
            </>
          }
        >
          <p>{t("captions.downloadBody", { model: t(`captions.model_${chosen.id}`), size: mb(chosen.bytes) })}</p>
        </Dialog>
      )}
    </Dialog>
  );
}

/** Ô thời gian (giây, 2 chữ số thập phân), ghi khi rời ô. */
function TimeBox(props: { value: Ticks; onCommit: (v: Ticks) => void }) {
  return (
    <input
      key={props.value}
      className="caption-time"
      type="text"
      inputMode="decimal"
      defaultValue={secs(props.value).toFixed(2)}
      onBlur={(e) => {
        const v = Number(e.target.value.replace(",", "."));
        if (Number.isFinite(v) && v >= 0 && Math.abs(ticks(v) - props.value) > 1000) props.onCommit(ticks(v));
      }}
      onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
    />
  );
}
