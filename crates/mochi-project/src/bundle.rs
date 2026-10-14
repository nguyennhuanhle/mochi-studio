//! X07 (v1): đóng gói project + toàn bộ media thành một file .zip để chuyển sang máy khác, và mở
//! lại gói đó. Trong zip: `<Tên>.mochi/project.json` + `recordings/` + `media/` (media ngoài gói được
//! chép vào `media/`). `relPath` của mọi media trỏ vào trong gói → máy kia mở là thấy đủ media.
//! Bỏ qua `cache/`, `backups/`, file khóa (tạo lại được).

use crate::{MediaItem, MediaKind, Project, ProjectError};
use std::{
    collections::HashSet,
    fs::File,
    io::{self, BufWriter},
    path::{Path, PathBuf},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

/// Kết quả đóng gói.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PackResult {
    pub path: PathBuf,
    #[specta(type = f64)]
    pub bytes: u64,
    /// Tên media không tìm thấy (không có trong gói).
    pub missing: Vec<String>,
}

/// Một tệp sẽ đưa vào zip: (đường dẫn thật, tên trong zip).
type Entry = (PathBuf, String);

fn io_err(e: impl std::fmt::Display) -> ProjectError {
    ProjectError::Io { detail: e.to_string() }
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<Entry>) -> io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().to_string();
        let p = e.path();
        if p.is_dir() {
            walk(&p, &format!("{prefix}{name}/"), out)?;
        } else {
            out.push((p, format!("{prefix}{name}")));
        }
    }
    Ok(())
}

/// Đóng gói `project` (gói đang mở ở `project_dir`) vào `out`. `resolve` cho đường dẫn thật của media.
/// `progress(done_bytes, total_bytes)` được gọi trong lúc chép.
pub fn pack(
    project: &Project,
    project_dir: &Path,
    resolve: impl Fn(&MediaItem) -> Option<PathBuf>,
    out: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<PackResult, ProjectError> {
    let root = format!(
        "{}/",
        project_dir.file_name().and_then(|n| n.to_str()).unwrap_or("Project.mochi")
    );
    let mut project = project.clone();
    let mut entries: Vec<Entry> = Vec::new();
    let mut missing = Vec::new();
    let mut used: HashSet<String> = HashSet::new();
    for m in project.media.iter_mut() {
        let Some(path) = resolve(m) else {
            missing.push(m.name.clone());
            continue;
        };
        // Đã nằm trong gói → giữ vị trí tương đối; ngoài gói → chép vào media/ (tên không trùng).
        let rel = match path.strip_prefix(project_dir) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => {
                let base = path.file_name().and_then(|n| n.to_str()).unwrap_or("media").to_string();
                let folder = if m.kind == MediaKind::Recording { "recordings" } else { "media" };
                let mut name = format!("{folder}/{base}");
                let mut i = 2;
                while !used.insert(name.clone()) {
                    name = format!("{folder}/{i}-{base}");
                    i += 1;
                }
                name
            }
        };
        if m.kind == MediaKind::Recording {
            walk(&path, &format!("{root}{rel}/"), &mut entries).map_err(io_err)?;
        } else {
            entries.push((path.clone(), format!("{root}{rel}")));
        }
        m.rel_path = Some(rel);
    }
    // Bản quay trong gói không có trên timeline vẫn đi kèm (để không mất dữ liệu).
    let recs = project_dir.join(crate::package::RECORDINGS_DIR);
    if recs.is_dir() {
        let mut extra = Vec::new();
        walk(&recs, &format!("{root}{}/", crate::package::RECORDINGS_DIR), &mut extra).map_err(io_err)?;
        let have: HashSet<String> = entries.iter().map(|(_, n)| n.clone()).collect();
        entries.extend(extra.into_iter().filter(|(_, n)| !have.contains(n)));
    }

    let total: u64 = entries.iter().filter_map(|(p, _)| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    let tmp = out.with_extension("zip.partial");
    let result = (|| -> Result<(), ProjectError> {
        let mut zip = ZipWriter::new(BufWriter::new(File::create(&tmp).map_err(io_err)?));
        let json = serde_json::to_vec_pretty(&project).map_err(io_err)?;
        let deflate = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file(format!("{root}project.json"), deflate).map_err(io_err)?;
        io::Write::write_all(&mut zip, &json).map_err(io_err)?;
        // Video / âm thanh vốn đã nén → lưu nguyên (nhanh, không phình).
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored).large_file(true);
        let mut done = 0u64;
        for (path, name) in &entries {
            zip.start_file(name.as_str(), stored).map_err(io_err)?;
            let mut f = File::open(path).map_err(io_err)?;
            done += io::copy(&mut f, &mut zip).map_err(io_err)?;
            progress(done, total);
        }
        zip.finish().map_err(io_err)?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if out.exists() {
        std::fs::remove_file(out).map_err(io_err)?;
    }
    std::fs::rename(&tmp, out).map_err(io_err)?;
    let bytes = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    Ok(PackResult { path: out.to_path_buf(), bytes, missing })
}

/// Giải nén gói `zip` vào thư mục `dest_parent`; trả về thư mục project (`*.mochi`) bên trong.
/// Không ghi đè: thư mục project đã tồn tại → lỗi.
pub fn unpack(zip: &Path, dest_parent: &Path) -> Result<PathBuf, ProjectError> {
    let mut archive = ZipArchive::new(File::open(zip).map_err(io_err)?).map_err(io_err)?;
    let project_json = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().and_then(|f| f.enclosed_name()))
        .find(|p| p.file_name().is_some_and(|n| n == "project.json") && p.components().count() == 2)
        .ok_or_else(|| ProjectError::Unsupported { detail: "không phải gói project Mochi (thiếu project.json)".into() })?;
    let top = project_json.parent().map(Path::to_path_buf).unwrap_or_default();
    let dest = dest_parent.join(&top);
    if dest.exists() {
        return Err(ProjectError::Io { detail: format!("đã có thư mục {}", dest.display()) });
    }
    // `enclosed_name` của zip chặn đường dẫn thoát ra ngoài (../).
    archive.extract(dest_parent).map_err(io_err)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ticks, new_project};

    #[test]
    fn pack_then_unpack_keeps_media_reachable() {
        let tmp = std::env::temp_dir().join(format!("mochi-bundle-{}", uuid::Uuid::new_v4().simple()));
        let pkg = tmp.join("Demo.mochi");
        let outside = tmp.join("ngoai");
        std::fs::create_dir_all(pkg.join("recordings/rec1/content")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(pkg.join("recordings/rec1/recording-meta.json"), b"{}").unwrap();
        std::fs::write(pkg.join("recordings/rec1/content/display.mp4"), vec![7u8; 4096]).unwrap();
        std::fs::write(outside.join("nhac.mp3"), vec![1u8; 1000]).unwrap();
        std::fs::create_dir_all(pkg.join("cache")).unwrap();
        std::fs::write(pkg.join("cache/proxy.mp4"), b"x").unwrap();

        let item = |id: &str, kind, abs: PathBuf| MediaItem {
            id: id.into(),
            kind,
            name: abs.file_name().unwrap().to_string_lossy().into(),
            abs_path: abs,
            rel_path: None,
            hash: id.into(),
            size_bytes: 1.0,
            duration: Ticks::from_secs(1.0),
            video: None,
            audio: None,
            recording: None,
        };
        let mut p = new_project("Demo");
        p.media.push(item("r", MediaKind::Recording, pkg.join("recordings/rec1")));
        p.media.push(item("a", MediaKind::Audio, outside.join("nhac.mp3")));
        p.media.push(item("gone", MediaKind::Video, outside.join("mat.mp4")));
        let zip_path = tmp.join("Demo.zip");
        let r = pack(&p, &pkg, |m| crate::media::resolve(&pkg, m), &zip_path, |_, _| {}).unwrap();
        assert_eq!(r.missing, vec!["mat.mp4".to_string()]);

        let dest = tmp.join("may-khac");
        std::fs::create_dir_all(&dest).unwrap();
        let opened = unpack(&zip_path, &dest).unwrap();
        assert_eq!(opened, dest.join("Demo.mochi"));
        assert!(!opened.join("cache").exists(), "cache không đi kèm");
        let back: Project = serde_json::from_slice(&std::fs::read(opened.join("project.json")).unwrap()).unwrap();
        let rec = back.media.iter().find(|m| m.id == "r").unwrap();
        let audio = back.media.iter().find(|m| m.id == "a").unwrap();
        assert_eq!(rec.rel_path.as_deref(), Some("recordings/rec1"));
        assert_eq!(audio.rel_path.as_deref(), Some("media/nhac.mp3"));
        // Máy kia: tìm được media qua relPath dù đường dẫn tuyệt đối cũ không còn.
        std::fs::remove_dir_all(&pkg).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
        assert_eq!(crate::media::resolve(&opened, rec).unwrap(), opened.join("recordings/rec1"));
        assert_eq!(std::fs::read(crate::media::resolve(&opened, audio).unwrap()).unwrap().len(), 1000);
        // Giải nén lần nữa vào cùng chỗ → không ghi đè.
        assert!(unpack(&zip_path, &dest).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
