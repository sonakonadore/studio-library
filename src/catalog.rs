use crate::audio::{self, Analysis};
use crate::projects::{self, Project, ProjectEdit, Rule, is_audio};
use crate::scope::{Cancelled, Scope, Source, Stamp, check_cancel, key};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use walkdir::WalkDir;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VirtualFolder {
    pub name: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum SampleGrouping {
    #[default]
    Instrument,
    Genre,
    Length,
    Brightness,
}

impl SampleGrouping {
    pub const ALL: [Self; 4] = [
        Self::Instrument,
        Self::Genre,
        Self::Length,
        Self::Brightness,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Instrument => "Instrument",
            Self::Genre => "Genre",
            Self::Length => "Length",
            Self::Brightness => "Brightness",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum ProjectGrouping {
    #[default]
    #[serde(alias = "None")]
    Title,
    Daw,
    ModifiedDate,
}

impl ProjectGrouping {
    pub const ALL: [Self; 3] = [Self::Title, Self::Daw, Self::ModifiedDate];

    pub fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Daw => "DAW",
            Self::ModifiedDate => "Modified date",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub roots: Vec<Scope>,
    pub project_grouping: ProjectGrouping,
    pub samples: Option<Scope>,
    pub additional_sample_folders: Vec<Scope>,
    pub sample_grouping: SampleGrouping,
    pub dark_mode: bool,
    pub manual: Vec<Scope>,
    pub rules: Vec<Rule>,
    pub edits: BTreeMap<PathBuf, ProjectEdit>,
    pub related: BTreeMap<PathBuf, Vec<Source>>,
    pub primary: Option<String>,
    pub virtual_folders: Vec<VirtualFolder>,
    pub outputs: Vec<PathBuf>,
}

impl Settings {
    pub fn sample_folders(&self) -> impl Iterator<Item = &Scope> {
        self.samples.iter().chain(&self.additional_sample_folders)
    }
}

#[derive(Clone, Debug)]
pub struct Sample {
    pub source: Source,
    pub analysis: Analysis,
    pub tags: Vec<String>,
    pub customized: bool,
    stamp: Stamp,
}

pub struct SampleScan {
    pub rows: Vec<Sample>,
    pub decoded: usize,
    pub skipped: usize,
    pub warnings: Vec<String>,
}

pub struct Store {
    connection: Connection,
}

pub fn database_path() -> Result<PathBuf> {
    let folder = match std::env::var_os("STUDIO_LIBRARY_DATA_DIR") {
        Some(folder) => PathBuf::from(folder),
        None => PathBuf::from(std::env::var_os("HOME").context("Home folder not found.")?)
            .join("Library/Application Support/Studio Library"),
    };
    std::fs::create_dir_all(&folder)?;
    Ok(folder.join("library.sqlite3"))
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(4))?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS samples (
                root TEXT NOT NULL, path TEXT NOT NULL, stamp TEXT NOT NULL, analysis TEXT NOT NULL,
                PRIMARY KEY(root, path)
             );
             CREATE TABLE IF NOT EXISTS sample_tags (path TEXT PRIMARY KEY, tags TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS project_snapshot (id INTEGER PRIMARY KEY, data TEXT NOT NULL);",
        )?;
        Ok(Self { connection })
    }

    pub fn settings(&self) -> Result<Settings> {
        let raw: Option<String> = self
            .connection
            .query_row("SELECT data FROM settings WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?;
        raw.map(|data| serde_json::from_str(&data).map_err(Into::into))
            .unwrap_or_else(|| Ok(Settings::default()))
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.connection.execute(
            "INSERT INTO settings (id, data) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            [serde_json::to_string(settings)?],
        )?;
        Ok(())
    }

    pub fn cached_projects(&self, settings: &Settings) -> Result<Vec<Project>> {
        let data: Option<String> = self
            .connection
            .query_row(
                "SELECT data FROM project_snapshot WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let mut rows = match data {
            Some(data) => serde_json::from_str::<Vec<Project>>(&data)?,
            None => Vec::new(),
        };
        rows.retain(|project| {
            if project.manual {
                settings.manual.iter().any(|scope| {
                    scope.path == project.source.path
                        && projects::manual_owner(scope, &settings.roots) == &project.source.scope
                })
            } else {
                settings.roots.contains(&project.source.scope)
            }
        });
        for project in &mut rows {
            project.offline = true;
        }
        Ok(rows)
    }

    pub fn scan_projects(
        &self,
        settings: &Settings,
        cancel: &AtomicBool,
        progress: impl FnMut(usize),
    ) -> Result<projects::Scan> {
        let previous = self.cached_projects(settings)?;
        let mut scan = projects::scan(
            &settings.roots,
            &settings.manual,
            &settings.rules,
            &settings.outputs,
            cancel,
            progress,
        )?;
        let found: BTreeSet<_> = scan
            .projects
            .iter()
            .map(|project| project.source.path.clone())
            .collect();
        scan.projects.extend(previous.into_iter().filter(|project| {
            !found.contains(&project.source.path)
                && (scan.incomplete.contains(&project.source.scope.path)
                    || (project.manual && scan.incomplete.contains(&project.source.path)))
        }));
        projects::order(&mut scan.projects);
        let data = serde_json::to_string(&scan.projects)?;
        check_cancel(cancel)?;
        self.connection.execute(
            "INSERT INTO project_snapshot (id, data) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            [data],
        )?;
        Ok(scan)
    }

    pub fn tag_sample(&self, source: &Source, tags: &[String]) -> Result<()> {
        self.connection.execute(
            "INSERT INTO sample_tags (path, tags) VALUES (?1, ?2) ON CONFLICT(path) DO UPDATE SET tags = excluded.tags",
            params![path_text(&source.path)?, serde_json::to_string(tags)?],
        )?;
        Ok(())
    }

    pub fn reset_sample_tags(&self, source: &Source) -> Result<()> {
        self.connection.execute(
            "DELETE FROM sample_tags WHERE path = ?1",
            [path_text(&source.path)?],
        )?;
        Ok(())
    }

    pub fn samples(&self, root: &Scope) -> Result<Vec<Sample>> {
        let mut statement = self.connection.prepare(
            "SELECT s.path, s.stamp, s.analysis, t.tags FROM samples AS s LEFT JOIN sample_tags AS t USING (path)
             WHERE s.root = ?1 ORDER BY s.path",
        )?;
        let rows = statement.query_map([path_text(&root.path)?], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (path, stamp, analysis, tags) = row?;
            let analysis: Analysis = serde_json::from_str(&analysis)?;
            let customized = tags.is_some();
            let tags = match tags {
                Some(tags) => serde_json::from_str(&tags)?,
                None => analysis.tags.clone(),
            };
            Ok(Sample {
                source: root.source(path.into()),
                analysis,
                tags,
                customized,
                stamp: serde_json::from_str(&stamp)?,
            })
        })
        .collect()
    }

    pub fn samples_in<'a>(
        &self,
        folders: impl IntoIterator<Item = &'a Scope>,
    ) -> Result<Vec<Sample>> {
        let mut rows = Vec::new();
        for folder in folders {
            rows.extend(self.samples(folder)?);
        }
        Ok(unique_samples(rows))
    }

    pub fn scan_sample_folders(
        &mut self,
        folders: &[Scope],
        excluded: &[PathBuf],
        cancel: &AtomicBool,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<SampleScan> {
        let mut combined = SampleScan {
            rows: Vec::new(),
            decoded: 0,
            skipped: 0,
            warnings: Vec::new(),
        };
        for folder in folders {
            check_cancel(cancel)?;
            let found = combined.rows.len();
            let decoded = combined.decoded;
            match self.scan_samples(folder, excluded, cancel, |n, d| {
                progress(found + n, decoded + d);
            }) {
                Ok(scan) => {
                    combined.rows.extend(scan.rows);
                    combined.decoded += scan.decoded;
                    combined.skipped += scan.skipped;
                    combined.warnings.extend(scan.warnings);
                }
                Err(error) if error.is::<Cancelled>() => return Err(error),
                Err(error) => {
                    combined
                        .warnings
                        .push(format!("{} · {error:#}", folder.path.display()));
                    combined.rows.extend(self.samples(folder)?);
                }
            }
        }
        combined.rows = unique_samples(combined.rows);
        Ok(combined)
    }

    pub fn scan_samples(
        &mut self,
        root: &Scope,
        excluded: &[PathBuf],
        cancel: &AtomicBool,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<SampleScan> {
        ensure!(root.directory, "Choose a sample folder.");
        root.check(&root.path)?;
        let previous: BTreeMap<PathBuf, (Stamp, u32)> = self
            .samples(root)?
            .into_iter()
            .map(|sample| (sample.source.path, (sample.stamp, sample.analysis.revision)))
            .collect();
        let mut seen = BTreeSet::new();
        let mut changed = Vec::new();
        let mut warnings = Vec::new();
        let mut skipped = 0;
        for entry in WalkDir::new(&root.path)
            .same_file_system(true)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0
                    || (!entry.file_name().to_string_lossy().starts_with('.')
                        && !excluded.iter().any(|output| entry.path() == output))
            })
        {
            check_cancel(cancel)?;
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warnings.push(error.to_string());
                    continue;
                }
            };
            if entry.file_type().is_symlink() {
                skipped += 1;
            }
            if !entry.file_type().is_file() || !is_audio(entry.path()) {
                continue;
            }
            let source = root.source(entry.into_path());
            if source.path.to_str().is_none() {
                skipped += 1;
                continue;
            }
            let stamp = match source.metadata() {
                Ok(meta) => Stamp::from(&meta),
                Err(error) => {
                    warnings.push(format!("{error:#}"));
                    continue;
                }
            };
            seen.insert(source.path.clone());
            if previous.get(&source.path) != Some(&(stamp, audio::ANALYSIS_REVISION)) {
                let analysis = match audio::analyze(&source, cancel) {
                    Ok(analysis) => analysis,
                    Err(error) if error.is::<Cancelled>() => return Err(error),
                    Err(error) => audio::unavailable(&source, &error),
                };
                check_cancel(cancel)?;
                match source.metadata() {
                    Ok(meta) if Stamp::from(&meta) == stamp => {
                        changed.push((source.path, stamp, analysis))
                    }
                    _ => warnings.push(format!(
                        "File changed during scan · {}",
                        source.path.display()
                    )),
                }
            }
            progress(seen.len(), changed.len());
        }
        check_cancel(cancel)?;
        root.check(&root.path)?;
        let root_text = path_text(&root.path)?;
        let transaction = self.connection.transaction()?;
        for (path, stamp, analysis) in &changed {
            check_cancel(cancel)?;
            transaction.execute(
                "INSERT INTO samples VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(root, path) DO UPDATE SET stamp = excluded.stamp, analysis = excluded.analysis",
                params![root_text, path_text(path)?, serde_json::to_string(stamp)?, serde_json::to_string(analysis)?],
            )?;
        }
        if warnings.is_empty() {
            for path in previous.keys().filter(|path| !seen.contains(*path)) {
                check_cancel(cancel)?;
                transaction.execute(
                    "DELETE FROM samples WHERE root = ?1 AND path = ?2",
                    params![root_text, path_text(path)?],
                )?;
            }
        }
        check_cancel(cancel)?;
        root.check(&root.path)?;
        transaction.commit()?;
        Ok(SampleScan {
            rows: self.samples(root)?,
            decoded: changed.len(),
            skipped,
            warnings,
        })
    }
}

fn unique_samples(mut rows: Vec<Sample>) -> Vec<Sample> {
    rows.sort_by(|a, b| a.source.path.cmp(&b.source.path));
    rows.dedup_by(|a, b| a.source.path == b.source.path);
    rows
}

pub fn edited_tags(input: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    input
        .split([',', '\n'])
        .map(str::trim)
        .filter(|part| !part.is_empty() && seen.insert(key(part)))
        .map(str::to_owned)
        .collect()
}

pub fn matches_tags(tags: &[String], filters: &[String]) -> bool {
    filters
        .iter()
        .all(|filter| tags.iter().any(|tag| key(tag) == key(filter)))
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .context("Choose a path with valid UTF-8 characters.")
}
