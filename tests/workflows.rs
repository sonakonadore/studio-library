use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use studio_library::{
    catalog::{Settings, Store, VirtualFolder, edited_tags, matches_tags},
    copying::{project_plan, sample_plan},
    projects::{self, Rule, RuleKind},
    scope::{Cancelled, Scope},
};

fn fixture(root: &Path, relative: &str, bytes: &[u8]) -> Result<PathBuf> {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().context("fixture parent")?)?;
    fs::write(&path, bytes)?;
    Ok(path)
}

#[test]
fn groups_versions_and_flags_distinct_same_title_candidates() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = Scope::select(temp.path())?;
    for path in [
        "Songs A/Orbit v01.als",
        "Songs A/Orbit v02.als",
        "Work/Harvest v01/Harvest.cpr",
        "Work/Harvest v02/Harvest.cpr",
        "Songs A/Dawn v03.als",
        "Songs B/Dawn v01.als",
    ] {
        fixture(&root.path, path, b"mock project")?;
    }
    let scan = projects::scan(&[root], &[], &[], &[], &AtomicBool::new(false), |_| {})?;
    assert_eq!(scan.projects.len(), 6);
    for title in ["Orbit", "Harvest"] {
        let versions = scan
            .projects
            .iter()
            .filter(|project| project.suggested.title == title)
            .collect::<Vec<_>>();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].suggested.version, "v01");
        assert_eq!(versions[1].suggested.version, "v02");
        assert!(
            versions
                .iter()
                .all(|project| project.suggested.group == title && !project.review)
        );
    }
    assert!(
        scan.projects
            .iter()
            .filter(|project| project.suggested.title == "Dawn")
            .all(|project| project.review)
    );
    Ok(())
}

#[test]
fn copies_fender_history_and_logic_package_without_following_links() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("source");
    let output = temp.path().join("output");
    fs::create_dir_all(&output)?;
    let song = fixture(&source, "Orbit/Orbit v01.song", b"fake Fender session")?;
    fixture(&source, "Orbit/Media/sub/take.wav", b"fake audio")?;
    fixture(&source, "Orbit/History/Orbit v00.song", b"fake backup")?;
    let logic = fixture(
        &source,
        "Dawn v02.logicx/Alternatives/000/ProjectData",
        b"fake Logic package",
    )?;
    let outside = fixture(temp.path(), "outside/Private.als", b"outside")?;
    symlink(&outside, source.join("Out of Scope.als"))?;
    symlink(&outside, source.join("Orbit/Media/linked.wav"))?;
    let stop = AtomicBool::new(false);
    let scope = Scope::select(&source)?;
    let destination = Scope::select(&output)?;
    let scan = projects::scan(&[scope], &[], &[], &[], &stop, |_| {})?;
    assert_eq!(scan.projects.len(), 2);
    assert_eq!(scan.skipped, 2);
    let session = scan
        .projects
        .iter()
        .find(|project| project.daw == "Fender Studio Pro")
        .context("Fender")?;
    assert_eq!(session.nearby.len(), 1);
    let plan = project_plan(session, &session.suggested, &[], destination.clone(), &stop)?;
    assert_eq!(plan.skipped, 1);
    assert_eq!(plan.bytes(), 40);
    assert!(!plan.target().exists());
    let report = plan.execute(&stop, |_, _| {});
    assert!(report.outcome.is_none(), "{}", report.message());
    let target = plan.target();
    assert_eq!(fs::read(target.join("Orbit v01.song"))?, fs::read(song)?);
    assert_eq!(fs::read(target.join("Media/sub/take.wav"))?, b"fake audio");
    assert_eq!(
        fs::read(target.join("History/Orbit v00.song"))?,
        b"fake backup"
    );
    assert!(!target.join("Media/linked.wav").exists());
    assert!(project_plan(session, &session.suggested, &[], destination.clone(), &stop).is_err());
    let package = scan
        .projects
        .iter()
        .find(|project| project.directory)
        .context("Logic package")?;
    let plan = project_plan(package, &package.suggested, &[], destination, &stop)?;
    let report = plan.execute(&stop, |_, _| {});
    assert!(report.outcome.is_none(), "{}", report.message());
    assert_eq!(
        fs::read(
            plan.target()
                .join("Dawn v02.logicx/Alternatives/000/ProjectData")
        )?,
        fs::read(logic)?
    );
    Ok(())
}

#[test]
fn manual_scope_and_saved_rules_are_distinct() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("projects");
    let manual = fixture(&source, "River v02.desk", b"custom session")?;
    fixture(&source, "River v03.desk", b"another version")?;
    fixture(&source, "Desk Project/internal.bin", b"custom folder")?;
    let scope = Scope::select(&source)?;
    let manual = Scope::select(&manual)?;
    let stop = AtomicBool::new(false);
    let direct = projects::scan(&[], std::slice::from_ref(&manual), &[], &[], &stop, |_| {})?;
    assert_eq!(direct.projects.len(), 1);
    assert!(direct.projects[0].manual);
    let db = temp.path().join("data.sqlite3");
    let store = Store::open(&db)?;
    store.save_settings(&Settings {
        rules: vec![
            Rule::new("Desk", RuleKind::Extension, ".DESK")?,
            Rule::new("Desk", RuleKind::FolderName, "Desk Project")?,
        ],
        primary: Some("Desk".into()),
        ..Settings::default()
    })?;
    let settings = Store::open(&db)?.settings()?;
    let scan = projects::scan(&[scope], &[manual], &settings.rules, &[], &stop, |_| {})?;
    assert_eq!(scan.projects.len(), 3);
    assert!(scan.projects.iter().all(|project| project.daw == "Desk"));
    assert!(scan.projects.iter().any(|project| project.directory));
    Ok(())
}

#[test]
fn validates_cancellations_disconnects_permissions_and_new_collisions() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("drive");
    let destination = temp.path().join("personal");
    fs::create_dir(&destination)?;
    let path = fixture(&source, "test.wav", &vec![13; 3 * 1024 * 1024])?;
    let scope = Scope::select(&source)?;
    let sample = scope.source(scope.path.join("test.wav"));
    let output = Scope::select(&destination)?;
    let stop = AtomicBool::new(false);
    let plan = sample_plan(std::slice::from_ref(&sample), output, &stop)?;
    let report = plan.execute(&stop, |_, _| stop.store(true, Ordering::Relaxed));
    assert!(report.outcome.is_some_and(|error| error.is::<Cancelled>()));
    assert_eq!(fs::read_dir(&destination)?.count(), 0);
    assert_eq!(fs::read(&path)?.len(), 3 * 1024 * 1024);
    stop.store(false, Ordering::Relaxed);

    fs::set_permissions(&path, fs::Permissions::from_mode(0o000))?;
    let unprivileged = unsafe { libc::geteuid() != 0 };
    if unprivileged {
        assert!(sample.open().is_err());
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    let plan = sample_plan(
        std::slice::from_ref(&sample),
        Scope::select(&destination)?,
        &stop,
    )?;
    fs::rename(&source, temp.path().join("offline"))?;
    fs::create_dir(&source)?;
    fixture(&source, "test.wav", b"new volume lookalike")?;
    let report = plan.execute(&stop, |_, _| {});
    assert!(report.outcome.is_some());
    assert_eq!(fs::read_dir(&destination)?.count(), 0);
    assert_eq!(fs::read(&path)?, b"new volume lookalike");
    assert_eq!(
        fs::read(temp.path().join("offline/test.wav"))?.len(),
        3 * 1024 * 1024
    );

    let scope = Scope::select(&source)?;
    let plan = sample_plan(
        &[scope.source(scope.path.join("test.wav"))],
        Scope::select(&destination)?,
        &stop,
    )?;
    fixture(&destination, "test.wav", b"existing file")?;
    let report = plan.execute(&stop, |_, _| {});
    assert!(report.outcome.is_some());
    assert_eq!(fs::read(destination.join("test.wav"))?, b"existing file");
    Ok(())
}

fn pcm_wave() -> Vec<u8> {
    let frames = 4_000u32;
    let pcm_bytes = frames * 2;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + pcm_bytes).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(8_000u32.to_le_bytes());
    bytes.extend(16_000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(pcm_bytes.to_le_bytes());
    for frame in 0..frames {
        let wave = (f64::from(frame) * std::f64::consts::TAU / 37.0).sin();
        bytes.extend(((wave * 3000.0) as i16).to_le_bytes());
    }
    bytes
}

fn pcm_aiff() -> Vec<u8> {
    let frames = 512u32;
    let pcm_bytes = frames * 2;
    let mut bytes = b"FORM".to_vec();
    bytes.extend((46 + pcm_bytes).to_be_bytes());
    bytes.extend(b"AIFFCOMM");
    bytes.extend(18u32.to_be_bytes());
    bytes.extend(1u16.to_be_bytes());
    bytes.extend(frames.to_be_bytes());
    bytes.extend(16u16.to_be_bytes());
    bytes.extend([0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]); // 44,100 Hz
    bytes.extend(b"SSND");
    bytes.extend((8 + pcm_bytes).to_be_bytes());
    bytes.extend([0; 8]);
    bytes.resize(bytes.len() + pcm_bytes as usize, 0);
    bytes
}

#[test]
fn incrementally_indexes_three_audio_formats_and_persists_tags() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("splice");
    let original = pcm_wave();
    let kick = fixture(&root, "House/Kick oneshot.wav", &original)?;
    fixture(&root, "House/Pad.aiff", &pcm_aiff())?;
    let mut silent_mp3_frame = vec![0; 417];
    silent_mp3_frame[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0xc4]);
    fixture(&root, "Mystery.mp3", &silent_mp3_frame.repeat(8))?;
    fixture(&root, "corrupt.wav", b"bad data")?;
    symlink(&kick, root.join("link.wav"))?;
    let scope = Scope::select(&root)?;
    let db = temp.path().join("cache.sqlite3");
    let mut store = Store::open(&db)?;
    let stop = AtomicBool::new(false);
    let scan = store.scan_samples(&scope, &[], &stop, |_, _| {})?;
    assert_eq!(scan.decoded, 4);
    assert_eq!(scan.skipped, 1);
    assert_eq!(
        scan.rows
            .iter()
            .filter(|item| item.analysis.seconds.is_some())
            .count(),
        3,
        "{:?}",
        scan.rows
            .iter()
            .map(|row| (&row.source.path, &row.analysis.error))
            .collect::<Vec<_>>()
    );
    let kick_row = scan
        .rows
        .iter()
        .find(|item| item.source.path.ends_with("Kick oneshot.wav"))
        .context("kick sample")?;
    assert!(matches_tags(
        &kick_row.tags,
        &edited_tags("악기 · 킥, 장르 · 하우스, 밝기 · 어두움, 형태 · 원샷 후보")
    ));
    let tags = edited_tags("개인 태그, personal, PERSONAL");
    store.tag_sample(&kick_row.source, &tags)?;
    store.save_settings(&Settings {
        samples: Some(scope.clone()),
        virtual_folders: vec![VirtualFolder {
            name: "즐겨찾기".into(),
            tags: tags.clone(),
        }],
        ..Settings::default()
    })?;
    drop(store);
    let mut store = Store::open(&db)?;
    let settings = store.settings()?;
    let scan = store.scan_samples(&scope, &[], &stop, |_, _| {})?;
    assert_eq!(scan.decoded, 0);
    let matches = scan
        .rows
        .iter()
        .filter(|row| matches_tags(&row.tags, &settings.virtual_folders[0].tags))
        .count();
    assert_eq!(matches, 1);
    let mut edited = original;
    edited.extend([0x0, 0x0]);
    fs::write(&kick, &edited)?;
    let scan = store.scan_samples(&scope, &[], &stop, |_, _| {})?;
    assert_eq!(scan.decoded, 1);
    let row = scan
        .rows
        .iter()
        .find(|row| row.customized)
        .context("customized sample")?;
    let output = temp.path().join("personal");
    fs::create_dir(&output)?;
    let destination = Scope::select(&output)?;
    let plan = sample_plan(
        std::slice::from_ref(&row.source),
        destination.clone(),
        &stop,
    )?;
    let report = plan.execute(&stop, |_, _| {});
    assert!(report.outcome.is_none(), "{}", report.message());
    assert_eq!(fs::read(output.join("Kick oneshot.wav"))?, edited);
    assert_eq!(fs::read(kick)?, edited);
    assert!(sample_plan(std::slice::from_ref(&row.source), destination, &stop).is_err());
    Ok(())
}
