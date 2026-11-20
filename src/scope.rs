use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, Metadata, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Scope {
    pub path: PathBuf,
    device: u64,
    inode: u64,
    pub directory: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Source {
    pub scope: Scope,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Stamp {
    pub len: u64,
    device: u64,
    inode: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl From<&Metadata> for Stamp {
    fn from(meta: &Metadata) -> Self {
        Self {
            len: meta.len(),
            device: meta.dev(),
            inode: meta.ino(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        }
    }
}

impl Scope {
    pub fn select(path: &Path) -> Result<Self> {
        let selected = fs::symlink_metadata(path).with_context(|| path.display().to_string())?;
        ensure!(
            selected.is_file() || selected.is_dir(),
            "일반 파일/폴더 선택 필요"
        );
        let path = path.canonicalize()?;
        ensure!(path.to_str().is_some(), "UTF-8 경로 선택 필요");
        Ok(Self {
            path,
            device: selected.dev(),
            inode: selected.ino(),
            directory: selected.is_dir(),
        })
    }

    pub fn check(&self, path: &Path) -> Result<Metadata> {
        let relative = path
            .strip_prefix(&self.path)
            .context("선택 범위 밖의 경로")?;
        ensure!(
            self.directory || relative.as_os_str().is_empty(),
            "선택 범위 밖의 경로"
        );
        let mut current = self.path.clone();
        let mut meta = fs::symlink_metadata(&current)
            .with_context(|| format!("선택 경로 접근 불가 · {}", current.display()))?;
        ensure!(
            meta.dev() == self.device && meta.ino() == self.inode && !meta.is_symlink(),
            "선택 경로 변경 · 다시 선택 · {}",
            current.display()
        );
        ensure!(
            current.canonicalize()? == self.path,
            "선택 경로의 상위 폴더 변경 · {}",
            current.display()
        );
        for part in relative.components() {
            let Component::Normal(part) = part else {
                anyhow::bail!("선택 범위 밖의 경로");
            };
            current.push(part);
            meta = fs::symlink_metadata(&current).with_context(|| current.display().to_string())?;
            ensure!(
                !meta.is_symlink() && meta.dev() == self.device,
                "링크 또는 다른 볼륨 건너뜀 · {}",
                current.display()
            );
        }
        Ok(meta)
    }

    pub fn source(&self, path: PathBuf) -> Source {
        Source {
            scope: self.clone(),
            path,
        }
    }

    pub fn accepts(&self, path: &Path) -> bool {
        path == self.path || (self.directory && path.starts_with(&self.path))
    }

    pub fn same_volume(&self, metadata: &Metadata) -> bool {
        metadata.dev() == self.device
    }
}

impl Source {
    pub fn metadata(&self) -> Result<Metadata> {
        self.scope.check(&self.path)
    }

    pub fn open(&self) -> Result<File> {
        let before = self.metadata()?;
        ensure!(before.is_file(), "일반 파일 선택 필요");
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)?;
        let after = file.metadata()?;
        ensure!(
            after.is_file()
                && Stamp::from(&before) == Stamp::from(&after)
                && Stamp::from(&self.metadata()?) == Stamp::from(&after),
            "파일 변경 · {}",
            self.path.display()
        );
        Ok(file)
    }
}

#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("취소됨")
    }
}

impl std::error::Error for Cancelled {}

pub fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled.into());
    }
    Ok(())
}

pub fn name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

pub fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub fn key(value: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    value.nfkc().flat_map(char::to_lowercase).collect()
}

pub fn safe_component(value: &str) -> Result<&str> {
    let value = value.trim();
    ensure!(
        !value.is_empty()
            && !matches!(value, "." | "..")
            && value.len() <= 240
            && !value.contains(['/', '\\', ':'])
            && !value.chars().any(char::is_control),
        "사용할 수 없는 폴더 이름 · {value}"
    );
    Ok(value)
}
