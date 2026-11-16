// SPDX-License-Identifier: MIT OR Apache-2.0
//! Atomic sibling replacement on one bounded I/O worker, independent of audio control.
use crossbeam::channel::{Receiver, Sender, bounded};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fingerprint {
    Missing,
    Content(Vec<u8>),
}
impl Fingerprint {
    pub fn read(path: &Path) -> Result<Self, String> {
        match fs::read(path) {
            Ok(bytes) => Ok(Self::Content(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::Missing),
            Err(e) => Err(format!("Read {}: {e}", path.display())),
        }
    }
}
#[derive(Clone, Debug)]
pub enum SaveError {
    Conflict {
        path: PathBuf,
        observed: Fingerprint,
    },
    Failed(String),
}
impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict { path, .. } => write!(
                f,
                "{} changed externally; confirm overwrite or use Save As",
                path.display()
            ),
            Self::Failed(e) => f.write_str(e),
        }
    }
}
impl From<String> for SaveError {
    fn from(s: String) -> Self {
        Self::Failed(s)
    }
}
#[derive(Clone, Debug)]
pub struct SaveReceipt {
    pub path: PathBuf,
    pub fingerprint: Fingerprint,
    pub warnings: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Create,
    Write,
    Flush,
    Rename,
    DirectorySync,
}
pub fn destination_path(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("Destination directory {}: {e}", parent.display()))?;
    Ok(parent.join(path.file_name().ok_or("Missing destination filename")?))
}
pub fn atomic_write(
    path: &Path,
    bytes: &[u8],
    expected: Fingerprint,
    cancel: &AtomicBool,
) -> Result<SaveReceipt, SaveError> {
    atomic_write_with(path, bytes, expected, cancel, |_| Ok(()))
}
fn atomic_write_with(
    path: &Path,
    bytes: &[u8],
    expected: Fingerprint,
    cancel: &AtomicBool,
    mut hook: impl FnMut(Stage) -> Result<(), String>,
) -> Result<SaveReceipt, SaveError> {
    let path = destination_path(path)?;
    let parent = path.parent().unwrap();
    let check = || -> Result<(), SaveError> {
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(SaveError::Failed(
                "Refusing to replace a symlink destination; choose its target explicitly".into(),
            ));
        }
        let observed = Fingerprint::read(&path)?;
        if observed != expected {
            return Err(SaveError::Conflict {
                path: path.clone(),
                observed,
            });
        }
        Ok(())
    };
    check()?;
    let canceled = || {
        if cancel.load(Ordering::Acquire) {
            Err("Save canceled before replacement".to_owned())
        } else {
            Ok(())
        }
    };
    canceled()?;
    hook(Stage::Create)?;
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let mut temp = None;
    for _ in 0..64 {
        let candidate = parent.join(format!(
            ".wisp-save-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temp = Some((candidate, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Create temporary file: {e}").into()),
        }
    }
    let (temp, mut file) = temp.ok_or_else(|| {
        SaveError::Failed("Cannot allocate a unique sibling temporary file".into())
    })?;
    let result = (|| {
        canceled()?;
        hook(Stage::Write)?;
        file.write_all(bytes)
            .map_err(|e| format!("Write temporary file: {e}"))?;
        hook(Stage::Flush)?;
        file.flush()
            .map_err(|e| format!("Flush temporary file: {e}"))?;
        if let Ok(metadata) = fs::metadata(&path) {
            file.set_permissions(metadata.permissions())
                .map_err(|e| format!("Preserve permissions: {e}"))?;
        }
        file.sync_all()
            .map_err(|e| format!("Sync temporary file: {e}"))?;
        drop(file);
        canceled()?;
        hook(Stage::Rename)?;
        check()?;
        // std::fs::rename replaces files on supported platforms; never delete the
        // destination to work around an unsupported/failed atomic replacement.
        fs::rename(&temp, &path).map_err(|e| format!("Atomic replace {}: {e}", path.display()))?;
        let mut warnings = vec![];
        let directory_sync = hook(Stage::DirectorySync).and_then(|_| {
            #[cfg(unix)]
            {
                fs::File::open(parent)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| e.to_string())
            }
            #[cfg(not(unix))]
            {
                Ok(())
            }
        });
        if let Err(e) = directory_sync {
            warnings.push(format!(
                "File replaced successfully; directory durability could not be confirmed: {e}"
            ));
        }
        Ok(SaveReceipt {
            path: path.clone(),
            fingerprint: Fingerprint::Content(bytes.to_vec()),
            warnings,
        })
    })();
    let _ = fs::remove_file(&temp);
    result
}
pub struct SaveJob {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub expected: Fingerprint,
    pub references: Vec<super::assets::BufferReference>,
}
pub struct IoWorker {
    pub tx: Sender<SaveJob>,
    pub rx: Receiver<Result<SaveReceipt, SaveError>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl IoWorker {
    pub fn spawn() -> Self {
        let (tx, jobs) = bounded::<SaveJob>(1);
        let (results, rx) = bounded(1);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                let job = match jobs.recv_timeout(Duration::from_millis(20)) {
                    Ok(j) => j,
                    Err(crossbeam::channel::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let result = (|| {
                    let path = destination_path(&job.path)?;
                    let text = std::str::from_utf8(&job.bytes).map_err(|e| e.to_string())?;
                    let (text, warnings) =
                        super::assets::rebase(text, &job.references, path.parent().unwrap())?;
                    let mut receipt =
                        atomic_write(&path, text.as_bytes(), job.expected, &worker_stop)?;
                    receipt.warnings.extend(warnings);
                    Ok::<_, SaveError>(receipt)
                })();
                if results
                    .send_timeout(result, Duration::from_millis(20))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            tx,
            rx,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for IoWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub struct Temp(pub PathBuf);
    impl Temp {
        pub fn new() -> Self {
            static N: AtomicU64 = AtomicU64::new(1);
            let p = std::env::temp_dir().join(format!(
                "wisp-m3-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn failure_stages_keep_old_bytes_and_clean_only_our_temp() {
        let dir = Temp::new();
        let path = dir.0.join("test.twf");
        fs::write(&path, b"old").unwrap();
        fs::write(dir.0.join("unrelated.tmp"), b"keep").unwrap();
        for failure in [Stage::Create, Stage::Write, Stage::Flush, Stage::Rename] {
            let result = atomic_write_with(
                &path,
                b"new",
                Fingerprint::Content(b"old".to_vec()),
                &AtomicBool::new(false),
                |s| {
                    if s == failure {
                        Err(format!("injected {s:?}"))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(result.is_err());
            assert_eq!(fs::read(&path).unwrap(), b"old");
            assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
        }
        let receipt = atomic_write_with(
            &path,
            b"new",
            Fingerprint::Content(b"old".to_vec()),
            &AtomicBool::new(false),
            |s| {
                if s == Stage::DirectorySync {
                    Err("injected durability failure".into())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(receipt.warnings.len(), 1);
    }
    #[test]
    fn conflict_cancellation_and_recheck_prevent_replacement() {
        let dir = Temp::new();
        let path = dir.0.join("test.twf");
        fs::write(&path, b"external").unwrap();
        assert!(matches!(
            atomic_write(
                &path,
                b"new",
                Fingerprint::Content(b"old".to_vec()),
                &AtomicBool::new(false)
            ),
            Err(SaveError::Conflict { .. })
        ));
        assert!(
            atomic_write(
                &path,
                b"new",
                Fingerprint::Content(b"external".to_vec()),
                &AtomicBool::new(true)
            )
            .is_err()
        );
        let result = atomic_write_with(
            &path,
            b"new",
            Fingerprint::Content(b"external".to_vec()),
            &AtomicBool::new(false),
            |s| {
                if s == Stage::Rename {
                    fs::write(&path, b"changed again").unwrap();
                }
                Ok(())
            },
        );
        assert!(matches!(result, Err(SaveError::Conflict { .. })));
        assert_eq!(fs::read(&path).unwrap(), b"changed again");
    }
    #[cfg(unix)]
    #[test]
    fn permissions_and_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = Temp::new();
        let path = dir.0.join("test.twf");
        fs::write(&path, b"old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        atomic_write(
            &path,
            b"new",
            Fingerprint::Content(b"old".to_vec()),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let link = dir.0.join("link.twf");
        symlink(&path, &link).unwrap();
        assert!(
            atomic_write(
                &link,
                b"bad",
                Fingerprint::Content(b"new".to_vec()),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert_eq!(fs::read(path).unwrap(), b"new");
    }
}
