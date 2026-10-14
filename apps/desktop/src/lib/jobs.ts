import { useSyncExternalStore } from "react";
import { commands, events, type JobStatus } from "../bindings";

/** Trạng thái tác vụ nền (proxy) theo media id, giữ ở cấp module. */
let status: Record<string, JobStatus> = {};
const subs = new Set<() => void>();
let listening = false;

function listen() {
  if (listening) return;
  listening = true;
  commands.jobsStatus().then((s) => {
    status = { ...s, ...status };
    subs.forEach((f) => f());
  });
  events.jobsUpdate.listen(({ payload }) => {
    status = { ...status, [payload.mediaId]: payload.status };
    subs.forEach((f) => f());
  });
}

export function useJobs(): Record<string, JobStatus> {
  listen();
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => status,
  );
}
