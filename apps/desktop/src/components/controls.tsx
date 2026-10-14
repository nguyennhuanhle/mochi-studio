import { useEffect, useRef, useState, type ReactNode } from "react";

/**
 * Thanh trượt chỉ gửi lệnh khi thả chuột / nhả phím — một lần kéo = một bước undo,
 * không làm ngập lịch sử. Giá trị hiển thị cập nhật liên tục khi kéo.
 */
export function Slider(props: {
  label: ReactNode;
  value: number;
  min: number;
  max: number;
  step?: number;
  format?: (v: number) => string;
  onCommit: (v: number) => void;
}) {
  const [v, setV] = useState(props.value);
  useEffect(() => setV(props.value), [props.value]);
  const commit = () => v !== props.value && props.onCommit(v);
  return (
    <label className="field">
      <span className="row space">
        <span>{props.label}</span>
        <span className="muted small">
          {props.format ? props.format(v) : Math.round(v)}
          {/* Hiển thị giới hạn: giá trị ngoài khoảng này bị kẹp lại. */}
          <span className="slider-range">
            {" "}
            ({props.format ? props.format(props.min) : props.min}–{props.format ? props.format(props.max) : props.max})
          </span>
        </span>
      </span>
      <input
        type="range"
        min={props.min}
        max={props.max}
        step={props.step ?? 1}
        value={v}
        onChange={(e) => setV(Number(e.target.value))}
        onPointerUp={commit}
        onKeyUp={commit}
      />
    </label>
  );
}

/** Ô màu: gửi lệnh sau khi ngừng chọn 350 ms. */
export function ColorField(props: { label: ReactNode; value: string; onCommit: (v: string) => void }) {
  const [v, setV] = useState(props.value);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => setV(props.value), [props.value]);
  return (
    <label className="row space">
      <span>{props.label}</span>
      <input
        type="color"
        value={v.slice(0, 7)}
        onChange={(e) => {
          const next = e.target.value.toUpperCase();
          setV(next);
          window.clearTimeout(timer.current);
          timer.current = window.setTimeout(() => props.onCommit(next), 350);
        }}
      />
    </label>
  );
}

export function Check(props: { label: ReactNode; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="check">
      <input type="checkbox" checked={props.checked} onChange={(e) => props.onChange(e.target.checked)} />
      <span>{props.label}</span>
    </label>
  );
}

export function Select<T extends string>(props: {
  label: ReactNode;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <label className="field">
      <span>{props.label}</span>
      <select value={props.value} onChange={(e) => props.onChange(e.target.value as T)}>
        {props.options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}

export function Section(props: { title: string; children: ReactNode; defaultOpen?: boolean }) {
  return (
    <details className="insp-section" open={props.defaultOpen ?? true}>
      <summary>{props.title}</summary>
      <div className="stack">{props.children}</div>
    </details>
  );
}
