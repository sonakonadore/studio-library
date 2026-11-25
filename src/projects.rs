use crate::scope::{Scope, Source, check_cancel, extension, key, name};
use anyhow::{Result, ensure};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, atomic::AtomicBool};
use walkdir::WalkDir;

pub const FORMATS: &[(&str, &str)] = &[
    ("Ableton Live", "als"),
    ("Logic Pro", "logicx"),
    ("Cubase", "cpr"),
    ("GarageBand", "band"),
    ("Fender Studio Pro", "song"),
    ("FL Studio", "flp"),
    ("REAPER", "rpp"),
    ("Pro Tools", "ptx"),
    ("Bitwig Studio", "bwproject"),
];

pub fn media_help(ext: &str) -> &'static str {
    match ext {
        "als" => "Ableton Live · File → Collect All and Save",
        "logicx" => "Logic Pro · File → Project Management → Consolidate",
        "song" => {
            "Fender Studio Pro · File → Save to New Folder · keep Media and History beside the .song file"
        }
        "cpr" => "Cubase · File → Back up Project",
        "rpp" => {
            "REAPER · Save Project As → choose the options to create a project folder and copy media"
        }
        "flp" => "FL Studio · File → Export → Zipped loop package",
        "ptx" => "Pro Tools · File → Save Copy In → include audio files",
        "band" => "GarageBand · Save the project and keep the entire .band package",
        "bwproject" => "Bitwig Studio · File → Collect and Save",
        _ => "Collect external media in your DAW before copying.",
    }
}

pub fn is_audio(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "wav" | "wave" | "aif" | "aiff" | "mp3"
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleKind {
    Extension,
    FolderName,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub daw: String,
    pub kind: RuleKind,
    pub pattern: String,
}

impl Rule {
    pub fn new(daw: &str, kind: RuleKind, pattern: &str) -> Result<Self> {
        let pattern = match kind {
            RuleKind::Extension => pattern.trim().trim_start_matches('.'),
            RuleKind::FolderName => pattern.trim(),
        };
        ensure!(!daw.trim().is_empty(), "Enter a DAW name.");
        ensure!(
            !pattern.is_empty()
                && !pattern.contains(['/', '\\', ':'])
                && !pattern.chars().any(char::is_control),
            "Invalid detection rule."
        );
        if kind == RuleKind::Extension {
            ensure!(
                !pattern.contains(['.', '*', '?']) && !pattern.chars().any(char::is_whitespace),
                "Enter one file extension, such as .als."
            );
        }
        Ok(Self {
            daw: daw.trim().into(),
            kind,
            pattern: key(pattern),
        })
    }

    fn matches(&self, path: &Path, directory: bool) -> bool {
        match self.kind {
            RuleKind::Extension => key(&extension(path)) == self.pattern,
            RuleKind::FolderName => directory && key(&name(path)) == self.pattern,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectEdit {
    pub title: String,
    pub version: String,
    pub group: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub source: Source,
    pub daw: String,
    pub directory: bool,
    pub manual: bool,
    pub suggested: ProjectEdit,
    #[serde(default)]
    pub modified_day: Option<String>,
    pub review: bool,
    #[serde(skip)]
    pub offline: bool,
    #[serde(skip)]
    pub companions: Vec<Source>,
    #[serde(skip)]
    pub nearby: Vec<Source>,
    context: PathBuf,
}

impl Project {
    pub fn grouping_location(&self) -> &Path {
        &self.context
    }

    pub fn drive_name(&self) -> Option<String> {
        self.source
            .path
            .strip_prefix("/Volumes")
            .ok()?
            .components()
            .next()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
    }
}

#[derive(Default)]
pub struct Scan {
    pub projects: Vec<Project>,
    pub warnings: Vec<String>,
    pub skipped: usize,
    pub(crate) incomplete: BTreeSet<PathBuf>,
}

pub fn scan(
    roots: &[Scope],
    manual: &[Scope],
    rules: &[Rule],
    excluded: &[PathBuf],
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize),
) -> Result<Scan> {
    let mut scan = Scan::default();
    let mut found = BTreeMap::<PathBuf, Project>::new();
    for scope in roots {
        check_cancel(cancel)?;
        if let Err(error) = scope.check(&scope.path) {
            scan.warnings.push(format!("{error:#}"));
            scan.incomplete.insert(scope.path.clone());
            continue;
        }
        let mut walk = WalkDir::new(&scope.path)
            .follow_links(false)
            .same_file_system(true)
            .into_iter();
        while let Some(result) = walk.next() {
            check_cancel(cancel)?;
            let entry = match result {
                Ok(entry) => entry,
                Err(error) => {
                    scan.warnings.push(error.to_string());
                    scan.incomplete.insert(scope.path.clone());
                    continue;
                }
            };
            let path = entry.path();
            let directory = entry.file_type().is_dir();
            if entry.file_type().is_symlink() {
                scan.skipped += 1;
                continue;
            }
            if (entry.depth() > 0
                && (excluded.iter().any(|output| path == output)
                    || entry.file_name().to_string_lossy().starts_with('.')))
                || in_history(path)
            {
                if directory {
                    walk.skip_current_dir();
                }
                continue;
            }
            if (!directory && !entry.file_type().is_file())
                || detect(path, directory, rules).is_none()
            {
                continue;
            }
            if directory {
                walk.skip_current_dir();
            }
            let item = project(
                scope.source(path.to_owned()),
                directory,
                false,
                rules,
                cancel,
                &mut scan,
            );
            match item {
                Ok(item) => {
                    found.entry(path.to_owned()).or_insert(item);
                }
                Err(error) => {
                    scan.warnings.push(format!("{error:#}"));
                    scan.incomplete.insert(scope.path.clone());
                }
            }
            progress(found.len());
        }
        if let Err(error) = scope.check(&scope.path) {
            scan.warnings.push(format!("{error:#}"));
            scan.incomplete.insert(scope.path.clone());
            found.retain(|_, project| project.source.scope != *scope);
        }
    }

    for scope in manual {
        check_cancel(cancel)?;
        if in_history(&scope.path) {
            scan.warnings.push(format!(
                "Skipped an item in History · {}",
                scope.path.display()
            ));
            continue;
        }
        let owner = manual_owner(scope, roots);
        let source = owner.source(scope.path.clone());
        let item = project(source, scope.directory, true, rules, cancel, &mut scan);
        match item {
            Ok(item) => {
                found.insert(scope.path.clone(), item);
            }
            Err(error) => {
                scan.warnings.push(format!("{error:#}"));
                scan.incomplete.insert(scope.path.clone());
            }
        }
        progress(found.len());
    }
    check_cancel(cancel)?;
    scan.projects = found.into_values().collect();
    order(&mut scan.projects);
    Ok(scan)
}

pub(crate) fn manual_owner<'a>(scope: &'a Scope, roots: &'a [Scope]) -> &'a Scope {
    roots
        .iter()
        .filter(|root| root.accepts(&scope.path))
        .max_by_key(|root| root.path.components().count())
        .unwrap_or(scope)
}

pub(crate) fn order(projects: &mut [Project]) {
    mark_review(projects);
    projects.sort_by_key(|item| {
        (
            key(&item.suggested.group),
            item.suggested.version.clone(),
            item.source.path.clone(),
        )
    });
}

fn in_history(path: &Path) -> bool {
    path.components()
        .any(|part| part.as_os_str().eq_ignore_ascii_case("History"))
}

fn detect(path: &Path, directory: bool, rules: &[Rule]) -> Option<String> {
    if let Some(rule) = rules
        .iter()
        .rev()
        .find(|rule| rule.matches(path, directory))
    {
        return Some(rule.daw.clone());
    }
    let ext = extension(path);
    if directory && !matches!(ext.as_str(), "logicx" | "band") {
        return None;
    }
    FORMATS
        .iter()
        .find(|(_, suffix)| *suffix == ext)
        .map(|(daw, _)| (*daw).to_owned())
}

static VERSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[\s_.(\[-])(?:v(?:er(?:sion)?)?[ ._-]*|버전[ ._-]*)([0-9]{1,6})[\s)\]]*$")
        .unwrap()
});

pub fn title_version(value: &str) -> (String, Option<String>) {
    let value = value.trim();
    let Some(captures) = VERSION.captures(value) else {
        return (value.to_owned(), None);
    };
    let version = captures[1].parse::<u32>().unwrap_or_default();
    let title = value[..captures.get(0).unwrap().start()].trim_end_matches([' ', '_', '-', '.']);
    (title.to_owned(), Some(format!("v{version:02}")))
}

fn suggestion(path: &Path, strip_ext: bool) -> (ProjectEdit, PathBuf) {
    let parent = path.parent().unwrap_or(path);
    let raw_name = if !strip_ext {
        name(path)
    } else {
        path.file_stem()
            .map(|part| part.to_string_lossy().into_owned())
            .unwrap_or_else(|| name(path))
    };
    let (mut title, file_version) = title_version(&raw_name);
    let (folder_title, folder_version) = title_version(&name(parent));
    if title.is_empty() && !folder_title.is_empty() {
        title = folder_title.clone();
    }
    if title.is_empty() {
        title = raw_name;
    }
    let folder_matches = folder_title.is_empty() || key(&folder_title) == key(&title);
    let context = if folder_version.is_some() && folder_matches {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };
    let version = file_version.or_else(|| folder_version.filter(|_| folder_matches));
    (
        ProjectEdit {
            group: title.clone(),
            title,
            version: version.unwrap_or_else(|| "버전 미지정".into()),
        },
        context.to_owned(),
    )
}

fn mark_review(projects: &mut [Project]) {
    let mut titles = HashMap::<String, (BTreeSet<PathBuf>, HashMap<String, usize>)>::new();
    for project in projects.iter() {
        let (contexts, versions) = titles.entry(key(&project.suggested.title)).or_default();
        contexts.insert(project.context.clone());
        *versions.entry(key(&project.suggested.version)).or_default() += 1;
    }
    for project in projects {
        let (contexts, versions) = &titles[&key(&project.suggested.title)];
        project.review = contexts.len() > 1 || versions[&key(&project.suggested.version)] > 1;
    }
}

fn project(
    source: Source,
    directory: bool,
    manual: bool,
    rules: &[Rule],
    cancel: &AtomicBool,
    scan: &mut Scan,
) -> Result<Project> {
    let metadata = source.metadata()?;
    let path = &source.path;
    let strip_ext = !directory
        || matches!(extension(path).as_str(), "logicx" | "band")
        || rules
            .iter()
            .rev()
            .find(|rule| rule.matches(path, directory))
            .is_some_and(|rule| rule.kind == RuleKind::Extension);
    let (suggested, context) = suggestion(path, strip_ext);
    let (copy_base, companions) = match copy_parts(&source, directory) {
        Ok(parts) => parts,
        Err(error) => {
            scan.warnings.push(format!("{error:#}"));
            (path.parent().unwrap_or(path).to_owned(), Vec::new())
        }
    };
    let mut nearby = Vec::new();
    let near = if directory {
        path.as_path()
    } else {
        &copy_base
    };
    if source.scope.accepts(near) {
        for entry in WalkDir::new(near)
            .follow_links(false)
            .same_file_system(true)
            .max_depth(4)
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0
                    || (!in_history(entry.path())
                        && !entry.file_name().to_string_lossy().starts_with('.'))
            })
        {
            check_cancel(cancel)?;
            match entry {
                Ok(entry) if entry.file_type().is_file() && is_audio(entry.path()) => {
                    nearby.push(source.scope.source(entry.into_path()));
                    if nearby.len() >= 200 {
                        scan.warnings.push(format!(
                            "Showing the first 200 nearby audio files · {}",
                            path.display()
                        ));
                        break;
                    }
                }
                Err(error) => scan.warnings.push(error.to_string()),
                _ => {}
            }
        }
        nearby.sort_by(|a, b| a.path.cmp(&b.path));
    }
    Ok(Project {
        daw: detect(path, directory, rules).unwrap_or_else(|| "Manual project".into()),
        source,
        directory,
        manual,
        suggested,
        modified_day: local_day(metadata.mtime()),
        review: false,
        offline: false,
        companions,
        nearby,
        context,
    })
}

fn local_day(timestamp: libc::time_t) -> Option<String> {
    let mut day = std::mem::MaybeUninit::<libc::tm>::uninit();
    // localtime_r writes a fully initialized tm on success.
    if unsafe { libc::localtime_r(&timestamp, day.as_mut_ptr()) }.is_null() {
        return None;
    }
    let day = unsafe { day.assume_init() };
    Some(format!(
        "{:04}-{:02}-{:02}",
        day.tm_year + 1900,
        day.tm_mon + 1,
        day.tm_mday
    ))
}

pub(crate) fn copy_parts(source: &Source, directory: bool) -> Result<(PathBuf, Vec<Source>)> {
    let path = &source.path;
    let ext = extension(path);
    let mut copy_base = path.parent().unwrap_or(path).to_owned();
    if let Some(parent) = path.parent().filter(|p| source.scope.accepts(p)) {
        if ext == "als" {
            for ancestor in parent.ancestors().take_while(|p| source.scope.accepts(p)) {
                if named_children(&source.scope, ancestor, &["Ableton Project Info"])?.len() == 1 {
                    copy_base = ancestor.to_owned();
                    break;
                }
            }
        }
        let names: &[&str] = match ext.as_str() {
            "song" => &["Media", "History", "Bounces", "Stems", "Cache", "Mixdown"],
            "als" => &["Ableton Project Info", "Samples"],
            "logicx" if !directory => &[
                "Audio Files",
                "Media",
                "Bounces",
                "Sampler Instruments",
                "Samples",
                "Impulse Responses",
            ],
            "cpr" => &["Audio", "Edits", "Images", "Freeze"],
            "rpp" => &["Media", "Audio"],
            "ptx" => &["Audio Files", "Fade Files", "Video Files", "Clip Groups"],
            _ => &[],
        };
        let companions = if names.is_empty() {
            Vec::new()
        } else {
            named_children(&source.scope, &copy_base, names)?
        };
        return Ok((copy_base, companions));
    }
    Ok((copy_base, Vec::new()))
}

fn named_children(scope: &Scope, directory: &Path, names: &[&str]) -> Result<Vec<Source>> {
    scope.check(directory)?;
    let mut result = Vec::new();
    for entry in std::fs::read_dir(directory)
        .map_err(|error| anyhow::anyhow!("{} · {error}", directory.display()))?
    {
        let entry = entry?;
        if names
            .iter()
            .any(|name| entry.file_name().eq_ignore_ascii_case(name))
            && entry.file_type()?.is_dir()
            && scope.same_volume(&entry.metadata()?)
        {
            result.push(scope.source(entry.path()));
        }
    }
    Ok(result)
}
