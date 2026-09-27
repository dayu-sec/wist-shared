//! Shared filesystem helpers.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

pub fn ensure_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn read_json<T>(path: &Path) -> io::Result<T>
where
    T: DeserializeOwned,
{
    let text = fs::read_to_string(path)?;
    serde_json::from_str(&text).map_err(io::Error::other)
}

pub fn write_json_atomic<T>(path: &Path, value: &T) -> io::Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    write_bytes_atomic(path, &bytes)
}

pub fn write_json_private_atomic<T>(path: &Path, value: &T) -> io::Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    write_bytes_private_atomic(path, &bytes)
}

pub fn write_json_compact_atomic<T>(path: &Path, value: &T) -> io::Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    write_bytes_atomic(path, &bytes)
}

/// 原子写字节：先写临时文件 `*.tmp`、fsync、再 rename 覆盖目标，最后 fsync 父目录。
///
/// 注意：**总是追加一个结尾换行**（`b"\n"`）—— 本仓的调用方据此把 JSON 写成行式文件；
/// 若要写不含换行的二进制，请自行处理。父目录会自动创建。
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    ensure_parent(path)?;

    let tmp_path = path.with_extension("tmp");
    let mut file = File::create(&tmp_path)?;
    file.write_all(bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);

    fs::rename(&tmp_path, path)?;
    sync_parent_dir(path)?;
    Ok(())
}

/// 同 [`write_bytes_atomic`]，但把临时文件与最终文件设为 0600，并把**父目录**收为 0700
/// （敏感文件如 `agent_runtime.json` 落盘用）。裸文件名（无父目录）时跳过父目录改权限。
#[cfg(unix)]
pub fn write_bytes_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    ensure_parent(path)?;
    // 裸文件名（无目录部分）的 parent 是空路径：`set_permissions("")` 会 ENOENT。
    if let Some(parent) = non_empty_parent(path) {
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }

    let tmp_path = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&tmp_path)?;
    file.write_all(bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);

    fs::rename(&tmp_path, path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    sync_parent_dir(path)?;
    Ok(())
}

#[cfg(not(unix))]
pub fn write_bytes_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_bytes_atomic(path, bytes)
}

/// 取父目录，但把"空路径"（裸文件名的父）视为没有父目录。
///
/// `Path::new("a.json").parent()` 返回 `Some("")`，而 `File::open("")` / `set_permissions("")`
/// 都会 ENOENT —— 若直接使用，会让"写成功"却返回错误（`sync_parent_dir`）。
#[cfg(unix)]
fn non_empty_parent(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

#[cfg(unix)]
fn sync_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = non_empty_parent(path) {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// 每个测试一个独立临时目录（不依赖外部 crate）。
    fn temp_dir(tag: &str) -> PathBuf {
        let unique = format!(
            "wist-shared-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn write_then_read_json_round_trips() {
        #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        struct Payload {
            name: String,
            count: u32,
        }
        let dir = temp_dir("json-round-trip");
        let path = dir.join("nested/deeper/data.json");
        let value = Payload {
            name: "wist".into(),
            count: 7,
        };
        write_json_atomic(&path, &value).expect("write");
        assert!(path.exists(), "父目录应被自动创建");
        let read: Payload = read_json(&path).expect("read");
        assert_eq!(read, value);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn compact_json_has_no_pretty_whitespace_and_ends_with_newline() {
        let dir = temp_dir("json-compact");
        let path = dir.join("c.json");
        write_json_compact_atomic(&path, &[1, 2, 3]).expect("write");
        let text = fs::read_to_string(&path).expect("read");
        assert_eq!(text, "[1,2,3]\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pretty_json_ends_with_exactly_one_newline() {
        let dir = temp_dir("json-pretty");
        let path = dir.join("p.json");
        write_json_atomic(&path, &serde_json::json!({ "a": 1 })).expect("write");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.ends_with('\n'));
        assert!(!text.ends_with("\n\n"));
        // 美化输出应当能被读回。
        let value: serde_json::Value = read_json(&path).expect("read");
        assert_eq!(value["a"], serde_json::json!(1));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_bytes_appends_exactly_one_trailing_newline() {
        let dir = temp_dir("bytes-newline");
        let path = dir.join("b.bin");
        write_bytes_atomic(&path, b"payload").expect("write");
        assert_eq!(fs::read(&path).expect("read"), b"payload\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_json_on_invalid_or_missing_input_is_an_error_not_a_panic() {
        let dir = temp_dir("json-errors");
        let missing = dir.join("nope.json");
        assert!(read_json::<serde_json::Value>(&missing).is_err());

        let broken = dir.join("broken.json");
        write_bytes_atomic(&broken, b"{ not json").expect("write");
        assert!(read_json::<serde_json::Value>(&broken).is_err());
        fs::remove_dir_all(&dir).ok();
    }

    /// 裸文件名（无目录部分）也必须能写：`ensure_parent` 容忍空父路径，
    /// 但旧实现的 `sync_parent_dir` 会 `File::open("")` 报 ENOENT，让写成功后返回 Err。
    #[test]
    fn bare_relative_filename_is_writable() {
        let dir = temp_dir("bare");
        let guard = CwdGuard::enter(&dir);

        write_json_atomic(Path::new("bare.json"), &serde_json::json!({ "ok": true }))
            .expect("裸文件名应当可写");
        let value: serde_json::Value = read_json(Path::new("bare.json")).expect("read");
        assert_eq!(value["ok"], serde_json::json!(true));
        assert!(!Path::new("bare.tmp").exists(), "不应残留临时文件");

        drop(guard);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_parent_has_no_directory_to_sync() {
        // 私有 helper 的直接钉桩：空父路径不是错误（这正是上面裸文件名路径依赖的修复）。
        assert_eq!(non_empty_parent(Path::new("bare.json")), None);
        assert_eq!(
            non_empty_parent(Path::new("dir/a.json")),
            Some(Path::new("dir"))
        );
        #[cfg(unix)]
        assert!(sync_parent_dir(Path::new("bare.json")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn private_writes_restrict_file_and_parent_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("private");
        let path = dir.join("secret.json");
        write_json_private_atomic(&path, &serde_json::json!({ "token": "x" })).expect("write");

        let file_mode = fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600, "私有文件应为 0600");
        let dir_mode = fs::metadata(&dir).expect("meta").permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "父目录应被收紧为 0700");

        let value: serde_json::Value = read_json(&path).expect("read");
        assert_eq!(value["token"], serde_json::json!("x"));
        fs::remove_dir_all(&dir).ok();
    }

    /// 进入临时目录并在 drop 时切回；配合互斥避免测试并行时的 CWD 竞争。
    struct CwdGuard {
        original: PathBuf,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    impl CwdGuard {
        fn enter(dir: &Path) -> Self {
            let lock = CWD_LOCK.lock().unwrap_or_else(|error| error.into_inner());
            let original = std::env::current_dir().expect("cwd");
            std::env::set_current_dir(dir).expect("chdir");
            Self {
                original,
                _lock: lock,
            }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.original).ok();
        }
    }
}
