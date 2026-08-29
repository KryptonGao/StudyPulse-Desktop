//! Versioned ZIP backup export/import with staged validation and recovery.
//!
//! Backup work is intentionally transactional: an archive is extracted into a
//! private staging directory, validated for paths, sizes, checksums, typed
//! records, and relationships, then applied through a separate transaction
//! directory while the Workspace write lock is held. A recovery copy is made
//! before the live `Data` and `Media` trees are swapped.
//!
//! Import is deliberately split into inspection and apply. Inspection may be
//! cancelled and leaves the live tree untouched; apply takes an exclusive write
//! guard, preserves a recovery point, prepares replacement directories, and
//! swaps directory names only after all merge decisions have succeeded. The
//! recovery copy is returned in `ImportReport` so a host can expose it to a
//! repair workflow rather than pretending an import is irreversible.
//!
//! Schema 3 and 4 archives share the required core files. Extra data files are
//! copied and syntactically checked even when this crate has no typed model,
//! which prevents a round trip from becoming an accidental downgrade.
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
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
    CoachDataRow, ComprehensiveExamFull, DiaryEntry, ExamFull, ExamGoal, ExamPlan, ExamSimulation,
    IosRecord, MistakeNoteFull, Result, TaskItem, Workspace, WorkspaceError, decode_coach_payload,
    validate_wire_relative_path,
};

// These limits protect both extraction memory/disk use and archive traversal.
const FORMAT_IDENTIFIER: &str = "com.chenkai.gao.studypulse.backup";
const MAX_ARCHIVE_ENTRIES: usize = 10_000;
const MAX_SINGLE_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

// The manifest and checksums are mandatory, as are the compatibility data
// files. Optional newer files may be copied through without being required.
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
/// Archive metadata used to select a decoder and report its contents before
/// applying anything to the live Workspace.
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
/// Checksum document stored separately so the manifest remains human-readable.
struct Checksums {
    algorithm: String,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// One incoming/local difference identified during inspection.
pub struct BackupConflict {
    pub key: String,
    pub domain: String,
    pub record_id: Option<String>,
    pub display_name: String,
}

#[derive(Debug, Clone)]
/// Validated staged import plus its conflict summary.
/// `staging_path` is private to prevent callers from substituting an arbitrary
/// filesystem directory into `apply_backup`.
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
/// Whether restore replaces the two data trees or merges record-by-record.
pub enum RestoreMode {
    Replace,
    Merge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// User decision for one conflict key produced by inspection.
pub struct BackupResolution {
    pub conflict_key: String,
    pub use_incoming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Result of a successful restore, including the recoverable pre-restore path.
pub struct ImportReport {
    pub imported_records: u64,
    pub kept_local_records: u64,
    pub recovery_path: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Export switches that affect media inclusion and descriptive metadata.
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
    // Media is included by default for a complete local backup.
    true
}

fn default_locale() -> String {
    // Stable fallback for callers that do not provide a UI locale.
    "en_US".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Archive path and manifest returned after export completes.
pub struct BackupExportResult {
    pub archive_path: String,
    pub manifest: BackupManifest,
}

impl Workspace {
    /// Extract and validate an archive without mutating the live Workspace.
    /// The returned inspection owns a private staging directory used by the
    /// later apply/cancel operation.
    pub fn inspect_backup(&self, archive_path: impl AsRef<Path>) -> Result<BackupInspection> {
        let id = Uuid::new_v4().to_string();
        let staging = self.root().join(".studypulse/cache/imports").join(&id);
        fs::create_dir_all(&staging)?;
        // Cleanup is best-effort on validation failure; a failed inspection must
        // never leave untrusted extracted content presented as an active session.
        if let Err(error) = extract_and_validate(archive_path.as_ref(), &staging) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        // Manifest decoding is repeated here after extraction so malformed
        // metadata cannot be hidden by a valid archive container.
        let manifest: BackupManifest =
            serde_json::from_slice(&fs::read(staging.join("manifest.json"))?)?;
        // Check typed values and cross-file links only after archive-level
        // checksums and required files have passed.
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

    /// Apply a previously inspected backup as one Data/Media tree swap.
    /// A recovery copy is created first, and all intermediate work occurs under
    /// `.studypulse/cache` so a failed rename can restore the original trees.
    pub fn apply_backup(
        &self,
        inspection: &BackupInspection,
        mode: RestoreMode,
        resolutions: &[BackupResolution],
    ) -> Result<ImportReport> {
        // The inspection token is also the capability for using its staging
        // directory; a missing directory means the session was cancelled/cleaned.
        if !inspection.staging_path.exists() {
            return Err(WorkspaceError::ImportSessionNotFound);
        }
        if mode == RestoreMode::Merge {
            // Merge decisions must exactly cover the conflicts the inspection
            // reported. A partial or stale list would silently default an
            // undecided conflict to one side, so this fails closed before any
            // recovery copy or staging work happens.
            validate_merge_resolutions(&inspection.conflicts, resolutions)?;
        }
        // Keep reads/upserts from racing the recovery snapshot or the final
        // directory exchange.
        let _guard = self.exclusive_write();
        let operation_id = Uuid::new_v4().to_string();
        let recovery = self
            .root()
            .join(".studypulse/recovery")
            .join(format!("BeforeRestore-{operation_id}"));
        fs::create_dir_all(&recovery)?;
        // Recovery is intentionally separate from the transaction directories:
        // it remains available to a user even after a successful import.
        copy_tree(&self.root().join("Data"), &recovery.join("Data"))?;
        copy_tree(&self.root().join("Media"), &recovery.join("Media"))?;

        let transaction = self
            .root()
            .join(".studypulse/cache")
            .join(format!("apply-{operation_id}"));
        // Keep transaction trees parallel to the live names so the final swap is
        // a pair of directory renames rather than a large file-by-file update.
        let transaction_data = transaction.join("Data");
        let transaction_media = transaction.join("Media");
        fs::create_dir_all(&transaction_data)?;
        fs::create_dir_all(&transaction_media)?;

        // Borrow conflict keys from the caller's resolution list; the map is
        // used only during this synchronous operation.
        let resolution_map: HashMap<&str, bool> = resolutions
            .iter()
            .map(|resolution| (resolution.conflict_key.as_str(), resolution.use_incoming))
            .collect();
        // Build the complete replacement trees before touching live paths.
        let (imported_records, kept_local_records) = match mode {
            RestoreMode::Replace => {
                // Replace ignores local records but still copies the staged
                // archive as a complete tree, including unknown files.
                copy_tree(&inspection.staging_path.join("data"), &transaction_data)?;
                copy_tree(&inspection.staging_path.join("media"), &transaction_media)?;
                (
                    inspection.manifest.record_counts.values().sum::<usize>() as u64,
                    0,
                )
            }
            RestoreMode::Merge => {
                // Merge starts from local content and applies only incoming
                // records selected by the conflict resolution map.
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
            // Media is merged after Data so its conflict keys are evaluated with
            // the same staged transaction lifetime.
            merge_media(
                &inspection.staging_path.join("media"),
                &transaction_media,
                &resolution_map,
            )?;
        }

        // Rename old trees out of the way, then install prepared trees. Each
        // failure branch attempts to put the previous names back before return.
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
            // Restore the old Data name if installing the prepared tree fails.
            fs::rename(&old_data, &data)?;
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&media, &old_media) {
            // Data has been installed but Media has not moved; put Data back
            // before returning the error.
            let _ = fs::rename(&data, &transaction_data);
            let _ = fs::rename(&old_data, &data);
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&transaction_media, &media) {
            // The last rename failure is also rolled back best-effort, including
            // both old trees and the prepared Data directory.
            let _ = fs::rename(&data, &transaction_data);
            let _ = fs::rename(&old_data, &data);
            let _ = fs::rename(&old_media, &media);
            return Err(error.into());
        }
        // Cleanup is not part of the data swap's success condition; the recovery
        // directory is retained while temporary transaction remnants are not.
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

    /// Discard an inspection's staged files without touching live data.
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

    /// Copy current Workspace data into a staging tree, create a manifest and
    /// SHA-256 checksums, then package the tree as a ZIP archive.
    pub fn export_backup(
        &self,
        archive_path: impl AsRef<Path>,
        options: BackupExportOptions,
    ) -> Result<BackupExportResult> {
        // A per-operation staging path keeps concurrent exports from sharing
        // partial manifests or checksum files.
        let operation_id = Uuid::new_v4().to_string();
        let staging = self
            .root()
            .join(".studypulse/cache")
            .join(format!("export-{operation_id}"));
        let data = staging.join("data");
        let media = staging.join("media");
        fs::create_dir_all(&data)?;
        // Copy known and unknown Data files alike so newer schema fields survive
        // a round trip through this older desktop build.
        initialize_export_data(self.root(), &data)?;
        if options.includes_media {
            // Copy media only when requested; the manifest still records missing
            // references so an intentionally data-only export is transparent.
            copy_tree(&self.root().join("Media"), &media)?;
        }

        // Counts are advisory metadata, but they are calculated from staged
        // bytes so the manifest describes exactly what will be archived.
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
                // JSONL count is physical non-empty envelope count, matching
                // import validation rather than decoded array length.
                counts.insert(key.into(), count_nonempty_lines(&path)?);
            } else if name == "subjects.json" {
                // Subjects is the one record-like JSON array domain.
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
        // Compute warnings before writing the manifest so count and warning list
        // describe one immutable staged snapshot.
        let warnings = missing_media_warnings(&data, &media, options.includes_media)?;
        // Timestamps are UTC seconds: the manifest describes an export event,
        // not a record update requiring millisecond ordering.
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
        // The staging tree is private and not a live Workspace file, so direct
        // writes are appropriate here; the final archive is the public output.
        fs::write(
            staging.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;

        // Hash every staged file except that checksums.json is created after the
        // set is collected, avoiding a self-referential checksum entry.
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
        // Archive creation is last: every included file and checksum already
        // exists in staging, and no partial archive is reported as success.
        create_archive(&staging, archive_path.as_ref())?;
        // Return the caller's requested archive path, not the temporary staging path.
        let result = BackupExportResult {
            archive_path: archive_path.as_ref().to_string_lossy().into_owned(),
            manifest,
        };
        // The archive is now self-contained; staging is disposable after success.
        let _ = fs::remove_dir_all(staging);
        Ok(result)
    }
}

fn extract_and_validate(archive_path: &Path, staging: &Path) -> Result<()> {
    // Validate names and limits before writing each entry. The staging root is
    // trusted only after this portable-path check rejects traversal and links.
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
        // Normalize backslashes before validation so Windows-created archives
        // and Unix-created archives share one wire-path policy.
        let raw_name = entry.name().replace('\\', "/");
        let name = raw_name.trim_end_matches('/');
        if name.is_empty() {
            continue;
        }
        // Validate before duplicate tracking and size accounting so unsafe
        // aliases cannot produce confusing conflict/error results.
        validate_wire_relative_path(name)
            .map_err(|_| WorkspaceError::InvalidBackup(format!("unsafe path: {name}")))?;
        if !seen.insert(name.to_string()) {
            return Err(WorkspaceError::InvalidBackup(format!(
                "duplicate archive entry: {name}"
            )));
        }
        // ZIP symlink metadata is rejected even if the link target is not yet
        // present in staging; extraction must never create redirect entries.
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
        // Saturating addition avoids overflow turning a huge archive into a
        // small total-size value.
        total_size = total_size.saturating_add(entry.size());
        if total_size > MAX_TOTAL_BYTES {
            return Err(WorkspaceError::InvalidBackup(
                "archive expands beyond the allowed size".into(),
            ));
        }
        // Joining is safe only because `name` has passed the wire-path policy;
        // directories are created explicitly instead of following archive links.
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

    // Required-file checks catch incomplete archives before any typed parsing.
    for required in REQUIRED_FILES {
        if !staging.join(required).is_file() {
            return Err(WorkspaceError::InvalidBackup(format!(
                "required file is missing: {required}"
            )));
        }
    }
    // Re-read the manifest from the extracted bytes so the archive's declared
    // format, schema, and encryption flags are part of the verified content.
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
    // Checksums are validated for every listed file, including unexpected files
    // copied through for forward compatibility.
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
        // Every required payload must have an integrity entry; checksums.json
        // itself is intentionally excluded from this loop.
        if !checksums.files.contains_key(required) {
            return Err(WorkspaceError::InvalidBackup(format!(
                "checksum is missing: {required}"
            )));
        }
    }
    for (path, expected) in checksums.files {
        // Checksum names are input paths too, so they receive the same portable
        // traversal policy as ZIP entries.
        validate_wire_relative_path(&path)
            .map_err(|_| WorkspaceError::InvalidBackup(format!("unsafe checksum path: {path}")))?;
        let file = staging.join(&path);
        if !file.is_file() {
            // A checksum for a directory or absent file is never meaningful.
            return Err(WorkspaceError::InvalidBackup(format!(
                "checksummed file is missing: {path}"
            )));
        }
        // Hash the extracted file, not the compressed stream, so integrity is
        // checked against the payload that will later be imported.
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
        "exam_goals.jsonl",
        "exam_plans.jsonl",
        "exam_simulations.jsonl",
        "home_ask_sessions.jsonl",
        "study_suggestions.jsonl",
        "daily_ai_plans.jsonl",
        "score_predictions.jsonl",
        "exam_autopsies.jsonl",
    ];
    // Copy every existing data file first. This is intentional: files owned by
    // newer iOS/P1/P2 features must survive a Desktop import/export even when
    // this build has no typed UI for them yet.
    let source_data = root.join("Data");
    if source_data.is_dir() {
        // Copy unknown files too: export is a compatibility boundary, not a
        // typed projection that is allowed to drop newer data.
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
            // Preserve relative subdirectories for any future nested domain.
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
    // Ensure the standard JSONL set exists even when a very old Workspace did
    // not create one of the newer files yet.
    for file in JSONL_FILES {
        let source = root.join("Data").join(file);
        let target = destination.join(file);
        if !source.is_file() && !target.exists() {
            fs::write(target, [])?;
        }
    }
    // Singleton defaults repair only absent/empty legacy placeholders; existing
    // meaningful content is never replaced during export preparation.
    for (file, default_value) in JSON_FILES {
        let source = root.join("Data").join(file);
        let target = destination.join(file);
        // The old `{}` placeholders carry no useful user data and may be
        // replaced with the richer export defaults; non-empty files are kept.
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
    // Health history is optional and intentionally copied without entering the
    // required-file list, preserving compatibility with older backups.
    if root.join("Data/health_history.json").is_file() {
        fs::copy(
            root.join("Data/health_history.json"),
            destination.join("health_history.json"),
        )?;
    }
    Ok(())
}

fn manifest_key_for_file(name: &str) -> Option<&'static str> {
    // Keep manifest vocabulary independent of filenames so the public report
    // remains stable if an internal filename is ever migrated.
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
        "exam_goals.jsonl" => Some("examGoals"),
        "exam_plans.jsonl" => Some("examPlans"),
        "exam_simulations.jsonl" => Some("examSimulations"),
        "home_ask_sessions.jsonl" => Some("homeAskSessions"),
        "study_suggestions.jsonl" => Some("studySuggestions"),
        "daily_ai_plans.jsonl" => Some("dailyAiPlans"),
        "score_predictions.jsonl" => Some("scorePredictions"),
        "exam_autopsies.jsonl" => Some("examAutopsies"),
        "profile.json" => Some("profile"),
        "plant_state.json" => Some("plantState"),
        "achievements.json" => Some("achievements"),
        "coach_data.jsonl" => Some("coachData"),
        "preferences.json" => None,
        "health_history.json" => None,
        _ => None,
    }
}

fn validate_coach_jsonl(path: &Path) -> Result<()> {
    // Coach rows carry typed JSON inside Base64; decode known kinds here while
    // allowing unknown kinds to pass through for forward compatibility.
    for (index, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let row: CoachDataRow =
            serde_json::from_str(&line).map_err(|error| WorkspaceError::MalformedData {
                path: format!("{}:{}", path.display(), index + 1),
                detail: error.to_string(),
            })?;
        // Decode only known Coach kinds; unknown kinds are checked structurally
        // elsewhere and remain valid forward-compatible rows.
        match row.kind.as_str() {
            "goal" => {
                let _: crate::CoachGoal = decode_coach_payload(&row)?;
            }
            "analysis" => {
                let _: crate::CoachAnalysis = decode_coach_payload(&row)?;
            }
            "proposal" => {
                let _: crate::CoachProposal = decode_coach_payload(&row)?;
            }
            "chat" => {
                let _: crate::CoachChat = decode_coach_payload(&row)?;
            }
            "message" => {
                let _: crate::CoachConversationMessage = decode_coach_payload(&row)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn media_stats(root: &Path) -> Result<(usize, u64)> {
    // Media statistics are computed from files in the staged tree, not from
    // references, so manifest counts reflect actual archive payload size.
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
    // References come from known iOS fields; missing media becomes a warning so
    // export remains useful while making loss visible to the user.
    // Collect all known reference fields first, then deduplicate warnings once.
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
        // A media reference is relative to its category, never a free path.
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
    // Archive only regular files and never follow links. ZIP entry names use the
    // same slash-normalized relative form validated during import.
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
        // Normalize archive names even when exporting on Windows.
        let wire = relative.to_string_lossy().replace('\\', "/");
        writer.start_file(wire, SimpleFileOptions::default())?;
        let mut input = File::open(entry.path())?;
        std::io::copy(&mut input, &mut writer)?;
    }
    writer.finish()?;
    Ok(())
}

fn validate_decoded_content(staging: &Path, manifest: &BackupManifest) -> Result<()> {
    // This is the semantic validation phase: syntax/checksum checks have passed,
    // so now enforce RFC3339 dates, typed record invariants, counts, and links.
    // The manifest timestamp is the first semantic date checked because it is
    // needed to explain when the archive was produced.
    chrono::DateTime::parse_from_rfc3339(&manifest.created_at).map_err(|error| {
        WorkspaceError::InvalidBackup(format!("manifest createdAt is invalid: {error}"))
    })?;
    // Typed validators cover domains with strict model rules. Generic validation
    // below still checks every copied JSONL file for syntax and duplicate IDs.
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
    // P2 domains were added after the original required list, so validate them
    // conditionally when present in a schema-3 or schema-4 archive.
    if staging.join("data/exam_goals.jsonl").is_file() {
        validate_typed_jsonl::<ExamGoal>(&staging.join("data/exam_goals.jsonl"), |record| {
            if record.id != record.value.id {
                return Err("envelope id mismatch".into());
            }
            record.value.validate().map_err(|error| error.to_string())
        })?;
    }
    if staging.join("data/exam_plans.jsonl").is_file() {
        validate_typed_jsonl::<ExamPlan>(&staging.join("data/exam_plans.jsonl"), |record| {
            if record.id != record.value.id {
                return Err("envelope id mismatch".into());
            }
            record.value.validate().map_err(|error| error.to_string())
        })?;
    }
    if staging.join("data/exam_simulations.jsonl").is_file() {
        validate_typed_jsonl::<ExamSimulation>(
            &staging.join("data/exam_simulations.jsonl"),
            |record| {
                if record.id != record.value.id {
                    return Err("envelope id mismatch".into());
                }
                record.value.validate().map_err(|error| error.to_string())
            },
        )?;
    }
    if staging.join("data/coach_data.jsonl").is_file() {
        validate_coach_jsonl(&staging.join("data/coach_data.jsonl"))?;
    }
    for file in [
        "home_ask_sessions.jsonl",
        "study_suggestions.jsonl",
        "daily_ai_plans.jsonl",
        "score_predictions.jsonl",
        "exam_autopsies.jsonl",
    ] {
        let path = staging.join("data").join(file);
        if path.is_file() {
            validate_typed_jsonl::<crate::AiFeatureRecord>(&path, |record| {
                if record.id != record.value.id {
                    return Err("envelope id mismatch".into());
                }
                record
                    .value
                    .validate(file)
                    .map_err(|error| error.to_string())
            })?;
        }
    }

    // Unknown files are accepted, but their JSON must remain parseable so a
    // later client can safely consume the exported Workspace.
    for entry in fs::read_dir(staging.join("data"))? {
        let entry = entry?;
        // Generic validation complements typed validation and covers files that
        // this release copied through without a Rust model.
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
    // Counts are checked only when present in the manifest to support older
    // schema versions that did not report every domain.
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
            // Counts catch truncation or accidental extra lines even when the
            // JSON remains syntactically valid and checksums were regenerated.
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
    // Validate every typed envelope while tracking duplicate IDs independently
    // of the domain's model-level validation callback.
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
        // Envelope timestamps are optional for old files, but when present they
        // must still be valid RFC3339 values.
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
    // Build UUID sets once, then verify foreign-key-like links across records.
    // Backup import must reject dangling links before replacing live data.
    let data = staging.join("data");
    // These sets are the local foreign-key vocabulary used by the subsequent
    // relationship checks.
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

    // Phase and exam references are shared by several domains.
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
                // Missing links are rejected before live trees are swapped.
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
    // Exam reviews point back to mistakes and therefore need a second pass.
    for (_, record) in read_jsonl_map(&data.join("exams.jsonl"))? {
        if let Some(ids) = record
            .get("value")
            .and_then(|value| value.get("examReview"))
            .and_then(|review| review.get("linkedMistakeIds"))
            .and_then(Value::as_array)
        {
            for id in ids.iter().filter_map(Value::as_str) {
                // Exam review links are user-visible navigation targets, so a
                // dangling mistake UUID is not safe to import.
                if !mistakes.contains(id) {
                    return Err(WorkspaceError::InvalidBackup(format!(
                        "exam review references missing mistake UUID: {id}"
                    )));
                }
            }
        }
    }
    // Materialized routines and investment targets are checked separately from
    // the generic phase/exam loop because their wire shapes differ.
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
        // Both subject and parent links are optional, but each present UUID must
        // resolve within the same staged archive.
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
    // Unknown JSONL domains do not get a typed validator, but still cannot have
    // malformed JSON or repeated IDs hidden behind a future schema.
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
    // Inspection compares semantic JSONL records by UUID and singleton files by
    // bytes. This gives the UI actionable conflict keys before apply.
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
    // Media has no record envelope, so compare it by relative path and SHA-256.
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
    // Missing media is an addition, same hashes are identical, and differing
    // hashes require an explicit resolution just like a JSONL record conflict.
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

/// Merge apply requires decisions for exactly the conflicts the inspection
/// reported. A missing decision would otherwise default a conflict to one
/// side, a stale key would silently do nothing, and a duplicate entry makes
/// the caller's intent ambiguous — all three fail closed here.
fn validate_merge_resolutions(
    conflicts: &[BackupConflict],
    resolutions: &[BackupResolution],
) -> Result<()> {
    let conflict_keys: HashSet<&str> = conflicts
        .iter()
        .map(|conflict| conflict.key.as_str())
        .collect();
    let mut decided: HashSet<&str> = HashSet::with_capacity(resolutions.len());
    let mut unknown: Vec<&str> = Vec::new();
    for resolution in resolutions {
        if !conflict_keys.contains(resolution.conflict_key.as_str()) {
            unknown.push(resolution.conflict_key.as_str());
        }
        decided.insert(resolution.conflict_key.as_str());
    }
    let missing: Vec<&str> = conflict_keys.difference(&decided).copied().collect();
    let duplicated = decided.len() != resolutions.len();
    if missing.is_empty() && unknown.is_empty() && !duplicated {
        return Ok(());
    }
    let mut details = Vec::new();
    if !missing.is_empty() {
        details.push(format!("no decision for [{}]", summarize_keys(missing)));
    }
    if !unknown.is_empty() {
        details.push(format!(
            "not reported by inspection [{}]",
            summarize_keys(unknown)
        ));
    }
    if duplicated {
        details.push("duplicate entries for the same conflict key".to_string());
    }
    Err(WorkspaceError::ResolutionMismatch(details.join("; ")))
}

/// Deterministic, bounded key listing so a caller mistake stays readable even
/// with hundreds of unresolved conflicts.
fn summarize_keys(mut keys: Vec<&str>) -> String {
    const LIMIT: usize = 5;
    keys.sort_unstable();
    if keys.len() <= LIMIT {
        keys.join(", ")
    } else {
        format!(
            "{}, ... and {} more",
            keys[..LIMIT].join(", "),
            keys.len() - LIMIT
        )
    }
}

fn merge_data(
    incoming: &Path,
    target: &Path,
    resolutions: &HashMap<&str, bool>,
) -> Result<(u64, u64)> {
    // Merge starts from a copied local tree; only selected incoming conflicts
    // replace records, so an error before the final swap leaves live data intact.
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
        // JSONL is merged by record key; singleton JSON is resolved as one
        // conflict because there is no safe domain-specific merge contract.
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
                // Resolution keys include the domain to avoid collisions between
                // identical UUID strings in different files.
                let conflict_key = format!("{domain}:{key}");
                // An explicit decision keeps or replaces regardless of content.
                // Without one, only a record that really differs locally needs
                // a decision; a conflict that appeared after inspection (local
                // data changed between inspect and apply) defaults to keeping
                // the local record instead of silently losing it.
                let keep_local = match resolutions.get(conflict_key.as_str()) {
                    Some(use_incoming) => !*use_incoming && local.contains_key(&key),
                    None => match local.get(&key) {
                        Some(existing) => existing != &value,
                        None => false,
                    },
                };
                if keep_local {
                    kept += 1;
                } else {
                    local.insert(key, value);
                    imported += 1;
                }
            }
            // The destination is still inside the transaction tree, so a
            // partially written merge cannot damage the live Workspace.
            write_jsonl_map(&destination, local)?;
        } else {
            let conflict_key = format!("{domain}:singleton");
            // Identical bytes need no decision; differing bytes with no
            // decision are an after-inspection conflict and keep the local
            // file rather than silently overwriting it.
            let keep_local = match resolutions.get(conflict_key.as_str()) {
                Some(use_incoming) => !*use_incoming && destination.exists(),
                None => destination.exists() && fs::read(&source)? != fs::read(&destination)?,
            };
            if keep_local {
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
    // Media conflicts use `media:<relative path>` keys; the local file stays in
    // place when the caller declined incoming bytes, or when local bytes
    // changed after inspection and no decision covers them.
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
        // Hash comparison keeps identical media copying (no decision needed)
        // while a post-inspection conflict defaults to keeping local bytes.
        let keep_local = match resolutions.get(conflict_key.as_str()) {
            Some(use_incoming) => !*use_incoming && destination.exists(),
            None => {
                destination.exists() && sha256_file(entry.path())? != sha256_file(&destination)?
            }
        };
        if keep_local {
            continue;
        }
        if let Some(parent) = destination.parent() {
            // Preserve the incoming category hierarchy under transaction Media.
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), destination)?;
    }
    Ok(())
}

fn read_jsonl_map(path: &Path) -> Result<HashMap<String, Value>> {
    // Index JSONL by a stable record key for conflict comparison and merge.
    // Raw Values are retained so unknown fields are not normalized away.
    let file = File::open(path)?;
    let mut values = HashMap::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        // Syntax errors retain the physical file/line for repair tooling.
        let value: Value =
            serde_json::from_str(&line).map_err(|error| WorkspaceError::MalformedData {
                path: format!("{}:{}", path.display(), index + 1),
                detail: error.to_string(),
            })?;
        // Legacy/generic rows without an ID use their content hash as a stable
        // comparison key rather than being silently discarded.
        let key = record_id(&value).unwrap_or_else(|| sha256_bytes(line.as_bytes()));
        // `insert` is intentional here: semantic duplicate detection happens in
        // typed validation, while generic comparison only needs a last value for
        // legacy rows that have no stable ID.
        values.insert(key, value);
    }
    Ok(values)
}

fn read_jsonl_map_if_exists(path: &Path) -> Result<HashMap<String, Value>> {
    // Optional newer domains may not exist in schema-3 archives.
    // Returning an empty map avoids manufacturing an error for optional files.
    if path.exists() {
        read_jsonl_map(path)
    } else {
        Ok(HashMap::new())
    }
}

fn write_jsonl_map(path: &Path, values: HashMap<String, Value>) -> Result<()> {
    // Sort by key so merged archives are deterministic even when HashMap input
    // order differs between runs.
    let mut values: Vec<_> = values.into_iter().collect();
    values.sort_by(|left, right| left.0.cmp(&right.0));
    let mut file = File::create(path)?;
    for (_, value) in values {
        // Keep one physical JSON object per line so the result remains valid
        // input for the generic JSONL reader.
        serde_json::to_writer(&mut file, &value)?;
        file.write_all(b"\n")?;
    }
    file.flush()?;
    Ok(())
}

fn record_id(value: &Value) -> Option<String> {
    // Typed rows put IDs at the envelope/value level; Coach rows encode them in
    // a Base64 payload, so inspect that form before the generic fallbacks.
    if let (Some(kind), Some(payload)) = (
        value.get("kind").and_then(Value::as_str),
        value.get("payload").and_then(Value::as_str),
    ) && let Ok(bytes) = BASE64.decode(payload)
        && let Ok(decoded) = serde_json::from_slice::<Value>(&bytes)
        && let Some(id) = decoded.get("id").and_then(Value::as_str)
    {
        return Some(format!("coach:{kind}:{id}"));
    }
    // Standard records expose either the envelope ID or the nested value ID.
    value
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| value.get("value")?.get("id")?.as_str())
        .map(str::to_owned)
}

fn display_name(value: &Value, fallback: &str) -> String {
    // Conflict UI gets a human-readable label when possible; the stable key is
    // still used as a fallback when the incoming row has no title/name.
    ["title", "name", "subject"]
        .into_iter()
        .find_map(|key| {
            // Prefer nested `value` fields because JSONL envelopes put the
            // display data there; singleton objects use their own top-level key.
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
    // Blank separators do not represent records and are excluded from counts.
    Ok(BufReader::new(File::open(path)?)
        .lines()
        .map_while(std::result::Result::ok)
        .filter(|line| !line.trim().is_empty())
        .count())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    // Copy regular files/directories without following links. Recovery and
    // transaction callers install the result only after the copy succeeds.
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
        // `relative` comes from a WalkDir rooted at `source`; only normal
        // directory/file entries are materialized below the destination.
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
    // Stream hashing in bounded chunks so large media files do not require a
    // full in-memory buffer during archive validation.
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    // A fixed buffer bounds memory regardless of the media file size.
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        // Feed only the bytes read; the remainder of the buffer is stale data.
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    // Small JSON rows can be hashed directly for generic merge keys.
    // Hex output is stable in conflict keys and manifests.
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zip::{ZipWriter, write::SimpleFileOptions};

    // Build small deterministic archives for schema/restore tests without
    // relying on a fixture that could hide which required files are present.
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
    // Archive traversal, duplicate, and unsafe path defenses must fail before
    // any import session becomes usable.
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
    // Both supported backup schemas use the same extraction/checksum contract,
    // even when one predates newer typed domains.
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
    // Replace restore preserves future/unknown fields and exposes a recovery
    // directory for post-restore repair or inspection.
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
    // A Desktop export can be inspected and restored as schema 4 without losing
    // data that this version does not have a typed UI for.
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

    fn task_with(id: Uuid, title: &str) -> TaskItem {
        let timestamp = "2026-07-31T12:00:00Z".to_string();
        TaskItem {
            id,
            title: title.into(),
            task_type: crate::TaskType::Homework,
            due_date: timestamp.clone(),
            reminder_date: timestamp.clone(),
            subject: "Math".into(),
            importance: 3,
            notes: String::new(),
            is_completed: false,
            reminder_event_id: None,
            reminder_calendar_id: None,
            created_at: timestamp,
            phase_id: None,
            coach_execution_data: None,
            coach_goal_id: None,
            coach_proposal_id: None,
            extra: BTreeMap::new(),
        }
    }

    fn export_archive(workspace: &Workspace, directory: &Path, name: &str) -> PathBuf {
        let exported = workspace
            .export_backup(
                directory.join(name),
                BackupExportOptions {
                    includes_media: true,
                    includes_derived_health_data: false,
                    app_version: "test".into(),
                    app_build: "test".into(),
                    locale: "en_US".into(),
                },
            )
            .unwrap();
        PathBuf::from(exported.archive_path)
    }

    // Identical singleton JSON files are copied and counted as imported, so
    // record-level count assertions must add them on top of JSONL records.
    fn staged_singleton_count(inspection: &BackupInspection) -> u64 {
        fs::read_dir(inspection.staging_path.join("data"))
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension != "jsonl")
            })
            .count() as u64
    }

    // One decision per inspection conflict; `accept` picks the incoming side.
    fn resolutions_for(
        inspection: &BackupInspection,
        accept: impl Fn(&str) -> bool,
    ) -> Vec<BackupResolution> {
        inspection
            .conflicts
            .iter()
            .map(|conflict| BackupResolution {
                conflict_key: conflict.key.clone(),
                use_incoming: accept(&conflict.key),
            })
            .collect()
    }

    #[test]
    // Merge honors per-conflict decisions: declined records keep local
    // content, accepted records take incoming bytes, and records inspection
    // reported as added are imported without needing a decision.
    fn merge_applies_explicit_resolutions_record_by_record() {
        let temp = tempfile::tempdir().unwrap();
        let source = Workspace::create(temp.path().join("Source")).unwrap();
        let conflicting_id = Uuid::new_v4();
        let added_id = Uuid::new_v4();
        source
            .append_task(task_with(conflicting_id, "Incoming title"))
            .unwrap();
        source
            .append_task(task_with(added_id, "Backup only"))
            .unwrap();
        source
            .write_media("images/shared.bin", b"incoming")
            .unwrap();
        source.write_media("images/new.bin", b"new").unwrap();
        let archive = export_archive(&source, temp.path(), "merge.studypulsebackup");

        let destination = Workspace::create(temp.path().join("Destination")).unwrap();
        destination
            .append_task(task_with(conflicting_id, "Local title"))
            .unwrap();
        destination
            .write_media("images/shared.bin", b"local")
            .unwrap();

        let inspection = destination.inspect_backup(archive).unwrap();
        // Fresh workspaces also report dynamically seeded singleton JSON as
        // conflicting; only the task and media keys matter for this test.
        for expected in [
            format!("tasks:{conflicting_id}"),
            "media:images/shared.bin".into(),
        ] {
            assert!(
                inspection
                    .conflicts
                    .iter()
                    .any(|conflict| conflict.key == expected)
            );
        }

        // Decline the task conflict and accept every other one. The singleton
        // count is read before apply because a successful apply clears the
        // inspection staging directory.
        let singleton_count = staged_singleton_count(&inspection);
        let report = destination
            .apply_backup(
                &inspection,
                RestoreMode::Merge,
                &resolutions_for(&inspection, |key| !key.starts_with("tasks:")),
            )
            .unwrap();

        assert_eq!(report.imported_records, 1 + singleton_count);
        assert_eq!(report.kept_local_records, 1);
        let tasks = destination.read_tasks().unwrap();
        assert_eq!(
            tasks
                .iter()
                .find(|task| task.id == conflicting_id)
                .unwrap()
                .title,
            "Local title"
        );
        assert!(tasks.iter().any(|task| task.id == added_id));
        assert_eq!(
            destination.read_media("images/shared.bin").unwrap(),
            b"incoming"
        );
        assert_eq!(destination.read_media("images/new.bin").unwrap(), b"new");
    }

    #[test]
    // A merge with any inspection conflict left undecided fails closed before
    // any recovery copy or swap happens, leaving local data exactly as it was.
    fn merge_fails_closed_when_a_conflict_has_no_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let source = Workspace::create(temp.path().join("Source")).unwrap();
        let conflicting_id = Uuid::new_v4();
        let added_id = Uuid::new_v4();
        source
            .append_task(task_with(conflicting_id, "Incoming title"))
            .unwrap();
        source
            .append_task(task_with(added_id, "Backup only"))
            .unwrap();
        source
            .write_media("images/shared.bin", b"incoming")
            .unwrap();
        let archive = export_archive(&source, temp.path(), "merge.studypulsebackup");

        let destination = Workspace::create(temp.path().join("Destination")).unwrap();
        destination
            .append_task(task_with(conflicting_id, "Local title"))
            .unwrap();
        destination
            .write_media("images/shared.bin", b"local")
            .unwrap();

        let inspection = destination.inspect_backup(archive).unwrap();
        let error = destination
            .apply_backup(
                &inspection,
                RestoreMode::Merge,
                &[BackupResolution {
                    conflict_key: "media:images/shared.bin".into(),
                    use_incoming: true,
                }],
            )
            .unwrap_err();
        assert!(error.to_string().contains("no decision for"));
        assert!(
            error
                .to_string()
                .contains(&format!("tasks:{conflicting_id}"))
        );

        // A rejected apply leaves no recovery snapshot behind.
        assert!(
            fs::read_dir(destination.root().join(".studypulse/recovery"))
                .unwrap()
                .next()
                .is_none()
        );
        let tasks = destination.read_tasks().unwrap();
        assert_eq!(
            tasks
                .iter()
                .find(|task| task.id == conflicting_id)
                .unwrap()
                .title,
            "Local title"
        );
        assert!(!tasks.iter().any(|task| task.id == added_id));
        assert_eq!(
            destination.read_media("images/shared.bin").unwrap(),
            b"local"
        );
    }

    #[test]
    // Resolution keys the inspection never reported — and duplicated entries —
    // are caller bugs rather than decisions, so merge refuses them instead of
    // silently ignoring them.
    fn merge_rejects_unknown_and_duplicate_resolution_keys() {
        let temp = tempfile::tempdir().unwrap();
        let source = Workspace::create(temp.path().join("Source")).unwrap();
        let conflicting_id = Uuid::new_v4();
        source
            .append_task(task_with(conflicting_id, "Incoming title"))
            .unwrap();
        source
            .write_media("images/shared.bin", b"incoming")
            .unwrap();
        let archive = export_archive(&source, temp.path(), "merge.studypulsebackup");

        let destination = Workspace::create(temp.path().join("Destination")).unwrap();
        destination
            .append_task(task_with(conflicting_id, "Local title"))
            .unwrap();
        destination
            .write_media("images/shared.bin", b"local")
            .unwrap();
        let inspection = destination.inspect_backup(archive).unwrap();

        let mut resolutions = resolutions_for(&inspection, |_| true);
        resolutions.push(BackupResolution {
            conflict_key: "tasks:bogus-key".into(),
            use_incoming: false,
        });
        let error = destination
            .apply_backup(&inspection, RestoreMode::Merge, &resolutions)
            .unwrap_err();
        assert!(error.to_string().contains("not reported by inspection"));
        assert!(error.to_string().contains("tasks:bogus-key"));

        let mut duplicated = resolutions_for(&inspection, |_| true);
        duplicated.push(BackupResolution {
            conflict_key: format!("tasks:{conflicting_id}"),
            use_incoming: false,
        });
        let error = destination
            .apply_backup(&inspection, RestoreMode::Merge, &duplicated)
            .unwrap_err();
        assert!(error.to_string().contains("duplicate entries"));
    }

    #[test]
    // Data that changes locally between inspect and apply creates conflicts
    // the resolution list cannot know about; merge keeps those local records
    // instead of silently overwriting them with stale incoming content.
    fn merge_keeps_local_records_that_conflict_after_inspection() {
        let temp = tempfile::tempdir().unwrap();
        let drifting_id = Uuid::new_v4();
        let added_id = Uuid::new_v4();

        // The destination writes the shared record first and the source's
        // JSONL is seeded from it: record envelopes carry write-time update
        // timestamps, so this is the only way both hold byte-identical bytes
        // and inspection reports no conflict for the record.
        let destination = Workspace::create(temp.path().join("Destination")).unwrap();
        destination
            .append_task(task_with(drifting_id, "Same at inspection"))
            .unwrap();
        destination
            .write_media("images/drift.bin", b"same")
            .unwrap();

        let source = Workspace::create(temp.path().join("Source")).unwrap();
        fs::copy(
            destination.root().join("Data/tasks.jsonl"),
            source.root().join("Data/tasks.jsonl"),
        )
        .unwrap();
        source
            .append_task(task_with(added_id, "Backup only"))
            .unwrap();
        source.write_media("images/drift.bin", b"same").unwrap();
        let archive = export_archive(&source, temp.path(), "drift.studypulsebackup");

        let inspection = destination.inspect_backup(archive).unwrap();
        // Only dynamically seeded singleton JSON conflicts; the drifted task
        // and media files were identical at inspection time.
        assert!(
            inspection
                .conflicts
                .iter()
                .all(|conflict| !conflict.key.starts_with("tasks:")
                    && !conflict.key.starts_with("media:"))
        );

        destination
            .upsert_task(task_with(drifting_id, "Edited locally"))
            .unwrap();
        destination
            .write_media("images/drift.bin", b"edited")
            .unwrap();

        let singleton_count = staged_singleton_count(&inspection);
        let report = destination
            .apply_backup(
                &inspection,
                RestoreMode::Merge,
                &resolutions_for(&inspection, |_| true),
            )
            .unwrap();
        assert_eq!(report.imported_records, 1 + singleton_count);
        assert_eq!(report.kept_local_records, 1);
        assert_eq!(
            destination
                .read_tasks()
                .unwrap()
                .iter()
                .find(|task| task.id == drifting_id)
                .unwrap()
                .title,
            "Edited locally"
        );
        assert!(
            destination
                .read_tasks()
                .unwrap()
                .iter()
                .any(|task| task.id == added_id)
        );
        assert_eq!(
            destination.read_media("images/drift.bin").unwrap(),
            b"edited"
        );
    }
}
