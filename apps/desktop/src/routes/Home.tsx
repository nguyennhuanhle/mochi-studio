import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { commands } from "../bindings";
import { useOpenProject } from "../lib/useOpenProject";
import mochiMark from "../assets/mochi-mark.png";
import GpuWarning from "../components/GpuWarning";
import CachePanel from "../components/CachePanel";

export default function Home() {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const [recent, setRecent] = useState<string[]>([]);
  /** S02: project lần trước không đóng sạch (crash) → đề nghị mở lại để khôi phục. */
  const [unclean, setUnclean] = useState<string[]>([]);
  const [unpackError, setUnpackError] = useState<string | null>(null);
  const [version, setVersion] = useState("");
  useEffect(() => {
    commands.appVersion().then(setVersion);
  }, []);
  const { open, dialog } = useOpenProject();

  useEffect(() => {
    commands.recentProjects().then(setRecent);
    commands.projectsUnclean().then(setUnclean);
  }, [navigate]);

  return (
    <div className="page">
      <div className="row" style={{ justifyContent: "space-between" }}>
        <h1 className="brand" aria-label={t("app.name")}>
          <img src={mochiMark} alt="" className="brand-mark" />
          <span className="brand-word">
            <b>
              Moch<span className="brand-i">ı</span>
            </b>{" "}
            Studio
          </span>
        </h1>
        <label className="row">
          <span className="muted">{t("lang.label")}</span>
          <select value={i18n.language} onChange={(e) => i18n.changeLanguage(e.target.value)}>
            <option value="vi">{t("lang.vi")}</option>
            <option value="en">{t("lang.en")}</option>
          </select>
        </label>
      </div>
      <div className="row">
        <button className="primary" onClick={() => navigate("/record")}>
          ● {t("home.newRecording")}
        </button>
        <button
          onClick={async () => {
            const dir = await commands.pickFolder();
            if (dir) open(dir);
          }}
        >
          {t("home.openProject")}
        </button>
        {/* X07: mở gói .zip do "Đóng gói" tạo (giải nén cạnh file zip). */}
        <button
          onClick={async () => {
            const r = await commands.projectUnpackPick();
            if (r.status === "error") setUnpackError(r.error);
            else if (r.data) open(r.data);
          }}
        >
          {t("home.openZip")}
        </button>
        <button
          onClick={async () => {
            const r = await commands.projectCreate(`Mochi ${new Date().toLocaleDateString()}`);
            if (r.status === "ok") navigate("/editor");
          }}
        >
          {t("home.newProject")}
        </button>
      </div>
      {unpackError && (
        <div className="error-box" role="alert">
          {t("home.unpackFailed", { detail: unpackError })}
        </div>
      )}
      <GpuWarning />
      {/* S08 (v1): kiểm tra cập nhật — chỉ mở trang Releases, app không tự kết nối mạng. */}
      <p className="muted small row">
        Mochi Studio {version}
        <button className="link-btn" onClick={() => commands.openReleasesPage()} title={t("update.hint")}>
          {t("update.check")}
        </button>
      </p>
      {unclean.length > 0 && (
        <div className="warn-box" role="status">
          <strong>{t("recover.homeTitle")}</strong>
          {unclean.map((dir) => (
            <div key={dir} className="row space">
              <span>{dir.split(/[\\/]/).pop()?.replace(/\.mochi$/, "")}</span>
              <button className="primary" onClick={() => open(dir)}>
                {t("recover.homeOpen")}
              </button>
            </div>
          ))}
        </div>
      )}
      <h3 style={{ marginBottom: 0 }}>{t("home.recent")}</h3>
      {recent.length === 0 ? (
        <p className="muted">{t("home.noRecent")}</p>
      ) : (
        <ul className="recent-list">
          {recent.map((dir) => (
            <li key={dir}>
              <button className="recent-item" onClick={() => open(dir)}>
                <strong>{dir.split(/[\\/]/).pop()?.replace(/\.mochi$/, "")}</strong>
                <span className="muted small">{dir}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <CachePanel />
      {dialog}
    </div>
  );
}
