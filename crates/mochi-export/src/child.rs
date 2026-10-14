//! Xuất ở tiến trình riêng (X04): editor không chết theo khi encoder / driver crash.
//!
//! - Tiến trình con = chính file exe của app (hoặc `mochi-cli`) chạy với `--export-job <job.json>`.
//!   Nó in mỗi dòng stdout một `ChildMsg` (JSON), đọc stdin: dòng `cancel` = hủy.
//! - Tiến trình cha (`spawn`) đọc các dòng đó; tiến trình con thoát mà không gửi `Done`/`Failed`
//!   → `ExportError::Crashed` kèm đường dẫn log, và cha tự xóa file tạm.

use crate::error::ExportError;
use crate::job::{ChildMsg, ExportJob};
use crate::settings::partial_path;
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub const CHILD_FLAG: &str = "--export-job";

/// `argv` có `--export-job <path>` không (để `main` rẽ nhánh trước khi mở UI).
pub fn job_arg() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        if a == CHILD_FLAG {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

fn init_log(path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(file) = std::fs::File::create(path) {
        let _ = tracing_subscriber::fmt()
            // Không theo RUST_LOG của tiến trình cha: log xuất luôn đủ chi tiết để điều tra crash.
            .with_env_filter(tracing_subscriber::EnvFilter::new("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"))
            .with_ansi(false)
            .with_writer(Mutex::new(file))
            .try_init();
    }
}

/// Điểm vào của tiến trình con. Trả mã thoát: 0 = xong, 1 = lỗi đã báo, 2 = hủy.
pub async fn child_main(job_path: &Path) -> i32 {
    let job: ExportJob = match std::fs::read(job_path).map_err(|e| e.to_string()).and_then(|b| {
        serde_json::from_slice(&b).map_err(|e| e.to_string())
    }) {
        Ok(j) => j,
        Err(e) => {
            emit(&ChildMsg::Failed { error: ExportError::internal(format!("đọc job: {e}")) });
            return 1;
        }
    };
    init_log(&job.log);
    tracing::info!("bắt đầu job xuất {} → {}", job.id, job.output.display());

    let cancel = Arc::new(AtomicBool::new(false));
    // Kiểm thử: `MOCHI_TEST_EXPORT_IGNORE_CANCEL=1` → không nghe lệnh hủy (giả lập hủy chậm, bị kill).
    if mochi_project::test_env("MOCHI_TEST_EXPORT_IGNORE_CANCEL").is_none() {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                match line {
                    Ok(l) if l.trim() == "cancel" => cancel.store(true, Ordering::SeqCst),
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            // stdin đóng = tiến trình cha đã chết → hủy, không để file dở.
            cancel.store(true, Ordering::SeqCst);
        });
    }

    let mut on_msg = |m: ChildMsg| emit(&m);
    match crate::job::run(&job, &cancel, &mut on_msg).await {
        Ok(done) => {
            tracing::info!("xong: {done:?}");
            emit(&done);
            0
        }
        Err(ExportError::Cancelled) => {
            tracing::info!("đã hủy");
            emit(&ChildMsg::Failed { error: ExportError::Cancelled });
            2
        }
        Err(e) => {
            tracing::error!("lỗi: {e}");
            emit(&ChildMsg::Failed { error: e });
            1
        }
    }
}

/// Số log xuất (của các lần lỗi) giữ lại trong thư mục log.
pub const KEEP_LOGS: usize = 20;

/// Giữ `keep` log mới nhất; xóa `*.job.json` còn sót (tiến trình cha chết giữa chừng).
fn prune_logs(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut logs = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        if name.ends_with(".job.json") {
            let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|d| d > Duration::from_secs(24 * 3600)));
            if old {
                let _ = std::fs::remove_file(&p);
            }
        } else if name.ends_with(".log") {
            if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
                logs.push((t, p));
            }
        }
    }
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in logs.into_iter().skip(keep) {
        let _ = std::fs::remove_file(p);
    }
}

fn emit(m: &ChildMsg) {
    let mut out = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut out, m);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

/// Tiến trình con đang chạy, nhìn từ phía cha.
pub struct ChildExport {
    child: Arc<Mutex<Child>>,
    /// Người dùng đã bấm hủy: tiến trình con có bị kill sau 10 s thì vẫn là "đã hủy", không phải crash.
    cancelled: Arc<AtomicBool>,
    stdin: Mutex<Option<ChildStdin>>,
    reader: Mutex<Option<std::thread::JoinHandle<Result<ChildMsg, ExportError>>>>,
}

impl ChildExport {
    /// Ghi `job` ra `<log dir>/job.json` rồi chạy `exe [prefix_args…] --export-job job.json`.
    /// `on_msg` nhận mọi tin nhắn (Started / Progress / Fallback) trên thread đọc.
    pub fn spawn(
        exe: &Path,
        prefix_args: &[&str],
        job: &ExportJob,
        on_msg: impl FnMut(ChildMsg) + Send + 'static,
    ) -> Result<Self, ExportError> {
        let job_path = job.log.with_file_name(format!("{}.job.json", job.id));
        if let Some(dir) = job_path.parent() {
            std::fs::create_dir_all(dir).map_err(ExportError::internal)?;
            prune_logs(dir, KEEP_LOGS);
        }
        std::fs::write(&job_path, serde_json::to_vec(job).map_err(ExportError::internal)?)
            .map_err(ExportError::internal)?;
        let mut cmd = Command::new(exe);
        cmd.args(prefix_args)
            .arg(CHILD_FLAG)
            .arg(&job_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
            cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
        }
        let mut child = cmd.spawn().map_err(|e| ExportError::internal(format!("không chạy được tiến trình xuất: {e}")))?;
        let stdout = child.stdout.take().unwrap();
        let stdin = child.stdin.take();
        let child = Arc::new(Mutex::new(child));
        let partial = partial_path(&job.output);
        let log = job.log.clone();
        let waiter = child.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let was_cancelled = cancelled.clone();
        let job_file = job_path.clone();
        let mut on_msg = on_msg;
        let reader = std::thread::Builder::new()
            .name("mochi-export-reader".into())
            .spawn(move || {
                let mut last = None;
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    match serde_json::from_str::<ChildMsg>(&line) {
                        Ok(m @ (ChildMsg::Done { .. } | ChildMsg::Failed { .. })) => last = Some(m),
                        Ok(m) => on_msg(m),
                        Err(_) => tracing::debug!("export child: {line}"),
                    }
                }
                let status = loop {
                    if let Some(s) = waiter.lock().unwrap().try_wait().ok().flatten() {
                        break Some(s);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                };
                // job.json chứa bản chụp project + đường dẫn media: không để lại sau khi xong.
                let _ = std::fs::remove_file(&job_file);
                let result = match last {
                    Some(ChildMsg::Failed { error }) => Err(error),
                    Some(done) => Ok(done),
                    None => {
                        // Tiến trình con thoát không báo gì: dọn file tạm thay cho nó.
                        let _ = std::fs::remove_file(&partial);
                        if was_cancelled.load(Ordering::SeqCst) {
                            Err(ExportError::Cancelled) // hủy chậm quá 10 s → bị kill, vẫn là "đã hủy"
                        } else {
                            Err(ExportError::Crashed { exit_code: status.and_then(|s| s.code()), log: log.clone() })
                        }
                    }
                };
                // Log chỉ giữ khi có lỗi (để điều tra / nút "Mở log").
                if matches!(result, Ok(_) | Err(ExportError::Cancelled)) {
                    let _ = std::fs::remove_file(&log);
                }
                result
            })
            .map_err(ExportError::internal)?;
        Ok(Self { child, cancelled, stdin: Mutex::new(stdin), reader: Mutex::new(Some(reader)) })
    }

    /// Hủy êm (tiến trình con tự xóa file tạm); quá 10 s thì kill.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(stdin) = self.stdin.lock().unwrap().as_mut() {
            let _ = stdin.write_all(b"cancel\n");
            let _ = stdin.flush();
        }
        let child = self.child.clone();
        std::thread::spawn(move || {
            for _ in 0..200 {
                if child.lock().unwrap().try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = child.lock().unwrap().kill();
        });
    }

    /// Kill ngay (kiểm thử crash).
    pub fn kill(&self) {
        let _ = self.child.lock().unwrap().kill();
    }

    pub fn pid(&self) -> u32 {
        self.child.lock().unwrap().id()
    }

    /// Tiến trình con đã kết thúc và đã đọc hết kết quả chưa.
    pub fn is_finished(&self) -> bool {
        self.reader.lock().unwrap().as_ref().is_none_or(|r| r.is_finished())
    }

    /// Chờ tiến trình con kết thúc (chỉ gọi một lần).
    pub fn wait(&self) -> Result<ChildMsg, ExportError> {
        let reader = self.reader.lock().unwrap().take();
        match reader {
            Some(r) => r.join().unwrap_or_else(|_| Err(ExportError::internal("thread đọc bị panic"))),
            None => Err(ExportError::internal("đã chờ rồi")),
        }
    }
}
