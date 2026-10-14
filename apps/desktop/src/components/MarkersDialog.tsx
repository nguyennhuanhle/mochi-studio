import { useTranslation } from "react-i18next";
import type { EditOp, Marker, Ticks } from "../bindings";
import { formatTimecode } from "../lib/editor";
import Dialog from "./Dialog";

/** E14: danh sách marker — đổi tên, nhảy tới, xóa. Thêm marker bằng phím M tại đầu phát. */
export default function MarkersDialog(props: {
  markers: Marker[];
  apply: (op: EditOp) => Promise<boolean>;
  onSeek: (t: Ticks) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog title={t("marker.title")} onClose={props.onClose} actions={<button onClick={props.onClose}>{t("common.close")}</button>}>
      {props.markers.length === 0 && <p className="muted">{t("marker.empty")}</p>}
      <ul className="plain-list marker-list">
        {props.markers.map((m) => (
          <li key={m.id} className="row">
            <button onClick={() => props.onSeek(m.at)} title={t("marker.goto")}>
              {formatTimecode(m.at)}
            </button>
            <input
              key={`${m.id}-${m.label}`}
              defaultValue={m.label}
              maxLength={80}
              aria-label={t("marker.label")}
              onBlur={(e) => e.target.value !== m.label && props.apply({ op: "updateMarker", markerId: m.id, at: null, label: e.target.value })}
              onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
            />
            <button title={t("marker.delete")} onClick={() => props.apply({ op: "removeMarker", markerId: m.id })}>
              ✕
            </button>
          </li>
        ))}
      </ul>
      <p className="muted small">{t("marker.hint")}</p>
    </Dialog>
  );
}
