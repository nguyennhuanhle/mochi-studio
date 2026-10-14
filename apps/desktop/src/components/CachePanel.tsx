import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type CacheInfo } from "../bindings";
import { formatBytes } from "../lib/recorder";

const LIMITS = [5, 10, 20, 50, 100];

/** Bộ nhớ đệm (proxy, ảnh thu nhỏ, waveform): dung lượng, giới hạn, dọn. Không bao giờ xóa bản quay gốc (K-S3). */
export default function CachePanel() {
  const { t } = useTranslation();
  const [info, setInfo] = useState<CacheInfo | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    commands.cacheInfo().then(setInfo);
  }, []);
  if (!info) return null;
  return (
    <section className="cache-panel">
      <h3 style={{ margin: "0 0 6px" }}>{t("cache.title")}</h3>
      <div className="row small">
        <span>
          {t("cache.usage", {
            used: formatBytes(info.usage.totalBytes),
            proxies: formatBytes(info.usage.proxiesBytes),
            projects: info.usage.projects,
          })}
        </span>
        <span style={{ flex: 1 }} />
        <label className="row">
          {t("cache.limit")}
          <select
            value={info.limitGb}
            onChange={async (e) => {
              // Hạ giới hạn → dọn ngay nếu đang vượt.
              const r = await commands.cacheSetLimit(Number(e.target.value));
              if (r.status === "ok") setInfo(r.data);
            }}
          >
            {[...new Set([...LIMITS, info.limitGb])].sort((a, b) => a - b).map((g) => (
              <option key={g} value={g}>
                {g} GB
              </option>
            ))}
          </select>
        </label>
        <button
          disabled={busy || info.usage.totalBytes === 0}
          onClick={async () => {
            setBusy(true);
            setInfo(await commands.cacheClear());
            setBusy(false);
          }}
        >
          {t("cache.clear")}
        </button>
      </div>
      <p className="muted small" style={{ margin: "4px 0 0" }}>
        {t("cache.note")}
      </p>
    </section>
  );
}
