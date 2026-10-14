//! Hàng đợi tác vụ nền. Một thread làm lần lượt từng việc.
//! K-S4: `suspend()` khi bắt đầu quay → việc đang chạy bị dừng giữa chừng và quay lại đầu hàng đợi,
//! không việc nào chạy cho tới `resume()` (dừng quay).

use crate::proxy::{self, ProxyError};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum JobStatus {
    Queued,
    #[serde(rename_all = "camelCase")]
    Running { progress: f32 },
    /// Đang quay → hoãn (K-S4).
    Suspended,
    #[serde(rename_all = "camelCase")]
    Ready { path: PathBuf },
    #[serde(rename_all = "camelCase")]
    Failed { message: String },
}

/// Một việc tạo proxy: media `key` (id media), từ `source` ra `target`.
#[derive(Debug, Clone)]
pub struct ProxyJob {
    pub key: String,
    pub source: PathBuf,
    pub target: PathBuf,
}

type Listener = Box<dyn Fn(&str, &JobStatus) + Send + Sync>;

struct Inner {
    queue: VecDeque<ProxyJob>,
    status: HashMap<String, JobStatus>,
    suspended: bool,
    current: Option<(String, Arc<AtomicBool>)>,
    shutdown: bool,
}

#[derive(Clone)]
pub struct JobScheduler {
    inner: Arc<(Mutex<Inner>, Condvar)>,
    listener: Arc<Mutex<Option<Listener>>>,
}

impl Default for JobScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl JobScheduler {
    pub fn new() -> Self {
        let s = Self {
            inner: Arc::new((
                Mutex::new(Inner { queue: VecDeque::new(), status: HashMap::new(), suspended: false, current: None, shutdown: false }),
                Condvar::new(),
            )),
            listener: Arc::new(Mutex::new(None)),
        };
        let worker = s.clone();
        std::thread::Builder::new()
            .name("mochi-jobs".into())
            .spawn(move || worker.run())
            .expect("không tạo được thread tác vụ nền");
        s
    }

    /// Nhận mọi thay đổi trạng thái (để app phát sự kiện cho UI).
    pub fn on_update(&self, f: impl Fn(&str, &JobStatus) + Send + Sync + 'static) {
        *self.listener.lock().unwrap() = Some(Box::new(f));
    }

    fn set(&self, inner: &mut Inner, key: &str, st: JobStatus) {
        inner.status.insert(key.to_string(), st.clone());
        if let Some(l) = self.listener.lock().unwrap().as_ref() {
            l(key, &st);
        }
    }

    /// Thêm việc (bỏ qua nếu đã xong / đang chờ / đang chạy). Proxy đã có trên đĩa → `Ready` ngay.
    pub fn push(&self, job: ProxyJob) {
        let (m, cv) = &*self.inner;
        let mut inner = m.lock().unwrap();
        if job.target.exists() {
            self.set(&mut inner, &job.key.clone(), JobStatus::Ready { path: job.target });
            return;
        }
        let busy = inner.queue.iter().any(|j| j.key == job.key) || inner.current.as_ref().is_some_and(|c| c.0 == job.key);
        if busy {
            return;
        }
        let st = if inner.suspended { JobStatus::Suspended } else { JobStatus::Queued };
        self.set(&mut inner, &job.key.clone(), st);
        inner.queue.push_back(job);
        cv.notify_all();
    }

    /// Thử lại việc đã lỗi.
    pub fn retry(&self, job: ProxyJob) {
        {
            let mut inner = self.inner.0.lock().unwrap();
            inner.status.remove(&job.key);
        }
        self.push(job);
    }

    /// K-S4: bắt đầu quay → dừng việc đang chạy, hoãn mọi việc.
    pub fn suspend(&self) {
        let (m, _) = &*self.inner;
        let mut inner = m.lock().unwrap();
        if inner.suspended {
            return;
        }
        inner.suspended = true;
        if let Some((_, cancel)) = &inner.current {
            cancel.store(true, Ordering::SeqCst);
        }
        let keys: Vec<String> = inner.queue.iter().map(|j| j.key.clone()).collect();
        for k in keys {
            self.set(&mut inner, &k, JobStatus::Suspended);
        }
        tracing::info!("tác vụ nền: tạm hoãn (đang quay)");
    }

    pub fn resume(&self) {
        let (m, cv) = &*self.inner;
        let mut inner = m.lock().unwrap();
        if !inner.suspended {
            return;
        }
        inner.suspended = false;
        let keys: Vec<String> = inner.queue.iter().map(|j| j.key.clone()).collect();
        for k in keys {
            self.set(&mut inner, &k, JobStatus::Queued);
        }
        cv.notify_all();
        tracing::info!("tác vụ nền: chạy tiếp");
    }

    pub fn is_suspended(&self) -> bool {
        self.inner.0.lock().unwrap().suspended
    }

    pub fn status(&self) -> HashMap<String, JobStatus> {
        self.inner.0.lock().unwrap().status.clone()
    }

    /// Đường dẫn proxy đã sẵn sàng (media id → file).
    pub fn ready(&self) -> HashMap<String, PathBuf> {
        self.status()
            .into_iter()
            .filter_map(|(k, s)| match s {
                JobStatus::Ready { path } if path.exists() => Some((k, path)),
                _ => None,
            })
            .collect()
    }

    /// Quên trạng thái của các media (proxy bị dọn khỏi cache).
    pub fn forget(&self, keys: &[String]) {
        let mut inner = self.inner.0.lock().unwrap();
        for k in keys {
            inner.status.remove(k);
        }
    }

    fn run(&self) {
        let (m, cv) = &*self.inner;
        loop {
            let (job, cancel) = {
                let mut inner = m.lock().unwrap();
                while !inner.shutdown && (inner.suspended || inner.queue.is_empty()) {
                    inner = cv.wait(inner).unwrap();
                }
                if inner.shutdown {
                    return;
                }
                let job = inner.queue.pop_front().unwrap();
                let cancel = Arc::new(AtomicBool::new(false));
                inner.current = Some((job.key.clone(), cancel.clone()));
                self.set(&mut inner, &job.key, JobStatus::Running { progress: 0.0 });
                (job, cancel)
            };
            // Bản quay (thư mục) → proxy là một thư mục; video → một file.
            let is_recording = job.source.is_dir();
            let tmp = job.target.with_extension(if is_recording { "partial" } else { "partial.mp4" });
            if let Some(dir) = tmp.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let mut last = -1.0f32;
            let report = |p: f32| {
                if p - last >= 0.02 || p >= 1.0 {
                    last = p;
                    let mut inner = m.lock().unwrap();
                    // Đã bị hoãn trong lúc chạy → giữ trạng thái Suspended, không ghi đè bằng tiến độ.
                    if !inner.suspended {
                        self.set(&mut inner, &job.key, JobStatus::Running { progress: p });
                    }
                }
            };
            let result = if is_recording {
                proxy::make_recording(&job.source, &tmp, &cancel, report)
            } else {
                proxy::make(&job.source, &tmp, &cancel, report)
            };
            let mut inner = m.lock().unwrap();
            inner.current = None;
            match result {
                Ok(()) => match std::fs::rename(&tmp, &job.target) {
                    Ok(()) => self.set(&mut inner, &job.key, JobStatus::Ready { path: job.target.clone() }),
                    Err(e) => self.set(&mut inner, &job.key, JobStatus::Failed { message: e.to_string() }),
                },
                Err(ProxyError::Cancelled) => {
                    remove_any(&tmp);
                    tracing::info!("proxy {} bị dừng vì đang quay, sẽ làm lại", job.key);
                    let key = job.key.clone();
                    inner.queue.push_front(job);
                    self.set(&mut inner, &key, JobStatus::Suspended);
                }
                Err(e) => {
                    remove_any(&tmp);
                    tracing::warn!("proxy {} lỗi: {e}", job.key);
                    self.set(&mut inner, &job.key, JobStatus::Failed { message: e.to_string() });
                }
            }
        }
    }
}

/// Xóa file hoặc thư mục dở dang của một proxy.
fn remove_any(p: &std::path::Path) {
    if p.is_dir() {
        let _ = std::fs::remove_dir_all(p);
    } else {
        let _ = std::fs::remove_file(p);
    }
}
