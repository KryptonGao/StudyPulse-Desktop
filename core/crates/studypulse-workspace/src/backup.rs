use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    ComprehensiveExamFull, DiaryEntry, ExamFull, IosRecord, MistakeNoteFull, Result, TaskItem,
    Workspace, WorkspaceError, validate_wire_relative_path,
};

const FORMAT_IDENTIFIER: &str = "com.chenkai.gao.studypulse.backup";
const MAX_ARCHIVE_ENTRIES: usize = 10_000;
const MAX_SINGLE_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

const REQUIRED_FILES: &[&str] = &[
    "manifest.json",
    "checksums.json",
    "data/subjects.json",
    "data/grades.jsonl",
    "data/mistakes.jsonl",
    "data/exams.jsonl",
    "data/comprehensive_exams.jsonl",
    "data/tasks.jsonl",
    "data/phases.jsonl",
    "data/routines.jsonl",
    "data/routine_instances.jsonl",
    "data/diary_entries.jsonl",
    "data/study_sessions.jsonl",
    "data/profile.json",
    "data/plant_state.json",
    "data/achievements.json",
    "data/coach_data.jsonl",
    "data/preferences.json",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub format_identifier: String,
    pub format_version: u32,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub app_build: String,
    pub schema_version: u32,
    pub created_at: String,
    #[serde(default)]
    pub record_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub includes_media: bool,
    #[serde(default)]
    pub includes_derived_health_data: bool,
    #[serde(default)]
    pub encrypted: bool,
    #[serde(default)]
    pub locale: String,
    #[serde(default)]
    pub media_file_count: usize,
    #[serde(default)]
    pub media_bytes: u64,
    #[serde(default)]
    pub missing_media_count: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Checksums {
    algorithm: String,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupConflict {
    pub key: String,
    pub domain: String,
    pub record_id: Option<String>,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct BackupInspection {
    pub id: String,
    pub manifest: BackupManifest,
    pub added_records: u64,
    pub identical_records: u64,
    pub conflicts: Vec<BackupConflict>,
    pub warnings: Vec<String>,
    pub(crate) staging_path: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RestoreMode {
    Replace,
    Merge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupResolution {
    pub conflict_key: String,
    pub use_incoming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub imported_records: u64,
    pub kept_local_records: u64,
    pub recovery_path: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportOptions {
    #[serde(default = "default_true")]
    pub includes_media: bool,
    #[serde(default)]
    pub includes_derived_health_data: bool,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub app_build: String,
    #[serde(default = "default_locale")]
    pub locale: String,
}

fn default_true() -> bool {
    true
}

fn default_locale() -> String {
    "en_US".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportResult {
    pub archive_path: String,
    pub manifest: BackupManifest,
}

impl Workspace {
    pub fn inspect_backup(&self, archive_path: impl AsRef<Path>) -> Result<BackupInspection> {
        let id = Uuid::new_v4().to_string();
        let staging = self.root().join(".studypulse/cache/imports").join(&id);
        fs::create_dir_all(&staging)?;
        if let Err(error) = extract_and_validate(archive_path.as_ref(), &staging) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        let manifest: BackupManifest =
            serde_json::from_slice(&fs::read(staging.join("manifest.json"))?)?;
        validate_decoded_content(&staging, &manifest)?;
        let (added_records, identical_records, conflicts) = compare_import(self.root(), &staging)?;
        Ok(BackupInspection {
            id,
            warnings: manifest.warnings.clone(),
            manifest,
            added_records,
            identical_records,
            conflicts,
            staging_path: staging,
        })
    }

    pub fn apply_backup(
        &self,
        inspection: &BackupInspection,
        mode: RestoreMode,
        resolutions: &[BackupResolution],
    ) -> Result<ImportReport> {
        if !inspection.staging_path.exists() {
            return Err(WorkspaceError::ImportSessionNotFound);
        }
        let _guard = self.exclusive_write();
        let operation_id = Uuid::new_v4().to_string();
        let recovery = self
            .root()
            .join(".studypulse/recovery")
            .join(format!("BeforeRestore-{operation_id}"));
        fs::create_dir_all(&recovery)?;
        copy_tree(&self.root().join("Data"), &recovery.join("Data"))?;
        copy_tree(&self.root().join("Media"), &recovery.join("Media"))?;

        let transaction = self
            .root()
            .join(".studypulse/cache")
            .join(format!("apply-{operation_id}"));
        let transaction_data = transaction.join("Data");
        let transaction_media = transaction.join("Media");
        fs::create_dir_all(&transaction_data)?;
        fs::create_dir_all(&transaction_media)?;

        let resolution_map: HashMap<&str, bool> = resolutions
            .iter()
            .map(|resolution| (resolution.conflict_key.as_str(), resolution.use_incoming))
            .collect();
        let (imported_records, kept_local_records) = match mode {
            RestoreMode::Replace => {
                copy_tree(&inspection.staging_path.join("data"), &transaction_data)?;
                copy_tree(&inspection.staging_path.join("media"), &transaction_media)?;
                (
                    inspection.manifest.record_counts.values().sum::<usize>() as u64,
                    0,
                )
            }
            RestoreMode::Merge => {
                copy_tree(&self.root().join("Data"), &transaction_data)?;
                copy_tree(&self.root().join("Media"), &transaction_media)?;
                merge_data(
                    &inspection.staging_path.join("data"),
                    &transaction_data,
                    &resolution_map,
                )?
            }
        };

        if mode == RestoreMode::Merge {
            merge_media(
                &inspection.staging_path.join("media"),
                &transaction_media,
                &resolution_map,
            )?;
        }

        let old_data = self
            .root()
            .join(".studypulse/cache")
            .join(format!("old-data-{operation_id}"));
        let old_media = self
            .root()
            .join(".studypulse/cache")
            .join(format!("old-media-{operation_id}"));
        let data = self.root().join("Data");
        let media = self.root().join("Media");
        fs::rename(&data, &old_data)?;
        if let Err(error) = fs::rename(&transaction_data, &data) {
            fs::rename(&old_data, &data)?;
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&media, &old_media) {
            let _ = fs::rename(&data, &transaction_data);
            let _ = fs::rename(&old_data, &data);
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&transaction_media, &media) {
            let _ = fs::rename(&data, &transaction_data);
            let _ = fs::rename(&old_data, &data);
            let _ = fs::rename(&old_media, &media);
            return Err(error.into());
        }
        let _ = fs::remove_dir_all(old_data);
        let _ = fs::remove_dir_all(old_media);
        let _ = fs::remove_dir_all(transaction);
        let _ = fs::remove_dir_all(&inspection.staging_path);

        Ok(ImportReport {
            imported_records,
            kept_local_records,
            recovery_path: recovery.to_string_lossy().into_owned(),
            warnings: inspection.warnings.clone(),
        })
    }

    pub fn cancel_backup(&self, inspection: &BackupInspection) -> Result<()> {
        if inspection
            .staging_path
            .starts_with(self.root().join(".studypulse/cache/imports"))
            && inspection.staging_path.exists()
        {
            fs::remove_dir_all(&inspection.staging_path)?;
        }
        Ok(())
    }

    pub fn export_backup(
        &self,
        archive_path: impl AsRef<Path>,
        options: BackupExportOptions,
    ) -> Result<BackupExportResult> {
        let operation_id = Uuid::new_v4().to_string();
        let staging = self
            .root()
            .join(".studypulse/cache")
            .join(format!("export-{operation_id}"));
        let data = staging.join("data");
        let media = staging.join("media");
        fs::create_dir_all(&data)?;
        initialize_export_data(self.root(), &data)?;
        if options.includes_media {
            copy_tree(&self.root().join("Media"), &media)?;
        }

        let mut counts = BTreeMap::new();
        for entry in fs::read_dir(&data)? {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(key) = manifest_key_for_file(name) else {
                continue;
            };
            if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
                counts.insert(key.into(), count_nonempty_lines(&path)?);
            } else if name == "subjects.json" {
                let values: Vec<Value> = serde_json::from_slice(&fs::read(path)?)?;
                counts.insert(key.into(), values.len());
            } else if name.ends_with(".json") {
                counts.insert(key.into(), 1);
            }
        }

        let (media_file_count, media_bytes) = if options.includes_media {
            media_stats(&media)?
        } else {
            (0, 0)
        };
        let warnings = missing_media_warnings(&data, &media, options.includes_media)?;
        let manifest = BackupManifest {
            format_identifier: FORMAT_IDENTIFIER.into(),
            format_version: 1,
            app_version: options.app_version,
            app_build: options.app_build,
            schema_version: 4,
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            record_counts: counts,
            includes_media: options.includes_media,
            includes_derived_health_data: options.includes_derived_health_data,
            encrypted: false,
            locale: options.locale,
            media_file_count,
            media_bytes,
            missing_media_count: warnings.len(),
            warnings,
        };
        fs::write(
            staging.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;

        let mut checksummed = BTreeMap::new();
        for entry in WalkDir::new(&staging)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let relative = entry
                .path()
                .strip_prefix(&staging)
                .map_err(|_| WorkspaceError::PathEscape(entry.path().display().to_string()))?;
            let wire = relative.to_string_lossy().replace('\\', "/");
            checksummed.insert(wire, sha256_file(entry.path())?);
        }
        fs::write(
            staging.join("checksums.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "algorithm": "SHA-256",
                "files": checksummed,
            }))?,
        )?;
        create_archive(&staging, archive_path.as_ref())?;
        let result = BackupExportResult {
            archive_path: archive_path.as_ref().to_string_lossy().into_owned(),
            manifest,
        };
        let _ = fs::remove_dir_all(staging);
        Ok(result)
    }
}

fn extract_and_validate(archive_path: &Path, staging: &Path) -> Result<()> {
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(WorkspaceError::InvalidBackup(
            "archive has too many entries".into(),
        ));
    }
    let mut seen = HashSet::new();
    let mut total_size = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let raw_name = entry.name().replace('\\', "/");
        let name = raw_name.trim_end_matches('/');
        if name.is_empty() {
            continue;
        }
        validate_wire_relative_path(name)
            .map_err(|_| WorkspaceError::InvalidBackup(format!("unsafe path: {name}")))?;
        if !seen.insert(name.to_string()) {
            return Err(WorkspaceError::InvalidBackup(format!(
                "duplicate archive entry: {name}"
            )));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(WorkspaceError::InvalidBackup(format!(
                "symbolic link entry: {name}"
            )));
        }
        if entry.size() > MAX_SINGLE_FILE_BYTES {
            return Err(WorkspaceError::InvalidBackup(format!(
                "entry is too large: {name}"
            )));
        }
        total_size = total_size.saturating_add(entry.size());
        if total_size > MAX_TOTAL_BYTES {
            return Err(WorkspaceError::InvalidBackup(
                "archive expands beyond the allowed size".into(),
            ));
        }
        let destination = staging.join(name);
        if entry.is_dir() || raw_name.ends_with('/') {
            fs::create_dir_all(destination)?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(destination)?;
        std::io::copy(&mut entry, &mut output)?;
        output.flush()?;
    }

    for required in REQUIRED_FILES {
        if !staging.join(required).is_file() {
            return Err(WorkspaceError::InvalidBackup(format!(
                "required file is missing: {required}"
            )));
        }
    }
    let manifest: BackupManifest =
        serde_json::from_slice(&fs::read(staging.join("manifest.json"))?)?;
    if manifest.format_identifier != FORMAT_IDENTIFIER || manifest.format_version != 1 {
        return Err(WorkspaceError::InvalidBackup(
            "format identifier or version is not supported".into(),
        ));
    }
    if !matches!(manifest.schema_version, 3 | 4) {
        return Err(WorkspaceError::UnsupportedBackupSchema(
            manifest.schema_version,
        ));
    }
    if manifest.encrypted {
        return Err(WorkspaceError::InvalidBackup(
            "encrypted backups are not supported".into(),
        ));
    }
    let checksums: Checksums = serde_json::from_slice(&fs::read(staging.join("checksums.json"))?)?;
    if !checksums.algorithm.eq_ignore_ascii_case("SHA-256") {
        return Err(WorkspaceError::InvalidBackup(
            "unsupported checksum algorithm".into(),
        ));
    }
    for required in REQUIRED_FILES
        .iter()
        .copied()
        .filter(|path| *path != "checksums.json")
    {
        if !checksums.files.contains_key(required) {
            return Err(WorkspaceError::InvalidBackup(format!(
                "checksum is missing: {required}"
            )));
        }
    }
    for (path, expected) in checksums.files {
        validate_wire_relative_path(&path)
            .map_err(|_| WorkspaceError::InvalidBackup(format!("unsafe checksum path: {path}")))?;
        let file = staging.join(&path);
        if !file.is_file() {
            return Err(WorkspaceError::InvalidBackup(format!(
                "checksummed file is missing: {path}"
            )));
        }
        let actual = sha256_file(&file)?;
        if !actual.eq_ignore_ascii_case(&expected) {
            return Err(WorkspaceError::InvalidBackup(format!(
                "checksum mismatch: {path}"
            )));
        }
    }
    Ok(())
}

fn initialize_export_data(root: &Path, destination: &Path) -> Result<()> {
    const JSONL_FILES: &[&str] = &[
        "grades.jsonl",
        "mistakes.jsonl",
        "exams.jsonl",
        "comprehensive_exams.jsonl",
        "tasks.jsonl",
        "phases.jsonl",
        "routines.jsonl",
        "routine_instances.jsonl",
        "diary_entries.jsonl",
        "study_sessions.jsonl",
        "time_investment_subjects.jsonl",
        "time_investment_subtasks.jsonl",
        "goal_rewards.jsonl",
        "coach_data.jsonl",
    ];
    // Copy every existing data file first. This is intentional: files owned by
    // newer iOS/P1/P2 features must survive a Desktop import/export even when
    // this build has no typed UI for them yet.
    let source_data = root.join("Data");
    if source_data.is_dir() {
        for entry in WalkDir::new(&source_data)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let relative = entry
                .path()
                .strip_prefix(&source_data)
                .map_err(|_| WorkspaceError::PathEscape(entry.path().display().to_string()))?;
            let target = destination.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), target)?;
        }
    }
    const JSON_FILES: &[(&str, &str)] = &[
        ("subjects.json", "[]"),
        (
            "profile.json",
            r#"{
                "username":"Student","age":16,"educationLevel":"High School",
                "educationSystem":"National Curriculum","region":"China",
                "selectedSubjects":[],"theme":"Auto","avatarFileName":null,
                "realName":"","grade":"","className":"","schoolName":"",
                "studentId":"","enrollmentYear":2026,"examYear":2026,
                "educationStage":"High School","regionCode":"mainland",
                "gender":"Not Specified","targetSchool":"","targetScore":0
            }"#,
        ),
        (
            "plant_state.json",
            r#"{
                "currentStage":"seed","history":[],
                "lastUpdated":"1970-01-01T00:00:00Z","forceOverride":null,
                "lastActivityAt":null,"simulatedStreak":null,"simulatedLastActiveDate":null
            }"#,
        ),
        (
            "achievements.json",
            r#"{
                "version":1,
                "config":{"mistakeReviewTarget":5,"gradeRecordTarget":1,"focusMinutesTarget":25,
                    "reminderEnabled":true,"reminderHour":20,"reminderMinute":0},
                "logs":[],"streak":{"current":0,"longest":0,"lastActiveDate":null,"totalActiveDays":0},
                "achievements":[],"cumulative":{"mistakeReviews":0,"gradesRecorded":0,"focusMinutes":0},
                "hasConfiguredGoals":false
            }"#,
        ),
        (
            "preferences.json",
            r#"{
                "dtoVersion":1,"appLanguage":null,"colorSchemeRaw":"system","chartTypeRaw":"line",
                "accentPaletteId":null,"glassEffectEnabled":false,"learningHeatmapOnTrends":true,
                "subjectMasteryRadarOnTrends":true,"activePhaseId":null,"cardSkinId":null,
                "timerAnimationId":null,"plantCardEnabled":true,"plantPetalColorId":null,
                "diaryEnabled":true,"diaryDailyReminderEnabled":false,"diaryDailyReminderHour":22,
                "habitInsightEnabled":false,"habitInsightNotificationEnabled":true,
                "habitInsightNotificationHour":7
            }"#,
        ),
    ];
    for file in JSONL_FILES {
        let source = root.join("Data").join(file);
        let target = destination.join(file);
        if !source.is_file() && !target.exists() {
            fs::write(target, [])?;
        }
    }
    for (file, default_value) in JSON_FILES {
        let source = root.join("Data").join(file);
        let target = destination.join(file);
        let invalid_singleton = target.exists()
            && matches!(
                *file,
                "profile.json" | "plant_state.json" | "achievements.json" | "preferences.json"
            )
            && fs::read_to_string(&target)
                .map(|value| value.trim() == "{}")
                .unwrap_or(false);
        if (!source.is_file() && !target.exists()) || invalid_singleton {
            fs::write(target, default_value.as_bytes())?;
        }
    }
    if root.join("Data/health_history.json").is_file() {
        fs::copy(
            root.join("Data/health_history.json"),
            destination.join("health_history.json"),
        )?;
    }
    Ok(())
}

fn manifest_key_for_file(name: &str) -> Option<&'static str> {
    match name {
        "subjects.json" => Some("subjects"),
        "grades.jsonl" => Some("grades"),
        "mistakes.jsonl" => Some("mistakes"),
        "exams.jsonl" => Some("exams"),
        "comprehensive_exams.jsonl" => Some("comprehensiveExams"),
        "tasks.jsonl" => Some("tasks"),
        "phases.jsonl" => Some("phases"),
        "routines.jsonl" => Some("routines"),
        "routine_instances.jsonl" => Some("routineInstances"),
        "diary_entries.jsonl" => Some("diaryEntries"),
        "study_sessions.jsonl" => Some("studySessions"),
        "time_investment_subjects.jsonl" => Some("timeInvestmentSubjects"),
        "time_investment_subtasks.jsonl" => Some("subTasks"),
        "goal_rewards.jsonl" => Some("goalRewards"),
        "profile.json" => Some("profile"),
        "plant_state.json" => Some("plantState"),
        "achievements.json" => Some("achievements"),
        "coach_data.jsonl" => None,
        "preferences.json" => None,
        "health_history.json" => None,
        _ => None,
    }
}

fn media_stats(root: &Path) -> Result<(usize, u64)> {
    if !root.exists() {
        return Ok((0, 0));
    }
    let mut count = 0;
    let mut bytes: u64 = 0;
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        count += 1;
        bytes = bytes.saturating_add(fs::metadata(entry.path())?.len());
    }
    Ok((count, bytes))
}

fn missing_media_warnings(data: &Path, media: &Path, includes_media: bool) -> Result<Vec<String>> {
    let mut references = Vec::new();
    for (file, field, category) in [
        ("grades.jsonl", "imageFileName", "images"),
        ("mistakes.jsonl", "audioFileName", "audio"),
    ] {
        let path = data.join(file);
        if !path.exists() {
            continue;
        }
        for (_, record) in read_jsonl_map(&path)? {
            if let Some(name) = record
                .get("value")
                .and_then(|value| value.get(field))
                .and_then(Value::as_str)
            {
                references.push((category, name.to_owned()));
            }
        }
    }
    let profile = data.join("profile.json");
    if profile.exists()
        && let Some(name) = serde_json::from_slice::<Value>(&fs::read(profile)?)?
            .get("avatarFileName")
            .and_then(Value::as_str)
    {
        references.push(("images", name.to_owned()));
    }
    let mut warnings = Vec::new();
    for (category, name) in references {
        let relative = format!("{category}/{name}");
        if !includes_media || !media.join(&relative).is_file() {
            warnings.push(format!("missing media reference: {relative}"));
        }
    }
    warnings.sort();
    warnings.dedup();
    Ok(warnings)
}

fn create_archive(source: &Path, archive_path: &Path) -> Result<()> {
    if let Some(parent) = archive_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(archive_path)?;
    let mut writer = ZipWriter::new(file);
    for entry in WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|_| WorkspaceError::PathEscape(entry.path().display().to_string()))?;
        let wire = relative.to_string_lossy().replace('\\', "/");
        writer.start_file(wire, SimpleFileOptions::default())?;
        let mut input = File::open(entry.path())?;
        std::io::copy(&mut input, &mut writer)?;
    }
    writer.finish()?;
    Ok(())
}

fn validate_decoded_content(staging: &Path, manifest: &BackupManifest) -> Result<()> {
    chrono::DateTime::parse_from_rfc3339(&manifest.created_at).map_err(|error| {
        WorkspaceError::InvalidBackup(format!("manifest createdAt is invalid: {error}"))
    })?;
    validate_typed_jsonl::<TaskItem>(&staging.join("data/tasks.jsonl"), |record| {
        if record.id != record.value.id {
            return Err("envelope id mismatch".into());
        }
        record.value.validate().map_err(|error| error.to_string())
    })?;
    validate_typed_jsonl::<MistakeNoteFull>(&staging.join("data/mistakes.jsonl"), |record| {
        (record.id == record.value.id)
            .then_some(())
            .ok_or_else(|| "envelope id mismatch".into())
    })?;
    validate_typed_jsonl::<ExamFull>(&staging.join("data/exams.jsonl"), |record| {
        (record.id == record.value.id)
            .then_some(())
            .ok_or_else(|| "envelope id mismatch".into())
    })?;
    validate_typed_jsonl::<ComprehensiveExamFull>(
        &staging.join("data/comprehensive_exams.jsonl"),
        |record| {
            (record.id == record.value.id)
                .then_some(())
                .ok_or_else(|| "envelope id mismatch".into())
        },
    )?;
    validate_typed_jsonl::<DiaryEntry>(&staging.join("data/diary_entries.jsonl"), |record| {
        if record.id != record.value.id {
            return Err("envelope id mismatch".into());
        }
        record.value.validate().map_err(|error| error.to_string())
    })?;

    for entry in fs::read_dir(staging.join("data"))? {
        let entry = entry?;
        match entry.path().extension().and_then(|value| value.to_str()) {
            Some("jsonl") => validate_generic_jsonl(&entry.path())?,
            Some("json") => {
                serde_json::from_slice::<Value>(&fs::read(entry.path())?).map_err(|error| {
                    WorkspaceError::MalformedData {
                        path: entry.path().display().to_string(),
                        detail: error.to_string(),
                    }
                })?;
            }
            _ => {}
        }
    }
    for (manifest_key, file_name) in [
        ("grades", "grades.jsonl"),
        ("mistakes", "mistakes.jsonl"),
        ("exams", "exams.jsonl"),
        ("comprehensiveExams", "comprehensive_exams.jsonl"),
        ("tasks", "tasks.jsonl"),
        ("phases", "phases.jsonl"),
        ("routines", "routines.jsonl"),
        ("routineInstances", "routine_instances.jsonl"),
        ("diaryEntries", "diary_entries.jsonl"),
        ("studySessions", "study_sessions.jsonl"),
        ("timeInvestmentSubjects", "time_investment_subjects.jsonl"),
        ("subTasks", "time_investment_subtasks.jsonl"),
        ("goalRewards", "goal_rewards.jsonl"),
    ] {
        let path = staging.join("data").join(file_name);
        if path.exists()
            && let Some(expected) = manifest.record_counts.get(manifest_key)
        {
            let actual = count_nonempty_lines(&path)?;
            if *expected != actual {
                return Err(WorkspaceError::InvalidBackup(format!(
                    "{manifest_key} record count does not match manifest"
                )));
            }
        }
    }
    validate_relationships(staging)?;
    Ok(())
}

fn validate_typed_jsonl<T>(
    path: &Path,
    validate: impl Fn(&IosRecord<T>) -> std::result::Result<(), String>,
) -> Result<()>
where
    T: for<'de> Deserialize<'de>,
{
    let file = File::open(path)?;
    let mut ids = HashSet::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: IosRecord<T> =
            serde_json::from_str(&line).map_err(|error| WorkspaceError::MalformedData {
                path: format!("{}:{}", path.display(), index + 1),
                detail: error.to_string(),
            })?;
        if !ids.insert(record.id) {
            return Err(WorkspaceError::InvalidBackup(format!(
                "duplicate UUID in {}",
                path.display()
            )));
        }
        if let Some(updated_at) = &record.updated_at {
            chrono::DateTime::parse_from_rfc3339(updated_at).map_err(|error| {
                WorkspaceError::MalformedData {
                    path: format!("{}:{}", path.display(), index + 1),
                    detail: format!("invalid updatedAt: {error}"),
                }
            })?;
        }
        validate(&record).map_err(|detail| WorkspaceError::MalformedData {
            path: format!("{}:{}", path.display(), index + 1),
            detail,
        })?;
    }
    Ok(())
}

fn validate_relationships(staging: &Path) -> Result<()> {
    let data = staging.join("data");
    let phases = read_jsonl_map(&data.join("phases.jsonl"))?
        .into_keys()
        .collect::<HashSet<_>>();
    let mistakes = read_jsonl_map(&data.join("mistakes.jsonl"))?
        .into_keys()
        .collect::<HashSet<_>>();
    let mut exams = read_jsonl_map(&data.join("exams.jsonl"))?
        .into_keys()
        .collect::<HashSet<_>>();
    exams.extend(read_jsonl_map(&data.join("comprehensive_exams.jsonl"))?.into_keys());
    let routines = read_jsonl_map(&data.join("routines.jsonl"))?
        .into_keys()
        .collect::<HashSet<_>>();
    let investment_subjects =
        read_jsonl_map_if_exists(&data.join("time_investment_subjects.jsonl"))?
            .into_keys()
            .collect::<HashSet<_>>();
    let subtasks = read_jsonl_map_if_exists(&data.join("time_investment_subtasks.jsonl"))?
        .into_keys()
        .collect::<HashSet<_>>();

    for (kind, file) in [
        ("task", "tasks.jsonl"),
        ("grade", "grades.jsonl"),
        ("mistake", "mistakes.jsonl"),
        ("exam", "exams.jsonl"),
        ("comprehensive exam", "comprehensive_exams.jsonl"),
        ("routine", "routines.jsonl"),
    ] {
        for (_, record) in read_jsonl_map(&data.join(file))? {
            let Some(value) = record.get("value") else {
                continue;
            };
            if let Some(phase_id) = value.get("phaseId").and_then(Value::as_str)
                && !phases.contains(phase_id)
            {
                return Err(WorkspaceError::InvalidBackup(format!(
                    "{kind} references missing phase UUID: {phase_id}"
                )));
            }
            if kind == "grade"
                && let Some(exam_id) = value.get("examId").and_then(Value::as_str)
                && !exams.contains(exam_id)
            {
                return Err(WorkspaceError::InvalidBackup(format!(
                    "grade references missing exam UUID: {exam_id}"
                )));
            }
        }
    }
    for (_, record) in read_jsonl_map(&data.join("exams.jsonl"))? {
        if let Some(ids) = record
            .get("value")
            .and_then(|value| value.get("examReview"))
            .and_then(|review| review.get("linkedMistakeIds"))
            .and_then(Value::as_array)
        {
            for id in ids.iter().filter_map(Value::as_str) {
                if !mistakes.contains(id) {
                    return Err(WorkspaceError::InvalidBackup(format!(
                        "exam review references missing mistake UUID: {id}"
                    )));
                }
            }
        }
    }
    for (_, record) in read_jsonl_map(&data.join("routine_instances.jsonl"))? {
        if let Some(routine_id) = record
            .get("value")
            .and_then(|value| value.get("routineId"))
            .and_then(Value::as_str)
            && !routines.contains(routine_id)
        {
            return Err(WorkspaceError::InvalidBackup(format!(
                "routine instance references missing routine UUID: {routine_id}"
            )));
        }
    }
    for (_, record) in read_jsonl_map_if_exists(&data.join("time_investment_subtasks.jsonl"))? {
        let Some(value) = record.get("value") else {
            continue;
        };
        let subject_id = value.get("subjectId").and_then(Value::as_str);
        if let Some(subject_id) = subject_id
            && !investment_subjects.contains(subject_id)
        {
            return Err(WorkspaceError::InvalidBackup(format!(
                "subTask references missing investment subject UUID: {subject_id}"
            )));
        }
        if let Some(parent_id) = value.get("parentSubTaskId").and_then(Value::as_str)
            && !subtasks.contains(parent_id)
        {
            return Err(WorkspaceError::InvalidBackup(format!(
                "subTask references missing parent UUID: {parent_id}"
            )));
        }
    }
    for (_, record) in read_jsonl_map(&data.join("study_sessions.jsonl"))? {
        if let Some(target) = record
            .get("value")
            .and_then(|value| value.get("investmentTarget"))
        {
            let id = target
                .get("subject")
                .or_else(|| target.get("subTask"))
                .and_then(|value| value.get("_0"))
                .and_then(Value::as_str);
            if let Some(id) = id
                && !investment_subjects.contains(id)
                && !subtasks.contains(id)
            {
                return Err(WorkspaceError::InvalidBackup(format!(
                    "study session references missing investment target UUID: {id}"
                )));
            }
        }
    }
    for (_, record) in read_jsonl_map_if_exists(&data.join("goal_rewards.jsonl"))? {
        if let Some(target) = record.get("value").and_then(|value| value.get("target")) {
            let id = target
                .get("subject")
                .or_else(|| target.get("subTask"))
                .and_then(|value| value.get("_0"))
                .and_then(Value::as_str);
            if let Some(id) = id
                && !investment_subjects.contains(id)
                && !subtasks.contains(id)
            {
                return Err(WorkspaceError::InvalidBackup(format!(
                    "goal reward references missing investment target UUID: {id}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_generic_jsonl(path: &Path) -> Result<()> {
    let file = File::open(path)?;
    let mut ids = HashSet::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(&line).map_err(|error| WorkspaceError::MalformedData {
                path: format!("{}:{}", path.display(), index + 1),
                detail: error.to_string(),
            })?;
        if let Some(id) = record_id(&value)
            && !ids.insert(id)
        {
            return Err(WorkspaceError::InvalidBackup(format!(
                "duplicate UUID in {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn compare_import(root: &Path, staging: &Path) -> Result<(u64, u64, Vec<BackupConflict>)> {
    let mut added = 0_u64;
    let mut identical = 0_u64;
    let mut conflicts = Vec::new();
    for entry in fs::read_dir(staging.join("data"))? {
        let entry = entry?;
        let incoming = entry.path();
        if incoming
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            let domain = incoming
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("unknown");
            let local = root.join("Data").join(entry.file_name());
            let local_map = if local.exists() {
                read_jsonl_map(&local)?
            } else {
                HashMap::new()
            };
            for (key, value) in read_jsonl_map(&incoming)? {
                match local_map.get(&key) {
                    None => added += 1,
                    Some(local) if local == &value => identical += 1,
                    Some(_) => conflicts.push(BackupConflict {
                        key: format!("{domain}:{key}"),
                        domain: domain.into(),
                        record_id: Some(key.clone()),
                        display_name: display_name(&value, &key),
                    }),
                }
            }
        } else {
            let local = root.join("Data").join(entry.file_name());
            if !local.exists() {
                added += 1;
            } else if fs::read(&local)? == fs::read(&incoming)? {
                identical += 1;
            } else {
                let domain = incoming
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("unknown");
                conflicts.push(BackupConflict {
                    key: format!("{domain}:singleton"),
                    domain: domain.into(),
                    record_id: None,
                    display_name: domain.into(),
                });
            }
        }
    }
    compare_media(root, staging, &mut added, &mut identical, &mut conflicts)?;
    Ok((added, identical, conflicts))
}

fn compare_media(
    root: &Path,
    staging: &Path,
    added: &mut u64,
    identical: &mut u64,
    conflicts: &mut Vec<BackupConflict>,
) -> Result<()> {
    let media = staging.join("media");
    if !media.exists() {
        return Ok(());
    }
    for entry in WalkDir::new(&media)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(&media)
            .map_err(|_| WorkspaceError::InvalidBackup("invalid media path".into()))?;
        let local = root.join("Media").join(relative);
        if !local.exists() {
            *added += 1;
        } else if sha256_file(&local)? == sha256_file(entry.path())? {
            *identical += 1;
        } else {
            let wire = relative.to_string_lossy().replace('\\', "/");
            conflicts.push(BackupConflict {
                key: format!("media:{wire}"),
                domain: "media".into(),
                record_id: None,
                display_name: wire,
            });
        }
    }
    Ok(())
}

fn merge_data(
    incoming: &Path,
    target: &Path,
    resolutions: &HashMap<&str, bool>,
) -> Result<(u64, u64)> {
    let mut imported = 0_u64;
    let mut kept = 0_u64;
    for entry in fs::read_dir(incoming)? {
        let entry = entry?;
        let source = entry.path();
        let destination = target.join(entry.file_name());
        let domain = source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown");
        if source
            .extension()
            .is_some_and(|extension| extension == "jsonl")
        {
            let mut local = if destination.exists() {
                read_jsonl_map(&destination)?
            } else {
                HashMap::new()
            };
            for (key, value) in read_jsonl_map(&source)? {
                let conflict_key = format!("{domain}:{key}");
                if local.contains_key(&key)
                    && resolutions
                        .get(conflict_key.as_str())
                        .is_some_and(|value| !*value)
                {
                    kept += 1;
                } else {
                    local.insert(key, value);
                    imported += 1;
                }
            }
            write_jsonl_map(&destination, local)?;
        } else {
            let conflict_key = format!("{domain}:singleton");
            if destination.exists()
                && resolutions
                    .get(conflict_key.as_str())
                    .is_some_and(|value| !*value)
            {
                kept += 1;
            } else {
                fs::copy(source, destination)?;
                imported += 1;
            }
        }
    }
    Ok((imported, kept))
}

fn merge_media(incoming: &Path, target: &Path, resolutions: &HashMap<&str, bool>) -> Result<()> {
    if !incoming.exists() {
        return Ok(());
    }
    for entry in WalkDir::new(incoming)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(incoming)
            .map_err(|_| WorkspaceError::InvalidBackup("invalid media path".into()))?;
        let destination = target.join(relative);
        let wire = relative.to_string_lossy().replace('\\', "/");
        let conflict_key = format!("media:{wire}");
        if destination.exists()
            && resolutions
                .get(conflict_key.as_str())
                .is_some_and(|value| !*value)
        {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), destination)?;
    }
    Ok(())
}

fn read_jsonl_map(path: &Path) -> Result<HashMap<String, Value>> {
    let file = File::open(path)?;
    let mut values = HashMap::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(&line).map_err(|error| WorkspaceError::MalformedData {
                path: format!("{}:{}", path.display(), index + 1),
                detail: error.to_string(),
            })?;
        let key = record_id(&value).unwrap_or_else(|| sha256_bytes(line.as_bytes()));
        values.insert(key, value);
    }
    Ok(values)
}

fn read_jsonl_map_if_exists(path: &Path) -> Result<HashMap<String, Value>> {
    if path.exists() {
        read_jsonl_map(path)
    } else {
        Ok(HashMap::new())
    }
}

fn write_jsonl_map(path: &Path, values: HashMap<String, Value>) -> Result<()> {
    let mut values: Vec<_> = values.into_iter().collect();
    values.sort_by(|left, right| left.0.cmp(&right.0));
    let mut file = File::create(path)?;
    for (_, value) in values {
        serde_json::to_writer(&mut file, &value)?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    Ok(())
}

fn record_id(value: &Value) -> Option<String> {
    value
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| value.get("value")?.get("id")?.as_str())
        .map(str::to_owned)
}

fn display_name(value: &Value, fallback: &str) -> String {
    ["title", "name", "subject"]
        .into_iter()
        .find_map(|key| {
            value
                .get("value")
                .and_then(|inner| inner.get(key))
                .or_else(|| value.get(key))
                .and_then(Value::as_str)
        })
        .unwrap_or(fallback)
        .to_owned()
}

fn count_nonempty_lines(path: &Path) -> Result<usize> {
    Ok(BufReader::new(File::open(path)?)
        .lines()
        .map_while(std::result::Result::ok)
        .filter(|line| !line.trim().is_empty())
        .count())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    if !source.exists() {
        fs::create_dir_all(destination)?;
        return Ok(());
    }
    fs::create_dir_all(destination)?;
    for entry in WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|_| WorkspaceError::PathEscape(entry.path().display().to_string()))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn make_golden_backup(root: &Path, name: &str, manifest: &str) -> PathBuf {
        let archive_path = root.join(name);
        let mut files = BTreeMap::<String, Vec<u8>>::new();
        files.insert("manifest.json".into(), manifest.as_bytes().to_vec());
        for path in REQUIRED_FILES {
            if *path == "manifest.json" || *path == "checksums.json" {
                continue;
            }
            let content = match *path {
                "data/subjects.json" => b"[]".to_vec(),
                "data/preferences.json" => br#"{"future":{"preserved":true}}"#.to_vec(),
                path if path.ends_with(".json") => b"{}".to_vec(),
                _ => Vec::new(),
            };
            files.insert((*path).into(), content);
        }
        let checksums = json!({
            "algorithm": "SHA-256",
            "files": files
                .iter()
                .map(|(path, bytes)| (path.clone(), sha256_bytes(bytes)))
                .collect::<BTreeMap<_, _>>()
        });
        files.insert(
            "checksums.json".into(),
            serde_json::to_vec(&checksums).unwrap(),
        );

        let file = File::create(&archive_path).unwrap();
        let mut writer = ZipWriter::new(file);
        for (path, bytes) in files {
            writer
                .start_file(path, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap();
        archive_path
    }

    #[test]
    fn rejects_unsafe_archive_path() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("bad.studypulsebackup");
        let file = File::create(&archive_path).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("../outside", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"bad").unwrap();
        writer.finish().unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir(&staging).unwrap();
        assert!(extract_and_validate(&archive_path, &staging).is_err());
        assert!(!temp.path().join("outside").exists());
    }

    #[test]
    fn accepts_schema_3_and_4_golden_backups() {
        for (schema, manifest) in [
            (3, include_str!("../tests/fixtures/backup_manifest_v3.json")),
            (4, include_str!("../tests/fixtures/backup_manifest_v4.json")),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
            let archive = make_golden_backup(
                temp.path(),
                &format!("schema-{schema}.studypulsebackup"),
                manifest,
            );
            let inspection = workspace.inspect_backup(archive).unwrap();
            assert_eq!(inspection.manifest.schema_version, schema);
            workspace.cancel_backup(&inspection).unwrap();
        }
    }

    #[test]
    fn replace_restore_keeps_unknown_fields_and_recovery_point() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Workspace")).unwrap();
        fs::write(
            workspace.root().join("Data/preferences.json"),
            br#"{"local":true}"#,
        )
        .unwrap();
        let archive = make_golden_backup(
            temp.path(),
            "schema-4.studypulsebackup",
            include_str!("../tests/fixtures/backup_manifest_v4.json"),
        );
        let inspection = workspace.inspect_backup(archive).unwrap();
        let report = workspace
            .apply_backup(&inspection, RestoreMode::Replace, &[])
            .unwrap();

        let restored: Value = serde_json::from_slice(
            &fs::read(workspace.root().join("Data/preferences.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(restored["future"]["preserved"], true);
        assert!(Path::new(&report.recovery_path).join("Data").is_dir());
    }

    #[test]
    fn desktop_export_is_schema4_round_trip_and_keeps_future_data() {
        let temp = tempfile::tempdir().unwrap();
        let source = Workspace::create(temp.path().join("Source")).unwrap();
        let now = "2026-07-31T12:00:00Z".to_string();
        source
            .append_task(TaskItem {
                id: Uuid::new_v4(),
                title: "Export me".into(),
                task_type: crate::TaskType::Homework,
                due_date: now.clone(),
                reminder_date: now.clone(),
                subject: "Math".into(),
                importance: 3,
                notes: "round trip".into(),
                is_completed: false,
                reminder_event_id: None,
                reminder_calendar_id: None,
                created_at: now,
                phase_id: None,
                coach_execution_data: None,
                coach_goal_id: None,
                coach_proposal_id: None,
                extra: BTreeMap::from([("futureTaskFlag".into(), json!(true))]),
            })
            .unwrap();
        fs::write(
            source.root().join("Data/future_domain.jsonl"),
            br#"{"dtoVersion":1,"id":"f6e62a0d-7b1f-4c58-a7a9-future000001","value":{"future":true}}
"#,
        )
        .unwrap();
        source
            .write_media("images/round-trip.bin", b"media")
            .unwrap();

        let archive = temp.path().join("desktop.studypulsebackup");
        let exported = source
            .export_backup(
                &archive,
                BackupExportOptions {
                    includes_media: true,
                    includes_derived_health_data: false,
                    app_version: "0.1.0".into(),
                    app_build: "test".into(),
                    locale: "en_US".into(),
                },
            )
            .unwrap();
        assert_eq!(exported.manifest.schema_version, 4);
        assert!(exported.manifest.includes_media);
        assert_eq!(exported.manifest.record_counts["tasks"], 1);

        let destination = Workspace::create(temp.path().join("Destination")).unwrap();
        let inspection = destination.inspect_backup(&archive).unwrap();
        destination
            .apply_backup(&inspection, RestoreMode::Replace, &[])
            .unwrap();
        assert_eq!(destination.read_tasks().unwrap().len(), 1);
        assert_eq!(
            serde_json::to_value(destination.read_tasks().unwrap()[0].clone()).unwrap()["futureTaskFlag"],
            true
        );
        assert_eq!(
            fs::read(destination.root().join("Data/future_domain.jsonl")).unwrap(),
            fs::read(source.root().join("Data/future_domain.jsonl")).unwrap()
        );
        assert_eq!(
            destination.read_media("images/round-trip.bin").unwrap(),
            b"media"
        );
    }
}
