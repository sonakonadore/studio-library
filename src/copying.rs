use crate::projects::{Project, ProjectEdit, copy_parts};
use crate::scope::{Cancelled, Scope, Source, Stamp, check_cancel, extension, key, safe_component};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use walkdir::WalkDir;

#[derive(Clone, Debug)]
struct Entry {
    source: Source,
    relative: PathBuf,
    stamp: Stamp,
    directory: bool,
}

#[derive(Debug)]
pub struct Plan {
    destination: Scope,
    project_path: Option<PathBuf>,
    entries: Vec<Entry>,
    pub skipped: usize,
}

pub struct Preview<'a> {
    pub source: &'a Path,
    pub target: PathBuf,
    pub bytes: u64,
    pub directory: bool,
}

#[derive(Default)]
pub struct CopyReport {
    pub published: Vec<PathBuf>,
    pub outcome: Option<anyhow::Error>,
}

pub fn project_plan(
    project: &Project,
    edit: &ProjectEdit,
    related: &[Source],
    destination: Scope,
    cancel: &AtomicBool,
) -> Result<Plan> {
    ensure!(
        destination.directory && !edit.title.trim().is_empty(),
        "Choose a song name and destination."
    );
    let project_path = PathBuf::from(safe_component(&edit.group)?)
        .join(safe_component(&edit.version)?)
        .join("프로젝트");
    let mut plan = Plan {
        destination,
        project_path: Some(project_path),
        entries: Vec::new(),
        skipped: 0,
    };
    plan.destination.check(&plan.destination.path)?;
    ensure!(
        extension(&project.source.path) != "song" || project.source.scope.directory,
        "Add the folder containing the .song file to include Media and History."
    );
    let (copy_base, companions) = copy_parts(&project.source, project.directory)?;
    let path = project.source.path.strip_prefix(&copy_base)?.to_owned();
    plan.add(&project.source, path, cancel)?;
    for companion in &companions {
        let path = companion.path.strip_prefix(&copy_base)?.to_owned();
        plan.add(companion, path, cancel)?;
    }
    for source in related {
        check_cancel(cancel)?;
        if plan
            .entries
            .iter()
            .any(|entry| entry.source.path == source.path)
        {
            continue;
        }
        let relative = if project.source.scope == source.scope
            && source.path.starts_with(&copy_base)
            && !plan
                .entries
                .iter()
                .any(|entry| !entry.directory && source.path.starts_with(&entry.source.path))
        {
            source.path.strip_prefix(&copy_base)?.to_owned()
        } else {
            PathBuf::from("관련 파일").join(
                source
                    .path
                    .file_name()
                    .context("Related file is missing a name.")?,
            )
        };
        plan.add(source, relative, cancel)?;
    }
    plan.validate_names()?;
    plan.check_target()?;
    Ok(plan)
}

pub fn sample_plan(sources: &[Source], destination: Scope, cancel: &AtomicBool) -> Result<Plan> {
    ensure!(
        destination.directory && !sources.is_empty(),
        "Select samples and a destination."
    );
    let mut plan = Plan {
        destination,
        project_path: None,
        entries: Vec::new(),
        skipped: 0,
    };
    plan.destination.check(&plan.destination.path)?;
    for source in sources {
        ensure!(
            source.metadata()?.is_file(),
            "Select a regular sample file."
        );
        let relative = PathBuf::from(
            source
                .path
                .file_name()
                .context("Sample file is missing a name.")?,
        );
        plan.add(source, relative, cancel)?;
    }
    plan.validate_names()?;
    plan.check_target()?;
    Ok(plan)
}

impl Plan {
    fn add(&mut self, source: &Source, relative: PathBuf, cancel: &AtomicBool) -> Result<()> {
        check_cancel(cancel)?;
        let meta = source.metadata()?;
        ensure!(
            !relative.as_os_str().is_empty(),
            "Enter a name for the copy."
        );
        ensure!(
            !meta.is_dir() || !self.destination.path.starts_with(&source.path),
            "Choose a destination outside the source folder · {}",
            source.path.display()
        );
        if meta.is_file() {
            self.entries.push(Entry {
                source: source.clone(),
                relative,
                stamp: Stamp::from(&meta),
                directory: false,
            });
            return Ok(());
        }
        ensure!(
            meta.is_dir(),
            "Unsupported file · {}",
            source.path.display()
        );
        let mut walker = WalkDir::new(&source.path)
            .follow_links(false)
            .same_file_system(true)
            .into_iter();
        while let Some(item) = walker.next() {
            check_cancel(cancel)?;
            let item = item?;
            let kind = item.file_type();
            let item_meta = item.metadata()?;
            if (!kind.is_file() && !kind.is_dir()) || !source.scope.same_volume(&item_meta) {
                if kind.is_dir() {
                    walker.skip_current_dir();
                }
                self.skipped += 1;
                continue;
            }
            let tail = item.path().strip_prefix(&source.path)?;
            let path = if tail.as_os_str().is_empty() {
                relative.clone()
            } else {
                relative.join(tail)
            };
            self.entries.push(Entry {
                source: source.scope.source(item.into_path()),
                relative: path,
                stamp: Stamp::from(&item_meta),
                directory: kind.is_dir(),
            });
        }
        Ok(())
    }

    fn validate_names(&self) -> Result<()> {
        let mut paths = BTreeSet::new();
        let mut files = BTreeSet::new();
        for entry in &self.entries {
            let normalized = key(&entry.relative.to_string_lossy());
            ensure!(
                paths.insert(normalized.clone()),
                "Duplicate copy path · {}",
                entry.relative.display()
            );
            if !entry.directory {
                files.insert(normalized);
            }
        }
        for entry in &self.entries {
            for parent in entry.relative.ancestors().skip(1) {
                ensure!(
                    !files.contains(&key(&parent.to_string_lossy())),
                    "A file and folder share the same path · {}",
                    parent.display()
                );
            }
        }
        Ok(())
    }

    pub fn target(&self) -> PathBuf {
        match &self.project_path {
            Some(path) => self.destination.path.join(path),
            None => self.destination.path.clone(),
        }
    }

    pub fn bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter(|entry| !entry.directory)
            .fold(0u64, |total, entry| total.saturating_add(entry.stamp.len))
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn previews(&self, range: std::ops::Range<usize>) -> impl Iterator<Item = Preview<'_>> {
        let target = self.target();
        self.entries[range].iter().map(move |entry| Preview {
            source: &entry.source.path,
            target: target.join(&entry.relative),
            bytes: if entry.directory { 0 } else { entry.stamp.len },
            directory: entry.directory,
        })
    }

    fn check_target(&self) -> Result<()> {
        self.destination.check(&self.destination.path)?;
        if let Some(path) = &self.project_path {
            for part in path
                .ancestors()
                .skip(1)
                .filter(|p| !p.as_os_str().is_empty())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                let current = self.destination.path.join(part);
                if missing(&current)? {
                    break;
                }
                ensure!(
                    self.destination.check(&current)?.is_dir(),
                    "Check the destination’s parent folder · {}",
                    current.display()
                );
            }
            let target = self.target();
            ensure!(
                missing(&target)?,
                "Destination already exists · {}",
                target.display()
            );
        } else {
            for entry in &self.entries {
                let target = self.destination.path.join(&entry.relative);
                ensure!(
                    missing(&target)?,
                    "Destination already exists · {}",
                    target.display()
                );
            }
        }
        Ok(())
    }

    pub fn execute(&self, cancel: &AtomicBool, mut progress: impl FnMut(u64, u64)) -> CopyReport {
        let mut report = CopyReport::default();
        let result = self.copy(cancel, &mut progress, &mut report.published);
        report.outcome = result.err();
        report
    }

    fn copy(
        &self,
        cancel: &AtomicBool,
        progress: &mut impl FnMut(u64, u64),
        published: &mut Vec<PathBuf>,
    ) -> Result<()> {
        check_cancel(cancel)?;
        self.check_target()?;
        let stage = tempfile::Builder::new()
            .prefix(".studio-library-")
            .tempdir_in(&self.destination.path)?;
        let payload = stage.path().join("payload");
        fs::create_dir(&payload)?;
        let mut bytes = 0u64;
        let total = self.bytes();
        for entry in &self.entries {
            check_cancel(cancel)?;
            self.destination.check(&payload)?;
            verify(entry)?;
            let target = payload.join(&entry.relative);
            if entry.directory {
                fs::create_dir_all(target)?;
                continue;
            }
            fs::create_dir_all(
                target
                    .parent()
                    .context("Destination has no parent folder.")?,
            )?;
            let mut input = entry.source.open()?;
            ensure!(
                Stamp::from(&input.metadata()?) == entry.stamp,
                "Source changed. Prepare the copy again."
            );
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            copy_bytes(&mut input, &mut output, cancel, |amount| {
                bytes = bytes.saturating_add(amount);
                progress(bytes, total);
            })?;
            verify(entry)?;
            preserve_metadata(&input, &output)?;
            output.sync_all()?;
        }
        for entry in &self.entries {
            check_cancel(cancel)?;
            verify(entry)?;
        }
        check_cancel(cancel)?;
        self.check_target()?;
        if let Some(path) = &self.project_path {
            let parent = path.parent().context("Destination has no parent folder.")?;
            let mut directory = self.destination.path.clone();
            for part in parent {
                directory.push(part);
                match fs::create_dir(&directory) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
                ensure!(
                    self.destination.check(&directory)?.is_dir(),
                    "Destination folder changed."
                );
            }
            check_cancel(cancel)?;
            let target = self.target();
            rename_exclusive(&payload, &target)?;
            published.push(target);
        } else {
            for entry in &self.entries {
                check_cancel(cancel)?;
                self.destination.check(&self.destination.path)?;
                let target = self.destination.path.join(&entry.relative);
                rename_exclusive(&payload.join(&entry.relative), &target)?;
                published.push(target);
            }
        }
        Ok(())
    }
}

fn verify(entry: &Entry) -> Result<()> {
    let meta = entry.source.metadata()?;
    let stamp = Stamp::from(&meta);
    ensure!(
        stamp == entry.stamp && meta.is_dir() == entry.directory,
        "Source changed. Prepare the copy again. · {}",
        entry.source.path.display()
    );
    Ok(())
}

fn copy_bytes(
    input: &mut File,
    output: &mut File,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<()> {
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        check_cancel(cancel)?;
        let read = input.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        output.write_all(&buffer[..read])?;
        progress(read as u64);
    }
}

fn missing(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error).with_context(|| path.display().to_string()),
    }
}

fn preserve_metadata(input: &File, output: &File) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        let code = unsafe {
            libc::fcopyfile(
                input.as_raw_fd(),
                output.as_raw_fd(),
                std::ptr::null_mut(),
                libc::COPYFILE_METADATA,
            )
        };
        if code != 0 {
            return Err(io::Error::last_os_error()).context("Could not copy file metadata.");
        }
    }
    #[cfg(not(target_os = "macos"))]
    output.set_permissions(input.metadata()?.permissions())?;
    Ok(())
}

fn rename_exclusive(from: &Path, to: &Path) -> Result<()> {
    let old = CString::new(from.as_os_str().as_bytes())?;
    let new = CString::new(to.as_os_str().as_bytes())?;
    #[cfg(target_os = "macos")]
    let code = unsafe { libc::renamex_np(old.as_ptr(), new.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(target_os = "linux")]
    let code = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            old.as_ptr(),
            libc::AT_FDCWD,
            new.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if code != 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::AlreadyExists {
            anyhow::bail!("Destination already exists · {}", to.display());
        }
        return Err(error).with_context(|| to.display().to_string());
    }
    Ok(())
}

impl CopyReport {
    pub fn message(&self) -> String {
        let published = if self.published.is_empty() {
            String::new()
        } else {
            format!(" · {} copied", self.published.len())
        };
        match &self.outcome {
            None => format!("Copy complete · {} copied", self.published.len()),
            Some(error) if error.is::<Cancelled>() => format!("Copy canceled{published}"),
            Some(error) => format!("Copy failed{published} · {error:#}"),
        }
    }
}
