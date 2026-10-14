import { useEffect, useState } from "react";
import { events } from "../bindings";

/** Lớp đếm ngược giữa màn hình (R04); trong suốt, không nhận chuột, không lọt vào bản quay. */
export default function Countdown() {
  const [n, setN] = useState<number | null>(null);
  useEffect(() => {
    document.documentElement.classList.add("overlay-window");
    const un = events.recorderEventMsg.listen(({ payload }) => {
      if (payload.type === "countdown") setN(payload.remaining);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);
  return <div className="countdown">{n !== null && <span key={n}>{n}</span>}</div>;
}
