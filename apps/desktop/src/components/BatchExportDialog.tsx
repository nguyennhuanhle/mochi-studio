import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type ExportFormat, type ExportSettings } from "../bindings";
import Dialog from "./Dialog";

type Preset = "youtube" | "uhd" | "gif" | "mp3" | "webm";

const PRESETS: Record<Preset, Pick<ExportSettings, "format" | "codec" | "shortSide" | "fps" | "quality">> = {
  youtube: { format: "mp4", codec: "h264", shortSide: 1080, fps: 30, quality: "high" },
  uhd: { format: "mp4", codec: "h264", shortSide: 2160, fps: 30, quality: "high" },
  gif: { format: "gif", codec: "h264", shortSide: 480, fps: 15, quality: "standard" },
  mp3: { format: "mp3", codec: "h264", shortSide: 720, fps: 30, quality: "high" },
  webm: { format: "webm", codec: "h264", shortSide: 1080, fps: 30, quality: "high" },
};

/**
 * X06 (v1): xuất hàng loạt — chọn nhiều project gần đây + một mẫu xuất; mỗi project thành một job
 * trong hàng đợi xuất (bảng tiến độ góc dưới). Tên file = tên thư mục project.
 */
export default function BatchExportDialog(props: { projects: string[]; onClose: () => void }) {
  const { t } = useTranslation();
  const [chosen, setChosen] = useState<string[]>(props.projects);
  const [preset, setPreset] = useState<Preset>("youtube");
  const [dir, setDir] = useState("");
  const [overwrite, setOverwrite] = useState(false);
  const [results, setResults] = useState<{ dir: string; ok: boolean; detail: string }[]>([]);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    commands.exportDefaultDir().then(setDir);
  }, []);

  const name = (p: string) => p.split(/[\\/]/).pop()?.replace(/\.mochi$/i, "") ?? "Mochi";
  const ext = (f: ExportFormat) => (f === "mp4" ? "mp4" : f);

  async function start() {
    setRunning(true);
    const out: { dir: string; ok: boolean; detail: string }[] = [];
    for (const p of chosen) {
      const base = PRESETS[preset];
      const r = await commands.exportProjectFile(p, {
        ...base,
        range: null,
        output: `${dir}\\${name(p)}.${ext(base.format)}`,
        overwrite,
        allowBlackFill: false,
        subtitles: false,
      });
      out.push({ dir: p, ok: r.status === "ok", detail: r.status === "ok" ? r.data.output : r.error.code });
      setResults([...out]);
    }
    setRunning(false);
  }

  return (
    <Dialog
      title={t("batch.title")}
      onClose={props.onClose}
      actions={
        <>
          <button onClick={props.onClose}>{t("common.close")}</button>
          <button className="primary" disabled={!chosen.length || !dir || running} onClick={start}>
            {t("batch.start", { n: chosen.length })}
          </button>
        </>
      }
    >
      <div className="stack">
        <label className="row small">
          {t("export.preset")}
          <select value={preset} onChange={(e) => setPreset(e.target.value as Preset)}>
            {(Object.keys(PRESETS) as Preset[]).map((p) => (
              <option key={p} value={p}>
                {t(`batch.preset_${p}`)}
              </option>
            ))}
          </select>
        </label>
        <span className="small muted">
          {t("export.saveTo")}: {dir}
        </span>
        <label className="check">
          <input type="checkbox" checked={overwrite} onChange={(e) => setOverwrite(e.target.checked)} />
          <span>{t("export.overwrite")}</span>
        </label>
        <ul className="plain-list batch-list">
          {props.projects.map((p) => {
            const r = results.find((x) => x.dir === p);
            return (
              <li key={p}>
                <label className="check">
                  <input
                    type="checkbox"
                    checked={chosen.includes(p)}
                    disabled={running}
                    onChange={(e) => setChosen((c) => (e.target.checked ? [...c, p] : c.filter((x) => x !== p)))}
                  />
                  <span>{name(p)}</span>
                  {r && <span className={r.ok ? "small ok-text" : "small error-text"}> — {r.ok ? t("batch.queued") : t("batch.failed", { code: r.detail })}</span>}
                </label>
              </li>
            );
          })}
        </ul>
        <p className="muted small">{t("batch.hint")}</p>
      </div>
    </Dialog>
  );
}
