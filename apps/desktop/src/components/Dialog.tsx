import { useEffect, type ReactNode } from "react";
import { commands } from "../bindings";

/** Số hộp thoại đang mở — preview native (HWND con) phải ẩn khi có hộp thoại đè lên. */
let openDialogs = 0;

/** Hộp thoại modal đơn giản; Esc để đóng. */
export default function Dialog(props: {
  title: string;
  children: ReactNode;
  actions: ReactNode;
  onClose: () => void;
}) {
  const { onClose } = props;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  useEffect(() => {
    if (openDialogs++ === 0) commands.previewSetVisible(false);
    return () => {
      if (--openDialogs === 0) commands.previewSetVisible(true);
    };
  }, []);
  return (
    <div className="dialog-backdrop" onClick={onClose}>
      <div className="dialog" role="dialog" aria-modal="true" aria-label={props.title} onClick={(e) => e.stopPropagation()}>
        <h3>{props.title}</h3>
        <div>{props.children}</div>
        <div className="dialog-actions">{props.actions}</div>
      </div>
    </div>
  );
}
