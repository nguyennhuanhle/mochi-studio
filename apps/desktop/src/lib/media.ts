import { useEffect, useRef, useState } from "react";
import { commands } from "../bindings";

/** Lỗi xem trước: "denied" = Windows chặn quyền (có nút mở Cài đặt), "failed" = lỗi khác. */
export type PreviewError = "denied" | "failed" | null;

function previewError(e: unknown): PreviewError {
  const name = (e as { name?: string })?.name;
  return name === "NotAllowedError" || name === "SecurityError" ? "denied" : "failed";
}

/** Peak 0..1 → mức hiển thị 0..1 (thang dB -60..0). */
function peakToLevel(peak: number): number {
  const db = peak > 0 ? 20 * Math.log10(peak) : -96;
  return Math.min(1, Math.max(0, (db + 60) / 60));
}

/**
 * Xem trước webcam / đo mức mic trong màn thiết lập quay (R02), bằng getUserMedia của WebView2.
 * Tên thiết bị của Cap (Windows) khớp phần đầu nhãn của trình duyệt, ví dụ
 * "1080P Web Camera" ↔ "1080P Web Camera (2bdf:0280)".
 * Phải gọi `release()` trước khi bắt đầu quay để pipeline Cap mở được thiết bị.
 */
async function findDevice(kind: MediaDeviceKind, name: string): Promise<string | undefined> {
  let devices = await navigator.mediaDevices.enumerateDevices();
  if (!devices.some((d) => d.kind === kind && d.label)) {
    // Nhãn chỉ có sau khi đã cấp quyền một lần.
    const probe = await navigator.mediaDevices.getUserMedia(
      kind === "videoinput" ? { video: true } : { audio: true },
    );
    probe.getTracks().forEach((t) => t.stop());
    devices = await navigator.mediaDevices.enumerateDevices();
  }
  const norm = (s: string) => s.toLowerCase().trim();
  const target = norm(name);
  return devices.find(
    (d) => d.kind === kind && (norm(d.label) === target || norm(d.label).startsWith(target)),
  )?.deviceId;
}

export function useCameraPreview(name: string | null) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const [error, setError] = useState<PreviewError>(null);

  useEffect(() => {
    let cancelled = false;
    setError(null);
    (async () => {
      if (!name) return;
      try {
        const deviceId = await findDevice("videoinput", name);
        const stream = await navigator.mediaDevices.getUserMedia({
          video: deviceId ? { deviceId: { exact: deviceId } } : true,
        });
        if (cancelled) return stream.getTracks().forEach((t) => t.stop());
        streamRef.current = stream;
        if (videoRef.current) videoRef.current.srcObject = stream;
      } catch (e) {
        if (!cancelled) setError(previewError(e));
      }
    })();
    return () => {
      cancelled = true;
      streamRef.current?.getTracks().forEach((t) => t.stop());
      streamRef.current = null;
    };
  }, [name]);

  const release = () => {
    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;
  };
  return { videoRef, error, release };
}

/** Mức đỉnh của mic, 0..1 (thang dB -60..0), cập nhật ~30 lần/giây. */
export function useMicLevel(name: string | null) {
  const [level, setLevel] = useState(0);
  const [error, setError] = useState<PreviewError>(null);
  const streamRef = useRef<MediaStream | null>(null);

  useEffect(() => {
    let cancelled = false;
    let raf = 0;
    let ctx: AudioContext | null = null;
    setError(null);
    (async () => {
      if (!name) return;
      try {
        const deviceId = await findDevice("audioinput", name);
        const stream = await navigator.mediaDevices.getUserMedia({
          audio: deviceId ? { deviceId: { exact: deviceId } } : true,
        });
        if (cancelled) return stream.getTracks().forEach((t) => t.stop());
        streamRef.current = stream;
        ctx = new AudioContext();
        const analyser = ctx.createAnalyser();
        analyser.fftSize = 1024;
        ctx.createMediaStreamSource(stream).connect(analyser);
        const buf = new Float32Array(analyser.fftSize);
        let shown = 0;
        const tick = () => {
          analyser.getFloatTimeDomainData(buf);
          let peak = 0;
          for (const v of buf) peak = Math.max(peak, Math.abs(v));
          const target = peakToLevel(peak);
          shown = target > shown ? target : shown * 0.92; // lên nhanh, xuống chậm
          setLevel(shown);
          raf = requestAnimationFrame(tick);
        };
        tick();
      } catch (e) {
        if (!cancelled) setError(previewError(e));
      }
    })();
    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
      ctx?.close();
      streamRef.current?.getTracks().forEach((t) => t.stop());
      streamRef.current = null;
      setLevel(0);
    };
  }, [name]);

  const release = () => {
    streamRef.current?.getTracks().forEach((t) => t.stop());
    streamRef.current = null;
  };
  return { level, error, release };
}

/**
 * R02: mức âm thanh hệ thống (loopback của thiết bị phát mặc định — đúng nguồn bản quay dùng).
 * Đo ở Rust, hỏi ~10 lần/giây; tắt khi `enabled` = false hoặc rời trang.
 */
export function useSystemAudioLevel(enabled: boolean) {
  const [level, setLevel] = useState(0);
  const [device, setDevice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    let shown = 0;
    const timer = window.setInterval(async () => {
      const r = await commands.systemAudioLevel();
      if (!alive) return;
      if (r.status === "error") {
        setError(r.error);
        return;
      }
      setError(null);
      setDevice(r.data.device);
      const target = peakToLevel(r.data.peak);
      shown = target > shown ? target : shown * 0.8;
      setLevel(shown);
    }, 100);
    return () => {
      alive = false;
      clearInterval(timer);
      setLevel(0);
      commands.systemAudioMeterStop();
    };
  }, [enabled]);

  return { level, device, error };
}
