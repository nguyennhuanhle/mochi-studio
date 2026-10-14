import { useEffect, useRef } from "react";
import { commands } from "../bindings";

function physRect(el: HTMLElement) {
  const r = el.getBoundingClientRect();
  const dpr = window.devicePixelRatio;
  return {
    x: Math.round(r.left * dpr),
    y: Math.round(r.top * dpr),
    width: Math.max(1, Math.round(r.width * dpr)),
    height: Math.max(1, Math.round(r.height * dpr)),
  };
}

/**
 * Vùng preview: một HWND con do wgpu vẽ trực tiếp (Spike B), khớp đúng hình chữ nhật của div này.
 * Không có frame nào đi qua IPC.
 */
export default function PreviewPane() {
  const slot = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = slot.current!;
    let attached = false;
    const sync = () => {
      const r = physRect(el);
      if (!attached) {
        attached = true;
        commands.previewAttach(r);
      } else {
        commands.previewResize(r);
      }
    };
    // Đợi layout ổn định rồi mới gắn.
    const raf = requestAnimationFrame(sync);
    const ro = new ResizeObserver(sync);
    ro.observe(el);
    window.addEventListener("resize", sync);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      window.removeEventListener("resize", sync);
      commands.previewDetach();
    };
  }, []);
  return <div ref={slot} className="preview-native" />;
}
