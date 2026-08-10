//! Filesystem-backed Workspace implementation.
//!
//! This module is the only layer that knows the directory layout and record
//! files.  It keeps writes serialized with one mutex and replaces files through
//! `atomic_write`, while reads validate JSONL envelopes before returning values.
//! All externally supplied paths are parsed as safe relative paths and checked
//! against symlink/canonical-root escapes before they reach the filesystem.
//!
//! The public CRUD methods intentionally stay thin. Domain-specific validation
//! lives beside each model, while JSONL identity, envelope-extra preservation,
//! timestamps, locking, and atomic replacement live in the helpers near the end
//! of this file. Keeping those concerns centralized prevents one newly added
//! record type from accidentally bypassing the storage contract.
//!
//! Backup code uses `exclusive_write` to extend the same process-local lock over
//! a multi-file transaction. Filesystem recovery is still handled by backup's
//! staging/recovery directories; this module supplies the safe primitives.
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::{
    AgentNotebook, CoachAnalysis, CoachChat, CoachConversationMessage, CoachData, CoachDataRow,
    CoachGoal, CoachProposal, ComprehensiveExamFull, DiaryEntry, ExamFull, ExamGoal, ExamPlan,
    ExamSimulation, FileEntry, GoalReward, Grade, IosRecord, MistakeNoteFull, Result, Routine,
    RoutineInstance, SafeRelativePath, SearchMatch, StudyPhase, StudySession, SubTask, Subject,
    TaskItem, TimeInvestmentSubject, WorkspaceError, WorkspaceInfo, decode_coach_payload,
    learning_report, make_coach_row, platform::is_link_like,
    safe_path::ensure_no_symlink_components,
};

// The metadata version gates future formats; opening a newer schema is safer
// than silently interpreting its fields with an older reader.
const WORKSPACE_SCHEMA_VERSION: u32 = 1;
// Search and import share the 1 MiB text bound to keep memory and scan time
// predictable for both the UI and Agent tools.
const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;
// A result cap prevents a broad query from turning the local library into an
// unbounded response payload.
const MAX_SEARCH_RESULTS: usize = 50;
// Media is kept separate from text because binary assets need a larger bound.
const MAX_MEDIA_FILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
/// On-disk identity and schema marker stored under `.studypulse`.
struct WorkspaceMetadata {
    format_identifier: String,
    id: Uuid,
    schema_version: u32,
}

#[derive(Debug)]
/// Shared immutable root/info plus the process-local write serialization lock.
/// The lock is intentionally held around read-modify-write sequences as well
/// as the final atomic rename so concurrent callers cannot lose an update.
struct WorkspaceInner {
    root: PathBuf,
    info: WorkspaceInfo,
    write_lock: Mutex<()>,
}

#[derive(Debug, Clone)]
/// Handle to one canonical local Workspace.
pub struct Workspace {
    inner: Arc<WorkspaceInner>,
}

impl Workspace {
    /// Create the directory skeleton and compatibility files for a Workspace.
    /// Existing files are left intact, allowing an older Workspace to be
    /// opened without destructive migration; new singleton files use the same
    /// atomic replacement helper as later writes.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref();
        fs::create_dir_all(root)?;
        // Keep the layout explicit so backups, FFI callers, and iOS-compatible
        // file names do not depend on implicit directory creation order.
        for directory in [
            "Documents",
            "Notes",
            "Data",
            "Media/images",
            "Media/audio",
            "Agent/runs",
            "Agent/artifacts",
            "Agent/memory",
            "Agent/notebooks",
            ".studypulse/cache/imports",
            ".studypulse/recovery",
            ".studypulse/index",
        ] {
            fs::create_dir_all(root.join(directory))?;
        }

        // JSONL files are created empty because their record envelope is added
        // only when the first typed value is upserted.
        for file in [
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
        ] {
            let path = root.join("Data").join(file);
            if !path.exists() {
                OpenOptions::new().create_new(true).write(true).open(path)?;
            }
        }
        // Singleton JSON files are valid immediately and therefore receive
        // their minimal JSON value through `atomic_write`.
        for (file, value) in [
            ("subjects.json", "[]"),
            ("profile.json", "{}"),
            ("plant_state.json", "{}"),
            ("achievements.json", "{}"),
            ("preferences.json", "{}"),
        ] {
            let path = root.join("Data").join(file);
            if !path.exists() {
                atomic_write(&path, value.as_bytes())?;
            }
        }

        // Metadata is the compatibility gate. If it already exists, preserve
        // its UUID/schema instead of regenerating identity on every open.
        let metadata_path = root.join(".studypulse/workspace.json");
        let metadata = if metadata_path.exists() {
            serde_json::from_slice::<WorkspaceMetadata>(&fs::read(&metadata_path)?)?
        } else {
            let metadata = WorkspaceMetadata {
                format_identifier: "com.chenkai.gao.studypulse.workspace".into(),
                id: Uuid::new_v4(),
                schema_version: WORKSPACE_SCHEMA_VERSION,
            };
            atomic_write(&metadata_path, &serde_json::to_vec_pretty(&metadata)?)?;
            metadata
        };
        Self::from_metadata(root, metadata)
    }

    /// Open an existing Workspace by reading metadata only; opening does not
    /// recreate missing data files or silently upgrade its schema.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref();
        let metadata_path = root.join(".studypulse/workspace.json");
        let metadata: WorkspaceMetadata = serde_json::from_slice(
            &fs::read(metadata_path).map_err(|_| WorkspaceError::InvalidWorkspace)?,
        )
        .map_err(|_| WorkspaceError::InvalidWorkspace)?;
        Self::from_metadata(root, metadata)
    }

    fn from_metadata(root: &Path, metadata: WorkspaceMetadata) -> Result<Self> {
        // Reject another product's directory and every future schema before
        // canonicalizing or exposing a handle to its contents.
        if metadata.format_identifier != "com.chenkai.gao.studypulse.workspace"
            || metadata.schema_version > WORKSPACE_SCHEMA_VERSION
        {
            return Err(WorkspaceError::InvalidWorkspace);
        }
        let canonical_root = root.canonicalize()?;
        let info = WorkspaceInfo {
            id: metadata.id.to_string(),
            root_path: canonical_root.to_string_lossy().into_owned(),
            schema_version: metadata.schema_version,
        };
        Ok(Self {
            inner: Arc::new(WorkspaceInner {
                root: canonical_root,
                info,
                write_lock: Mutex::new(()),
            }),
        })
    }

    /// Return a copy of the stable Workspace identity exposed to callers.
    pub fn info(&self) -> WorkspaceInfo {
        // Clone the small value object so callers cannot mutate shared state.
        self.inner.info.clone()
    }

    /// Return the canonical root used for all internal joins.
    pub fn root(&self) -> &Path {
        // Guarded helpers reuse this canonical path instead of re-resolving it.
        &self.inner.root
    }

    /// Resolve an existing wire path beneath the canonical root.
    /// Validation, per-component link inspection, canonicalization, and the
    /// final containment check are deliberately redundant security layers.
    pub fn resolve_existing(&self, relative: &str) -> Result<PathBuf> {
        let relative = SafeRelativePath::parse(relative)?;
        ensure_no_symlink_components(self.root(), relative.as_path())?;
        let path = self.root().join(relative.as_path());
        let canonical = path.canonicalize()?;
        if !canonical.starts_with(self.root()) {
            return Err(WorkspaceError::PathEscape(
                relative.as_path().to_string_lossy().into_owned(),
            ));
        }
        Ok(canonical)
    }

    /// Enumerate visible text files under the two user-editable library roots.
    /// Hidden entries, links, and binary files are excluded before paths are
    /// returned to the frontend or Agent.
    pub fn list_library_files(&self) -> Result<Vec<FileEntry>> {
        let mut entries = Vec::new();
        for top_level in ["Documents", "Notes"] {
            // Resolve the top-level directory through the same guard used for
            // individual files; WalkDir is never allowed to follow links.
            let base = self.resolve_existing(top_level)?;
            for entry in WalkDir::new(&base)
                .follow_links(false)
                .min_depth(1)
                .into_iter()
                .filter_entry(|entry| !is_hidden_or_link(entry))
                .filter_map(std::result::Result::ok)
            {
                let metadata = fs::symlink_metadata(entry.path())?;
                if is_link_like(&metadata) {
                    continue;
                }
                if metadata.is_file() && !is_probably_text(entry.path())? {
                    continue;
                }
                let relative = entry
                    .path()
                    .strip_prefix(self.root())
                    .map_err(|_| WorkspaceError::PathEscape(entry.path().display().to_string()))?;
                entries.push(FileEntry {
                    relative_path: wire_path(relative),
                    is_directory: metadata.is_dir(),
                    size_bytes: if metadata.is_file() {
                        metadata.len()
                    } else {
                        0
                    },
                    modified_at: metadata.modified().ok().map(system_time_string),
                });
            }
        }
        entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(entries)
    }

    /// Filter the complete library to a Notebook's selected source allow-list.
    /// Missing, directory, or duplicate selections are handled deterministically
    /// so a Notebook cannot authorize an arbitrary path at read time.
    pub fn list_selected_library_files(&self, source_paths: &[String]) -> Result<Vec<FileEntry>> {
        let available = self.list_library_files()?;
        // Start from the already filtered library rather than resolving caller
        // strings independently, so directories and links cannot be selected.
        let mut selected = Vec::with_capacity(source_paths.len());
        for source_path in source_paths {
            let Some(entry) = available
                .iter()
                .find(|entry| !entry.is_directory && entry.relative_path == *source_path)
            else {
                return Err(WorkspaceError::InvalidPath(source_path.clone()));
            };
            if !selected
                .iter()
                .any(|selected: &FileEntry| selected.relative_path == entry.relative_path)
            {
                selected.push(entry.clone());
            }
        }
        selected.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(selected)
    }

    /// Search all visible library entries by path and first matching text line.
    pub fn search_library(&self, query: &str) -> Result<Vec<SearchMatch>> {
        let entries = self.list_library_files()?;
        self.search_library_entries(query, entries)
    }

    /// Search only sources previously validated against the Notebook selection.
    pub fn search_selected_library(
        &self,
        query: &str,
        source_paths: &[String],
    ) -> Result<Vec<SearchMatch>> {
        let entries = self.list_selected_library_files(source_paths)?;
        self.search_library_entries(query, entries)
    }

    /// Read a selected text source with a hard output bound.  The caller is
    /// responsible for checking that the path belongs to the active Notebook;
    /// this method still applies the Workspace path and symlink guards.
    pub fn read_library_source(&self, relative: &str, max_chars: usize) -> Result<String> {
        // Resolve before reading so even read-only Agent access cannot cross a
        // Workspace boundary through a link or traversal segment.
        let path = self.resolve_existing(relative)?;
        let bytes = fs::read(path)?;
        if bytes.contains(&0) {
            return Err(WorkspaceError::MalformedData {
                path: relative.into(),
                detail: "source is not UTF-8 text".into(),
            });
        }
        let text = String::from_utf8(bytes).map_err(|_| WorkspaceError::MalformedData {
            path: relative.into(),
            detail: "source is not UTF-8 text".into(),
        })?;
        // Character truncation, rather than byte slicing, keeps UTF-8 output
        // valid for the Markdown renderer and FFI serialization.
        Ok(text.chars().take(max_chars).collect())
    }

    /// Persist a generated Agent artifact below the Workspace root.
    /// The restricted names and 10 MiB cap are part of the artifact contract,
    /// not merely UI validation, and the write uses the Workspace lock/rename.
    pub fn write_agent_artifact(
        &self,
        run_id: &str,
        artifact_id: &str,
        extension: &str,
        contents: &[u8],
    ) -> Result<String> {
        if run_id.is_empty()
            || artifact_id.is_empty()
            || extension.is_empty()
            || !run_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            || !artifact_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
            || !extension.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err(WorkspaceError::InvalidPath(
                "invalid Agent artifact name".into(),
            ));
        }
        if contents.len() > 10 * 1024 * 1024 {
            return Err(WorkspaceError::InvalidPath(
                "Agent artifact exceeds the 10 MiB limit".into(),
            ));
        }
        // Build the final relative path only after each user-controlled token
        // has passed its narrower character policy.
        let relative = format!("Agent/artifacts/{run_id}/{artifact_id}.{extension}");
        SafeRelativePath::parse(&relative)?;
        let path = self.root().join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Hold the same lock as record writes so artifact replacement cannot
        // race a backup or another Agent write in this process.
        let _guard = self.inner.write_lock.lock();
        atomic_write(&path, contents)?;
        Ok(relative)
    }

    /// Read workspace- or notebook-scoped Agent memory, returning an empty JSON
    /// object for a valid scope that has not been initialized yet.
    pub fn read_agent_memory(&self, scope: &str) -> Result<serde_json::Value> {
        // Scope is converted into a fixed relative layout only after the
        // whitelist check; arbitrary nested paths never reach `join`.
        let safe = match scope {
            "workspace" => "Agent/memory/workspace.json".to_owned(),
            value
                if !value.is_empty()
                    && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') =>
            {
                format!("Agent/notebooks/{value}/memory.json")
            }
            _ => {
                return Err(WorkspaceError::InvalidPath(
                    "invalid Agent memory scope".into(),
                ));
            }
        };
        let path = self.root().join(safe);
        if !path.exists() {
            return Ok(serde_json::json!({}));
        }
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }

    /// Persist Agent memory using the same scope whitelist and atomic write
    /// protocol as all other Workspace-owned JSON.
    pub fn write_agent_memory(&self, scope: &str, value: &serde_json::Value) -> Result<()> {
        let relative = match scope {
            "workspace" => "Agent/memory/workspace.json".to_owned(),
            scope
                if !scope.is_empty()
                    && scope.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') =>
            {
                format!("Agent/notebooks/{scope}/memory.json")
            }
            _ => {
                return Err(WorkspaceError::InvalidPath(
                    "invalid Agent memory scope".into(),
                ));
            }
        };
        // Create only the known parent directory, then serialize and replace
        // the file while holding the process-local write lock.
        let path = self.root().join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Pretty output keeps memory inspectable while the atomic helper keeps
        // partial JSON from being published after a crash.
        let bytes = serde_json::to_vec_pretty(value)?;
        let _guard = self.inner.write_lock.lock();
        atomic_write(&path, &bytes)
    }

    /// Import one UTF-8 text source into `Documents` without overwriting an
    /// existing file.  The name is a leaf only; callers cannot choose a path.
    pub fn import_library_source(&self, file_name: &str, contents: Vec<u8>) -> Result<FileEntry> {
        if file_name.is_empty()
            || file_name.starts_with('.')
            || file_name.contains(['/', '\\'])
            || Path::new(file_name)
                .file_name()
                .and_then(|value| value.to_str())
                != Some(file_name)
        {
            return Err(WorkspaceError::InvalidPath(file_name.into()));
        }
        if contents.len() as u64 > MAX_SEARCH_FILE_BYTES {
            return Err(WorkspaceError::InvalidPath(format!(
                "{file_name} exceeds the 1 MiB source limit"
            )));
        }
        if contents.contains(&0) || std::str::from_utf8(&contents).is_err() {
            return Err(WorkspaceError::InvalidPath(format!(
                "{file_name} is not a UTF-8 text file"
            )));
        }

        // Resolve the collision before acquiring the write lock; the lock below
        // serializes the actual replacement but preserves the established API.
        let source_name = unique_source_name(self.root(), file_name)?;
        let relative_path = format!("Documents/{source_name}");
        SafeRelativePath::parse(&relative_path)?;
        let target = self.root().join(&relative_path);
        let _guard = self.inner.write_lock.lock();
        atomic_write(&target, &contents)?;
        let metadata = fs::metadata(&target)?;
        Ok(FileEntry {
            relative_path,
            is_directory: false,
            size_bytes: metadata.len(),
            modified_at: metadata.modified().ok().map(system_time_string),
        })
    }

    fn search_library_entries(
        &self,
        query: &str,
        entries: Vec<FileEntry>,
    ) -> Result<Vec<SearchMatch>> {
        // Normalize once, then stop at the shared cap. A path match is emitted
        // before scanning content so a large/binary file can still be found by
        // its safe relative name without being opened as text.
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let mut matches = Vec::new();
        for entry in entries {
            if matches.len() >= MAX_SEARCH_RESULTS {
                break;
            }
            if entry.is_directory {
                continue;
            }
            if entry.relative_path.to_lowercase().contains(&needle) {
                matches.push(SearchMatch {
                    relative_path: entry.relative_path.clone(),
                    line_number: None,
                    snippet: entry.relative_path.clone(),
                });
            }
            if matches.len() >= MAX_SEARCH_RESULTS || entry.size_bytes > MAX_SEARCH_FILE_BYTES {
                continue;
            }
            let path = self.resolve_existing(&entry.relative_path)?;
            let file = fs::File::open(path)?;
            // Stop after the first matching line per file to keep results useful
            // and bounded even when a source repeats the query many times.
            for (index, line) in BufReader::new(file).lines().enumerate() {
                let Ok(line) = line else { break };
                if line.to_lowercase().contains(&needle) {
                    matches.push(SearchMatch {
                        relative_path: entry.relative_path.clone(),
                        line_number: Some((index + 1) as u32),
                        snippet: line.chars().take(240).collect(),
                    });
                    break;
                }
                if matches.len() >= MAX_SEARCH_RESULTS {
                    break;
                }
            }
        }
        matches.truncate(MAX_SEARCH_RESULTS);
        Ok(matches)
    }

    /// Read task envelopes explicitly because tasks have an additional
    /// validation path used by the host's completion-toggle command.
    pub fn read_tasks(&self) -> Result<Vec<TaskItem>> {
        let path = self.root().join("Data/tasks.jsonl");
        let file = fs::File::open(&path)?;
        let mut tasks = Vec::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // Keep the physical JSONL line in errors; this is the fastest way
            // to repair a malformed local file without losing other records.
            let record: IosRecord<TaskItem> =
                serde_json::from_str(&line).map_err(|error| WorkspaceError::MalformedData {
                    path: format!("Data/tasks.jsonl:{}", index + 1),
                    detail: error.to_string(),
                })?;
            if record.id != record.value.id {
                return Err(WorkspaceError::MalformedData {
                    path: format!("Data/tasks.jsonl:{}", index + 1),
                    detail: "envelope id does not match value id".into(),
                });
            }
            record.value.validate()?;
            tasks.push(record.value);
        }
        Ok(tasks)
    }

    /// Historical append API; JSONL upsert provides the same behavior while
    /// preventing duplicate UUIDs when a caller retries the operation.
    pub fn append_task(&self, task: TaskItem) -> Result<()> {
        self.upsert_task(task)
    }

    /// Validate a task before passing it through the shared envelope writer.
    pub fn upsert_task(&self, task: TaskItem) -> Result<()> {
        task.validate()?;
        self.upsert_jsonl_record("Data/tasks.jsonl", task, |value| value.id)
    }

    pub fn delete_task(&self, id: Uuid) -> Result<()> {
        // Delete by envelope UUID; absent IDs are safe no-ops.
        self.delete_jsonl_record::<TaskItem>("Data/tasks.jsonl", id)
    }

    /// Perform a read-modify-write completion toggle while preserving all
    /// fields, including unknown task extras and envelope extras.
    pub fn set_task_completed(&self, id: Uuid, is_completed: bool) -> Result<()> {
        let mut tasks = self.read_tasks()?;
        let task = tasks.iter_mut().find(|task| task.id == id).ok_or_else(|| {
            WorkspaceError::MalformedData {
                path: "Data/tasks.jsonl".into(),
                detail: format!("task UUID not found: {id}"),
            }
        })?;
        task.is_completed = is_completed;
        self.upsert_task(task.clone())
    }

    /// Read the one JSON-array domain; unlike JSONL, the array has no envelope
    /// and is serialized as pretty JSON for compatibility with the iOS file.
    pub fn read_subjects(&self) -> Result<Vec<Subject>> {
        let path = self.root().join("Data/subjects.json");
        if !path.exists() {
            // An absent optional array is equivalent to an empty subject list.
            return Ok(Vec::new());
        }
        serde_json::from_slice(&fs::read(path)?).map_err(Into::into)
    }

    pub fn upsert_subject(&self, subject: Subject) -> Result<()> {
        // Array domains use a read-modify-write because their wire format has no
        // independent line envelope to update in place.
        let mut values = self.read_subjects()?;
        upsert_value(&mut values, subject, |value| value.id);
        self.write_json_value("Data/subjects.json", &values)
    }

    pub fn delete_subject(&self, id: Uuid) -> Result<()> {
        // Removing an unknown UUID is intentionally idempotent.
        let mut values = self.read_subjects()?;
        values.retain(|value| value.id != id);
        self.write_json_value("Data/subjects.json", &values)
    }

    /// Phase records use the generic JSONL envelope path.
    pub fn read_phases(&self) -> Result<Vec<StudyPhase>> {
        // The generic reader unwraps `IosRecord.value` after envelope checks.
        self.read_jsonl_records("Data/phases.jsonl")
    }

    pub fn upsert_phase(&self, value: StudyPhase) -> Result<()> {
        // The closure supplies the domain's stable UUID to the common upsert.
        self.upsert_jsonl_record("Data/phases.jsonl", value, |value| value.id)
    }

    pub fn delete_phase(&self, id: Uuid) -> Result<()> {
        // Full-file rewrite keeps ordering and duplicate handling centralized.
        // The operation remains idempotent when the UUID is already absent.
        self.delete_jsonl_record::<StudyPhase>("Data/phases.jsonl", id)
    }

    /// Grade CRUD is deliberately thin; envelope validation and atomic writes
    /// stay centralized in the generic helpers below.
    pub fn read_grades(&self) -> Result<Vec<Grade>> {
        // Grades are intentionally returned in stored order; trends sort their
        // own scoped view without mutating persistence order.
        self.read_jsonl_records("Data/grades.jsonl")
    }

    pub fn upsert_grade(&self, value: Grade) -> Result<()> {
        // Unknown grade extras survive because the typed value flattens them.
        self.upsert_jsonl_record("Data/grades.jsonl", value, |value| value.id)
    }

    pub fn delete_grade(&self, id: Uuid) -> Result<()> {
        // Missing deletion targets are harmless and keep retries idempotent.
        // The remaining envelopes are published through one atomic replacement.
        self.delete_jsonl_record::<Grade>("Data/grades.jsonl", id)
    }

    /// Wrong-question CRUD feeds SRS and trends, so it shares the same record
    /// identity and duplicate detection as every JSONL domain.
    pub fn read_mistakes(&self) -> Result<Vec<MistakeNoteFull>> {
        self.read_jsonl_records("Data/mistakes.jsonl")
    }

    pub fn upsert_mistake(&self, value: MistakeNoteFull) -> Result<()> {
        // SRS state is part of the mistake value and is replaced with the same
        // UUID, rather than creating a separate review record.
        self.upsert_jsonl_record("Data/mistakes.jsonl", value, |value| value.id)
    }

    pub fn delete_mistake(&self, id: Uuid) -> Result<()> {
        // SRS enrollment disappears with its parent mistake UUID.
        self.delete_jsonl_record::<MistakeNoteFull>("Data/mistakes.jsonl", id)
    }

    /// Diary reads re-run score/date validation because analytics consumes the
    /// values directly and must not average malformed entries.
    pub fn read_diary_entries(&self) -> Result<Vec<DiaryEntry>> {
        let values: Vec<DiaryEntry> = self.read_jsonl_records("Data/diary_entries.jsonl")?;
        for value in &values {
            value.validate()?;
        }
        Ok(values)
    }

    pub fn upsert_diary_entry(&self, value: DiaryEntry) -> Result<()> {
        // Validate before reading/updating so invalid scores never enter the
        // daily averages consumed by analytics.
        value.validate()?;
        self.upsert_jsonl_record("Data/diary_entries.jsonl", value, |entry| entry.id)
    }

    pub fn delete_diary_entry(&self, id: Uuid) -> Result<()> {
        // Preserve other entries on the same calendar date.
        self.delete_jsonl_record::<DiaryEntry>("Data/diary_entries.jsonl", id)
    }

    /// Read complete exam records, retaining their nested checklist/review data.
    pub fn read_exams(&self) -> Result<Vec<ExamFull>> {
        self.read_jsonl_records("Data/exams.jsonl")
    }

    pub fn upsert_exam(&self, value: ExamFull) -> Result<()> {
        // Nested checklist/review fields are serialized as part of one envelope.
        self.upsert_jsonl_record("Data/exams.jsonl", value, |value| value.id)
    }

    pub fn delete_exam(&self, id: Uuid) -> Result<()> {
        // Nested checklist/review data is removed with the envelope.
        self.delete_jsonl_record::<ExamFull>("Data/exams.jsonl", id)
    }

    /// Read multi-subject exams through the same envelope contract.
    pub fn read_comprehensive_exams(&self) -> Result<Vec<ComprehensiveExamFull>> {
        self.read_jsonl_records("Data/comprehensive_exams.jsonl")
    }

    pub fn upsert_comprehensive_exam(&self, value: ComprehensiveExamFull) -> Result<()> {
        // Multi-subject exam values use the same UUID replacement semantics.
        self.upsert_jsonl_record("Data/comprehensive_exams.jsonl", value, |value| value.id)
    }

    pub fn delete_comprehensive_exam(&self, id: Uuid) -> Result<()> {
        // Multi-subject exam deletion uses the same shared helper.
        self.delete_jsonl_record::<ComprehensiveExamFull>("Data/comprehensive_exams.jsonl", id)
    }

    /// Read and validate exam goals before plans are allowed to reference them.
    pub fn read_exam_goals(&self) -> Result<Vec<ExamGoal>> {
        let values: Vec<ExamGoal> = self.read_jsonl_records("Data/exam_goals.jsonl")?;
        for value in &values {
            value.validate()?;
        }
        Ok(values)
    }

    pub fn upsert_exam_goal(&self, value: ExamGoal) -> Result<()> {
        // Goal bounds are checked before a plan can establish a relationship to
        // this record.
        value.validate()?;
        self.upsert_jsonl_record("Data/exam_goals.jsonl", value, |value| value.id)
    }

    pub fn delete_exam_goal(&self, id: Uuid) -> Result<()> {
        // Plan references are checked on read rather than silently cascaded.
        self.delete_jsonl_record::<ExamGoal>("Data/exam_goals.jsonl", id)
    }

    /// Read plans and enforce the local foreign-key relationship to goals.
    pub fn read_exam_plans(&self) -> Result<Vec<ExamPlan>> {
        let goals: std::collections::HashSet<_> = self
            .read_exam_goals()?
            .into_iter()
            .map(|value| value.id)
            .collect();
        let plans: Vec<ExamPlan> = self.read_jsonl_records("Data/exam_plans.jsonl")?;
        for plan in &plans {
            plan.validate()?;
        }
        // A plan without its goal cannot be interpreted safely by the planner;
        // reject the collection instead of returning a partially valid list.
        if let Some(plan) = plans
            .iter()
            .find(|value| !goals.contains(&value.exam_goal_id))
        {
            return Err(WorkspaceError::MalformedData {
                path: "Data/exam_plans.jsonl".into(),
                detail: format!("plan references missing exam goal {}", plan.exam_goal_id),
            });
        }
        Ok(plans)
    }

    /// Validate a plan and its goal link before the envelope upsert.
    pub fn upsert_exam_plan(&self, value: ExamPlan) -> Result<()> {
        value.validate()?;
        if !self
            .read_exam_goals()?
            .iter()
            .any(|goal| goal.id == value.exam_goal_id)
        {
            return Err(WorkspaceError::MalformedData {
                path: "Data/exam_plans.jsonl".into(),
                detail: "plan references a missing exam goal".into(),
            });
        }
        self.upsert_jsonl_record("Data/exam_plans.jsonl", value, |value| value.id)
    }

    pub fn delete_exam_plan(&self, id: Uuid) -> Result<()> {
        // Removing a plan leaves its source goal and other plans untouched.
        self.delete_jsonl_record::<ExamPlan>("Data/exam_plans.jsonl", id)
    }

    /// Read simulations with question-count, answer-link, and state checks.
    pub fn read_exam_simulations(&self) -> Result<Vec<ExamSimulation>> {
        let values: Vec<ExamSimulation> = self.read_jsonl_records("Data/exam_simulations.jsonl")?;
        for value in &values {
            value.validate()?;
        }
        Ok(values)
    }

    pub fn upsert_exam_simulation(&self, value: ExamSimulation) -> Result<()> {
        // Validate the generated question graph before persisting simulation
        // state that the grading UI will later reopen.
        value.validate()?;
        self.upsert_jsonl_record("Data/exam_simulations.jsonl", value, |value| value.id)
    }

    pub fn delete_exam_simulation(&self, id: Uuid) -> Result<()> {
        // Simulation history is independently addressable by its UUID.
        self.delete_jsonl_record::<ExamSimulation>("Data/exam_simulations.jsonl", id)
    }

    /// Decode the compact Coach JSONL rows into typed collections.
    /// Unknown rows and row-level extras are retained so a newer client can be
    /// round-tripped even when this build cannot interpret its row kind.
    pub fn read_coach_data(&self) -> Result<CoachData> {
        let path = self.root().join("Data/coach_data.jsonl");
        if !path.exists() {
            return Ok(CoachData::default());
        }
        let file = fs::File::open(path)?;
        let mut data = CoachData::default();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // Parse once as Value so unknown rows can be preserved byte-for-byte
            // at the JSON semantic level, then decode known rows by kind.
            let raw: serde_json::Value =
                serde_json::from_str(&line).map_err(|error| WorkspaceError::MalformedData {
                    path: format!("Data/coach_data.jsonl:{}", index + 1),
                    detail: error.to_string(),
                })?;
            let row: CoachDataRow = serde_json::from_value(raw.clone()).map_err(|error| {
                WorkspaceError::MalformedData {
                    path: format!("Data/coach_data.jsonl:{}", index + 1),
                    detail: error.to_string(),
                }
            })?;
            match row.kind.as_str() {
                "goal" => {
                    let value: CoachGoal = decode_coach_payload(&row)?;
                    data.row_extras
                        .insert(format!("goal:{}", value.id), row.extra);
                    data.goals.push(value);
                }
                "analysis" => {
                    let value: CoachAnalysis = decode_coach_payload(&row)?;
                    data.row_extras
                        .insert(format!("analysis:{}", value.id), row.extra);
                    data.analyses.push(value);
                }
                "proposal" => {
                    let value: CoachProposal = decode_coach_payload(&row)?;
                    data.row_extras
                        .insert(format!("proposal:{}", value.id), row.extra);
                    data.proposals.push(value);
                }
                "chat" => {
                    let value: CoachChat = decode_coach_payload(&row)?;
                    data.row_extras
                        .insert(format!("chat:{}", value.id), row.extra);
                    data.chats.push(value);
                }
                "message" => {
                    let value: CoachConversationMessage = decode_coach_payload(&row)?;
                    data.row_extras
                        .insert(format!("message:{}", value.id), row.extra);
                    data.messages.push(value);
                }
                // An unknown kind is data to preserve, not an error for an
                // older desktop build that cannot display it yet.
                _ => data.unknown_rows.push(raw),
            }
        }
        data.validate()?;
        Ok(data)
    }

    pub fn upsert_coach_goal(&self, value: CoachGoal) -> Result<()> {
        value.validate()?;
        let mut data = self.read_coach_data()?;
        upsert_value(&mut data.goals, value, |value| value.id);
        self.write_coach_data(&data)
    }

    pub fn upsert_coach_analysis(&self, value: CoachAnalysis) -> Result<()> {
        // Analyses are validated as part of the aggregate read/write contract.
        let mut data = self.read_coach_data()?;
        upsert_value(&mut data.analyses, value, |value| value.id);
        self.write_coach_data(&data)
    }

    pub fn upsert_coach_proposal(&self, value: CoachProposal) -> Result<()> {
        // Proposal replacement keeps its current row extras via the aggregate
        // writer, just like the other Coach kinds.
        let mut data = self.read_coach_data()?;
        upsert_value(&mut data.proposals, value, |value| value.id);
        self.write_coach_data(&data)
    }

    pub fn upsert_coach_chat(&self, value: CoachChat) -> Result<()> {
        // Chat metadata is stored in the same heterogeneous file as messages.
        let mut data = self.read_coach_data()?;
        upsert_value(&mut data.chats, value, |value| value.id);
        self.write_coach_data(&data)
    }

    pub fn upsert_coach_message(&self, value: CoachConversationMessage) -> Result<()> {
        // Message IDs remain independent so conversation history can be merged.
        let mut data = self.read_coach_data()?;
        upsert_value(&mut data.messages, value, |value| value.id);
        self.write_coach_data(&data)
    }

    /// Delete a goal and the analyses, proposals, chats, and messages that
    /// would otherwise retain dangling references to it.
    pub fn delete_coach_goal(&self, id: Uuid) -> Result<()> {
        let mut data = self.read_coach_data()?;
        data.goals.retain(|value| value.id != id);
        data.analyses.retain(|value| value.goal_id != id);
        data.proposals.retain(|value| value.goal_id != id);
        let chat_ids: std::collections::HashSet<_> = data
            .chats
            .iter()
            .filter(|value| value.goal_id == Some(id))
            .map(|value| value.id)
            .collect();
        data.chats.retain(|value| value.goal_id != Some(id));
        data.messages
            .retain(|value| !chat_ids.contains(&value.chat_id));
        self.write_coach_data(&data)
    }

    /// Re-encode typed Coach collections into the iOS-compatible Base64 row
    /// format, restoring row extras and appending unknown rows unchanged.
    pub fn write_coach_data(&self, data: &CoachData) -> Result<()> {
        data.validate()?;
        let mut rows = Vec::new();
        for (kind, value) in [
            ("goal", serde_json::to_value(&data.goals)?),
            ("analysis", serde_json::to_value(&data.analyses)?),
            ("proposal", serde_json::to_value(&data.proposals)?),
            ("chat", serde_json::to_value(&data.chats)?),
            ("message", serde_json::to_value(&data.messages)?),
        ] {
            let values = value.as_array().cloned().unwrap_or_default();
            for value in values {
                let row = match kind {
                    "goal" => {
                        let typed: CoachGoal = serde_json::from_value(value)?;
                        let mut row = make_coach_row(kind, &typed)?;
                        row.extra = data
                            .row_extras
                            .get(&format!("goal:{}", typed.id))
                            .cloned()
                            .unwrap_or_default();
                        row
                    }
                    "analysis" => {
                        let typed: CoachAnalysis = serde_json::from_value(value)?;
                        let mut row = make_coach_row(kind, &typed)?;
                        row.extra = data
                            .row_extras
                            .get(&format!("analysis:{}", typed.id))
                            .cloned()
                            .unwrap_or_default();
                        row
                    }
                    "proposal" => {
                        let typed: CoachProposal = serde_json::from_value(value)?;
                        let mut row = make_coach_row(kind, &typed)?;
                        row.extra = data
                            .row_extras
                            .get(&format!("proposal:{}", typed.id))
                            .cloned()
                            .unwrap_or_default();
                        row
                    }
                    "chat" => {
                        let typed: CoachChat = serde_json::from_value(value)?;
                        let mut row = make_coach_row(kind, &typed)?;
                        row.extra = data
                            .row_extras
                            .get(&format!("chat:{}", typed.id))
                            .cloned()
                            .unwrap_or_default();
                        row
                    }
                    _ => {
                        let typed: CoachConversationMessage = serde_json::from_value(value)?;
                        let mut row = make_coach_row(kind, &typed)?;
                        row.extra = data
                            .row_extras
                            .get(&format!("message:{}", typed.id))
                            .cloned()
                            .unwrap_or_default();
                        row
                    }
                };
                rows.push(serde_json::to_value(row)?);
            }
        }
        rows.extend(data.unknown_rows.iter().cloned());
        let mut bytes = Vec::new();
        for row in rows {
            serde_json::to_writer(&mut bytes, &row)?;
            bytes.push(b'\n');
        }
        // Coach data is one JSONL file, so serialize the complete replacement
        // while holding the same lock used by every other persistence domain.
        let _guard = self.inner.write_lock.lock();
        atomic_write(&self.root().join("Data/coach_data.jsonl"), &bytes)
    }

    /// Build the local report only after validating its requested range.
    pub fn learning_report(&self, range_days: i64) -> Result<crate::LearningReport> {
        crate::validate_report_range(range_days)?;
        learning_report(self, range_days)
    }

    /// Routine definitions and materialized instances are separate JSONL files
    /// so historical completion data survives schedule edits.
    pub fn read_routines(&self) -> Result<Vec<Routine>> {
        // Routine values are not regenerated on read; materialization is a
        // separate caller concern.
        self.read_jsonl_records("Data/routines.jsonl")
    }

    pub fn upsert_routine(&self, value: Routine) -> Result<()> {
        self.upsert_jsonl_record("Data/routines.jsonl", value, |value| value.id)
    }

    pub fn delete_routine(&self, id: Uuid) -> Result<()> {
        // Routine definition deletion does not silently rewrite instances.
        self.delete_jsonl_record::<Routine>("Data/routines.jsonl", id)
    }

    pub fn read_routine_instances(&self) -> Result<Vec<RoutineInstance>> {
        self.read_jsonl_records("Data/routine_instances.jsonl")
    }

    pub fn upsert_routine_instance(&self, value: RoutineInstance) -> Result<()> {
        self.upsert_jsonl_record("Data/routine_instances.jsonl", value, |value| value.id)
    }

    pub fn delete_routine_instance(&self, id: Uuid) -> Result<()> {
        // One materialized occurrence can be removed without changing recurrence.
        self.delete_jsonl_record::<RoutineInstance>("Data/routine_instances.jsonl", id)
    }

    /// Study sessions are the source for streaks, reports, and investment
    /// summaries; their raw timestamps remain available to those pure helpers.
    pub fn read_study_sessions(&self) -> Result<Vec<StudySession>> {
        self.read_jsonl_records("Data/study_sessions.jsonl")
    }

    pub fn upsert_study_session(&self, value: StudySession) -> Result<()> {
        // A session is immutable history from the analytics perspective, but
        // upsert permits timer completion to update the same UUID safely.
        self.upsert_jsonl_record("Data/study_sessions.jsonl", value, |value| value.id)
    }

    pub fn delete_study_session(&self, id: Uuid) -> Result<()> {
        // Session deletion removes one history item and recalculates analytics.
        self.delete_jsonl_record::<StudySession>("Data/study_sessions.jsonl", id)
    }

    /// Read the root investment categories before resolving subtask trees.
    pub fn read_time_investment_subjects(&self) -> Result<Vec<TimeInvestmentSubject>> {
        self.read_jsonl_records("Data/time_investment_subjects.jsonl")
    }

    pub fn upsert_time_investment_subject(&self, value: TimeInvestmentSubject) -> Result<()> {
        // Root investment categories use the same envelope timestamp/update path.
        self.upsert_jsonl_record("Data/time_investment_subjects.jsonl", value, |value| {
            value.id
        })
    }

    pub fn delete_time_investment_subject(&self, id: Uuid) -> Result<()> {
        // The primitive does not cascade; relationship validation remains explicit.
        self.delete_jsonl_record::<TimeInvestmentSubject>("Data/time_investment_subjects.jsonl", id)
    }

    /// Read nested investment targets; parent relationships are validated by
    /// backup import and interpreted by analytics during aggregation.
    pub fn read_subtasks(&self) -> Result<Vec<SubTask>> {
        self.read_jsonl_records("Data/time_investment_subtasks.jsonl")
    }

    pub fn upsert_subtask(&self, value: SubTask) -> Result<()> {
        // Parent IDs are preserved as value fields for analytics tree traversal.
        self.upsert_jsonl_record("Data/time_investment_subtasks.jsonl", value, |value| {
            value.id
        })
    }

    pub fn delete_subtask(&self, id: Uuid) -> Result<()> {
        // Parent/child cleanup is a caller decision, not hidden in storage.
        self.delete_jsonl_record::<SubTask>("Data/time_investment_subtasks.jsonl", id)
    }

    pub fn read_goal_rewards(&self) -> Result<Vec<GoalReward>> {
        self.read_jsonl_records("Data/goal_rewards.jsonl")
    }

    pub fn upsert_goal_reward(&self, value: GoalReward) -> Result<()> {
        // Rewards reference the investment target using its custom Swift shape.
        self.upsert_jsonl_record("Data/goal_rewards.jsonl", value, |value| value.id)
    }

    pub fn delete_goal_reward(&self, id: Uuid) -> Result<()> {
        // Rewards are independent records even when their target is removed.
        self.delete_jsonl_record::<GoalReward>("Data/goal_rewards.jsonl", id)
    }

    /// Remove one envelope by UUID and rewrite the remaining complete file.
    /// Full-file replacement keeps the JSONL invariant simple and deterministic.
    pub fn delete_jsonl_record<T: DeserializeOwned + Serialize>(
        &self,
        relative: &str,
        id: Uuid,
    ) -> Result<()> {
        let mut records: Vec<IosRecord<T>> = self.read_jsonl_envelopes(relative)?;
        records.retain(|record| record.id != id);
        self.write_jsonl_records(relative, &records)
    }

    /// Resolve a media-relative path while retaining the `Media/` prefix in the
    /// validation input; the caller still has to choose images or audio.
    pub fn media_path(&self, relative: &str) -> Result<PathBuf> {
        // Prefixing before parsing prevents a caller from escaping by passing a
        // path that is valid on its own but not under the media namespace.
        let path = format!("Media/{relative}");
        let safe = SafeRelativePath::parse(&path)?;
        ensure_no_symlink_components(self.root(), safe.as_path())?;
        Ok(self.root().join(safe.as_path()))
    }

    /// Read a bounded media file after link and regular-file checks.
    pub fn read_media(&self, relative: &str) -> Result<Vec<u8>> {
        let path = self.media_path(relative)?;
        let metadata = fs::symlink_metadata(&path)?;
        // Use metadata from the link itself so a redirected file cannot bypass
        // the size/type check before `fs::read` follows it.
        if !metadata.is_file() || metadata.len() > MAX_MEDIA_FILE_BYTES {
            return Err(WorkspaceError::InvalidPath(
                "media file is missing or exceeds the 64 MiB limit".into(),
            ));
        }
        Ok(fs::read(path)?)
    }

    /// Write only below `Media/images` or `Media/audio`, enforcing the 64 MiB
    /// limit before taking the lock and replacing the file atomically.
    pub fn write_media(&self, relative: &str, contents: &[u8]) -> Result<String> {
        let path = self.media_path(relative)?;
        let relative = SafeRelativePath::parse(&format!("Media/{relative}"))?;
        // Inspect the first component after `Media` rather than accepting an
        // arbitrary category that merely passed generic relative-path syntax.
        let first = relative
            .as_path()
            .components()
            .nth(1)
            .and_then(|component| component.as_os_str().to_str())
            .ok_or_else(|| WorkspaceError::InvalidPath("media category is missing".into()))?;
        if !matches!(first, "images" | "audio") {
            return Err(WorkspaceError::InvalidPath(
                "media must be stored below Media/images or Media/audio".into(),
            ));
        }
        // Enforce the byte limit before creating parent directories or taking
        // the write lock.
        if contents.len() as u64 > MAX_MEDIA_FILE_BYTES {
            return Err(WorkspaceError::InvalidPath(
                "media file exceeds the 64 MiB limit".into(),
            ));
        }
        let _guard = self.inner.write_lock.lock();
        atomic_write(&path, contents)?;
        Ok(relative
            .as_path()
            .strip_prefix("Media")
            .map_err(|_| WorkspaceError::PathEscape(relative.as_path().display().to_string()))?
            .to_string_lossy()
            .trim_start_matches(['/', '\\'])
            .replace('\\', "/"))
    }

    /// Return only envelope values while keeping all JSONL checks in one place.
    fn read_jsonl_records<T: DeserializeOwned>(&self, relative: &str) -> Result<Vec<T>> {
        // Mapping after envelope validation deliberately hides transport
        // metadata from domain callers.
        Ok(self
            .read_jsonl_envelopes(relative)?
            .into_iter()
            .map(|record| record.value)
            .collect())
    }

    /// Parse a JSONL file as envelopes and enforce its record-level invariants.
    ///
    /// The raw `Value` pass checks the duplicated IDs before typed deserialization
    /// so a malformed envelope reports the file and physical line. The UUID set
    /// then rejects duplicate records instead of letting last-write-wins hide
    /// corruption in an existing Workspace.
    fn read_jsonl_envelopes<T: DeserializeOwned>(
        &self,
        relative: &str,
    ) -> Result<Vec<IosRecord<T>>> {
        let path = self.root().join(relative);
        if !path.exists() {
            // Workspace::create normally creates these files, but treating an
            // absent newer domain as empty keeps old workspaces readable.
            return Ok(Vec::new());
        }
        let file = fs::File::open(&path)?;
        let mut records = Vec::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                // Permit blank separators inserted by manual repair tools.
                continue;
            }
            // Keep line-aware context for syntax errors and envelope checks.
            let raw: serde_json::Value =
                serde_json::from_str(&line).map_err(|error| WorkspaceError::MalformedData {
                    path: format!("{relative}:{}", index + 1),
                    detail: error.to_string(),
                })?;
            // Inspect both IDs as strings before typed parsing to provide the
            // same mismatch error for every generic record type.
            if let (Some(envelope_id), Some(value_id)) = (
                raw.get("id").and_then(serde_json::Value::as_str),
                raw.get("value")
                    .and_then(|value| value.get("id"))
                    .and_then(serde_json::Value::as_str),
            ) && envelope_id != value_id
            {
                return Err(WorkspaceError::MalformedData {
                    path: format!("{relative}:{}", index + 1),
                    detail: "envelope id does not match value id".into(),
                });
            }
            // Deserialize again into the typed envelope only after structural
            // checks have passed; this preserves useful error ordering.
            let record: IosRecord<T> =
                serde_json::from_str(&line).map_err(|error| WorkspaceError::MalformedData {
                    path: format!("{relative}:{}", index + 1),
                    detail: error.to_string(),
                })?;
            // Duplicate UUIDs make upsert/delete ambiguous, so fail closed.
            if records
                .iter()
                .any(|existing: &IosRecord<T>| existing.id == record.id)
            {
                return Err(WorkspaceError::MalformedData {
                    path: format!("{relative}:{}", index + 1),
                    detail: format!("duplicate UUID: {}", record.id),
                });
            }
            records.push(record);
        }
        Ok(records)
    }

    /// Serialize complete JSONL replacement content and publish it atomically.
    /// Callers do not write individual lines because a crash midway would leave
    /// a syntactically valid prefix with lost records.
    fn write_jsonl_records<T: Serialize>(&self, relative: &str, records: &[T]) -> Result<()> {
        let mut bytes = Vec::new();
        for record in records {
            // One newline per envelope keeps the file streamable and matches
            // the import/backup line-count contract.
            serde_json::to_writer(&mut bytes, record)?;
            bytes.push(b'\n');
        }
        // Serialize with all other writes before the temp-file/rename sequence.
        let _guard = self.inner.write_lock.lock();
        atomic_write(&self.root().join(relative), &bytes)
    }

    /// Replace one record by UUID while preserving its existing envelope extras.
    /// The updatedAt timestamp is generated in UTC with millisecond precision,
    /// matching the desktop/iOS interchange contract.
    fn upsert_jsonl_record<T, F>(&self, relative: &str, value: T, id: F) -> Result<()>
    where
        T: Clone + DeserializeOwned + Serialize,
        F: Fn(&T) -> Uuid,
    {
        let mut records: Vec<IosRecord<T>> = self.read_jsonl_envelopes(relative)?;
        let value_id = id(&value);
        // Envelope extras are metadata outside the typed value; carry them over
        // when updating so a newer client does not lose its fields.
        let envelope_extra = records
            .iter()
            .find(|record| record.id == value_id)
            .map(|record| record.extra.clone())
            .unwrap_or_default();
        // `updated_at` belongs to the envelope so every domain gets one common
        // timestamp policy without modifying the value's own historical dates.
        let envelope = IosRecord {
            dto_version: 1,
            id: value_id,
            updated_at: Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            value,
            extra: envelope_extra,
        };
        // Upsert replaces exactly one UUID and appends a new UUID in file order.
        if let Some(existing) = records.iter_mut().find(|record| record.id == value_id) {
            *existing = envelope;
        } else {
            records.push(envelope);
        }
        self.write_jsonl_records(relative, &records)
    }

    /// Write a pretty JSON singleton/array through the same lock and atomic
    /// replacement used by JSONL domains.
    fn write_json_value<T: Serialize>(&self, relative: &str, value: &T) -> Result<()> {
        // Pretty JSON is used for array/singleton files to remain inspectable;
        // atomic replacement still provides the crash-safety boundary.
        let bytes = serde_json::to_vec_pretty(value)?;
        let _guard = self.inner.write_lock.lock();
        atomic_write(&self.root().join(relative), &bytes)
    }

    /// Read and validate Notebook index data, returning an empty list when the
    /// optional index file has not been created by an older Workspace.
    pub fn read_agent_notebooks(&self) -> Result<Vec<AgentNotebook>> {
        let path = self.root().join("Agent/notebooks.json");
        if !path.exists() {
            // The index is optional for workspaces that have not used Agent yet.
            return Ok(Vec::new());
        }
        let notebooks: Vec<AgentNotebook> = serde_json::from_slice(&fs::read(path)?)?;
        for notebook in &notebooks {
            notebook.validate()?;
        }
        Ok(notebooks)
    }

    /// Validate Notebook IDs and selected-source allow-lists before publishing
    /// the complete pretty-JSON index atomically.
    pub fn write_agent_notebooks(&self, notebooks: Vec<AgentNotebook>) -> Result<()> {
        let mut ids = std::collections::HashSet::new();
        for notebook in &notebooks {
            // Validate both content and source authorization before publishing
            // any part of the index.
            notebook.validate()?;
            if !ids.insert(notebook.id) {
                return Err(WorkspaceError::MalformedData {
                    path: "Agent/notebooks.json".into(),
                    detail: format!("duplicate notebook id: {}", notebook.id),
                });
            }
            // This also rejects deleted/renamed sources, preventing a stale
            // Notebook allow-list from authorizing an arbitrary replacement.
            self.list_selected_library_files(&notebook.source_paths)?;
        }
        let bytes = serde_json::to_vec_pretty(&notebooks)?;
        let _guard = self.inner.write_lock.lock();
        atomic_write(&self.root().join("Agent/notebooks.json"), &bytes)
    }

    /// Borrow the write mutex for multi-file transactions such as backup restore.
    /// The guard's lifetime intentionally covers the caller's whole operation.
    pub(crate) fn exclusive_write(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.inner.write_lock.lock()
    }
}

fn is_hidden_or_link(entry: &walkdir::DirEntry) -> bool {
    // Hidden trees are excluded before traversal descends into them; link-like
    // entries are also treated as hidden so WalkDir cannot expose redirects.
    if entry.depth() > 0 && entry.file_name().to_string_lossy().starts_with('.') {
        return true;
    }
    fs::symlink_metadata(entry.path())
        .map(|metadata| is_link_like(&metadata))
        .unwrap_or(true)
}

fn is_probably_text(path: &Path) -> Result<bool> {
    // A small prefix sample is enough to reject common binary files without
    // reading the entire source; UTF-8 validation is repeated on actual reads.
    let mut file = fs::File::open(path)?;
    let mut bytes = vec![0_u8; 8 * 1024];
    let count = std::io::Read::read(&mut file, &mut bytes)?;
    bytes.truncate(count);
    Ok(!bytes.contains(&0) && std::str::from_utf8(&bytes).is_ok())
}

fn wire_path(path: &Path) -> String {
    // Convert native components to the portable slash-separated form exposed
    // to the frontend and Agent. Non-normal components are omitted by design.
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn unique_source_name(root: &Path, file_name: &str) -> Result<String> {
    // Preserve the original leaf when free; otherwise append a bounded numeric
    // suffix so importing the same source never overwrites user content.
    if !root.join("Documents").join(file_name).exists() {
        return Ok(file_name.into());
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| WorkspaceError::InvalidPath(file_name.into()))?;
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 2..=10_000 {
        let candidate = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        if !root.join("Documents").join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err(WorkspaceError::InvalidPath(format!(
        "could not create a unique source name for {file_name}"
    )))
}

fn upsert_value<T, F>(values: &mut Vec<T>, value: T, id: F)
where
    F: Fn(&T) -> Uuid,
{
    // JSON-array domains use the same UUID upsert semantics as JSONL domains,
    // but without an envelope layer.
    let value_id = id(&value);
    if let Some(existing) = values.iter_mut().find(|existing| id(existing) == value_id) {
        *existing = value;
    } else {
        values.push(value);
    }
}

fn system_time_string(value: SystemTime) -> String {
    // File metadata is surfaced as UTC seconds; persisted domain timestamps use
    // their own millisecond helper when exact update ordering matters.
    let value: DateTime<Utc> = value.into();
    value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    // Write beside the target, flush the complete payload, then rename it into
    // place. A UUID temp name avoids collisions, while `create_new` prevents a
    // stale temp file from being overwritten. The caller owns serialization via
    // `write_lock`; this helper is deliberately small and does not lock itself.
    let parent = path
        .parent()
        .ok_or_else(|| WorkspaceError::InvalidPath(path.display().to_string()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".studypulse-write-{}.tmp", Uuid::new_v4()));
    {
        // Closing the file before remove/rename makes the replacement portable
        // to Windows and ensures the bytes have reached the filesystem buffer.
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.flush()?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{AgentMessage, AgentMessageRole};

    #[test]
    // The create/append/read path proves the directory skeleton and the typed
    // JSONL envelope can round-trip one complete task.
    fn creates_expected_workspace_and_round_trips_task() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Study")).unwrap();
        for path in [
            "Documents",
            "Notes",
            "Data/tasks.jsonl",
            "Media/images",
            "Agent/runs",
            ".studypulse/workspace.json",
        ] {
            assert!(workspace.root().join(path).exists(), "{path}");
        }
        let now = "2026-07-30T09:00:00Z".to_string();
        let task = TaskItem {
            id: Uuid::new_v4(),
            title: "Review algebra".into(),
            task_type: crate::TaskType::Homework,
            due_date: now.clone(),
            reminder_date: now.clone(),
            subject: "Math".into(),
            importance: 4,
            notes: String::new(),
            is_completed: false,
            reminder_event_id: None,
            reminder_calendar_id: None,
            created_at: now,
            phase_id: None,
            coach_execution_data: None,
            coach_goal_id: None,
            coach_proposal_id: None,
            extra: BTreeMap::new(),
        };
        workspace.append_task(task.clone()).unwrap();
        assert_eq!(workspace.read_tasks().unwrap(), vec![task]);
    }

    #[test]
    // Unknown value fields must survive an upsert/delete cycle rather than being
    // dropped by the Rust model's known-field projection.
    fn diary_jsonl_round_trip_preserves_unknown_fields() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Study")).unwrap();
        let id = Uuid::new_v4();
        let mut extra = BTreeMap::new();
        extra.insert("iosOnlyField".into(), serde_json::json!({ "kept": true }));
        let entry = DiaryEntry {
            id,
            date: "2026-07-31T00:00:00Z".into(),
            mood_score: 4,
            energy_score: 2,
            energy_tag: "focused".into(),
            content: "## Review\n\nWorked through algebra.".into(),
            phase_id: None,
            created_at: "2026-07-31T08:00:00Z".into(),
            updated_at: "2026-07-31T09:00:00Z".into(),
            extra,
        };
        workspace.upsert_diary_entry(entry.clone()).unwrap();
        assert_eq!(workspace.read_diary_entries().unwrap(), vec![entry]);
        workspace.delete_diary_entry(id).unwrap();
        assert!(workspace.read_diary_entries().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    // A link that points outside the root is rejected before canonical access,
    // guarding the primary path-escape threat on Unix.
    fn rejects_symbolic_link_escape() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Study")).unwrap();
        symlink(outside.path(), workspace.root().join("Documents/outside")).unwrap();
        assert!(workspace.resolve_existing("Documents/outside").is_err());
    }

    #[test]
    // Library enumeration hides both dot-directories and binary content, while
    // text search still returns a bounded matching source.
    fn library_skips_hidden_trees_and_binary_files() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Study")).unwrap();
        fs::create_dir_all(workspace.root().join("Notes/.private")).unwrap();
        fs::write(
            workspace.root().join("Notes/.private/secret.txt"),
            "not visible",
        )
        .unwrap();
        fs::write(workspace.root().join("Documents/lesson.md"), "Algebra").unwrap();
        fs::write(
            workspace.root().join("Documents/archive.bin"),
            [0_u8, 1, 2, 3],
        )
        .unwrap();

        let files = workspace.list_library_files().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "Documents/lesson.md");
        assert_eq!(workspace.search_library("algebra").unwrap().len(), 1);
        assert!(workspace.search_library("secret").unwrap().is_empty());
    }

    #[test]
    // Agent reads/writes honor source bounds, scope allow-lists, and artifact
    // name restrictions while still round-tripping valid values.
    fn agent_sources_memory_and_artifacts_are_bounded_and_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Study")).unwrap();
        fs::write(
            workspace.root().join("Documents/lesson.md"),
            "Algebra\nFunctions",
        )
        .unwrap();
        assert_eq!(
            workspace
                .read_library_source("Documents/lesson.md", 7)
                .unwrap(),
            "Algebra"
        );
        assert!(workspace.read_library_source("../outside", 100).is_err());

        workspace
            .write_agent_memory("workspace", &serde_json::json!({"language": "zh"}))
            .unwrap();
        assert_eq!(
            workspace.read_agent_memory("workspace").unwrap()["language"],
            "zh"
        );

        let relative = workspace
            .write_agent_artifact("run-1", "artifact-1", "md", b"# Report")
            .unwrap();
        assert_eq!(relative, "Agent/artifacts/run-1/artifact-1.md");
        assert_eq!(
            fs::read(workspace.root().join(relative)).unwrap(),
            b"# Report"
        );
        assert!(
            workspace
                .write_agent_artifact("run_2", "artifact-2", "json", b"{}")
                .is_ok()
        );
        assert!(
            workspace
                .write_agent_artifact("../run", "artifact", "md", b"bad")
                .is_err()
        );
    }

    #[test]
    // Notebook persistence validates its selected source path and nested message
    // timestamps without changing the pretty-JSON index shape.
    fn notebook_sources_round_trip_inside_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Study")).unwrap();
        fs::write(workspace.root().join("Documents/lesson.md"), "Algebra").unwrap();
        let notebook = AgentNotebook {
            id: Uuid::new_v4(),
            title: "Algebra Review".into(),
            source_paths: vec!["Documents/lesson.md".into()],
            messages: vec![
                AgentMessage {
                    id: Uuid::new_v4(),
                    role: AgentMessageRole::User,
                    content: "Make a study plan".into(),
                    created_at: "2026-07-30T08:59:00Z".into(),
                },
                AgentMessage {
                    id: Uuid::new_v4(),
                    role: AgentMessageRole::Assistant,
                    content: "Start with chapter one.".into(),
                    created_at: "2026-07-30T09:00:00Z".into(),
                },
            ],
            last_goal: "Make a study plan".into(),
            last_answer: "Start with chapter one.".into(),
            updated_at: "2026-07-30T09:00:00Z".into(),
        };

        workspace
            .write_agent_notebooks(vec![notebook.clone()])
            .unwrap();

        assert_eq!(workspace.read_agent_notebooks().unwrap(), vec![notebook]);
        assert!(workspace.root().join("Agent/notebooks.json").is_file());
    }

    #[test]
    // Missing optional message arrays are the compatibility default for legacy
    // notebook JSON and must not make the whole index unreadable.
    fn legacy_notebook_without_messages_remains_readable() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Study")).unwrap();
        fs::write(
            workspace.root().join("Agent/notebooks.json"),
            r#"[{
                "id": "c70d3815-7177-43fc-954f-2c2df5e153fd",
                "title": "Legacy Notebook",
                "sourcePaths": [],
                "lastGoal": "Old question",
                "lastAnswer": "Old answer",
                "updatedAt": "2026-07-30T09:00:00Z"
            }]"#,
        )
        .unwrap();

        let notebooks = workspace.read_agent_notebooks().unwrap();

        assert_eq!(notebooks.len(), 1);
        assert!(notebooks[0].messages.is_empty());
        assert_eq!(notebooks[0].last_goal, "Old question");
        assert_eq!(notebooks[0].last_answer, "Old answer");
    }

    #[test]
    // Imports are text-only and collision-safe: the second same-named source is
    // renamed instead of overwriting the first.
    fn imported_sources_are_text_only_and_do_not_overwrite() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Study")).unwrap();

        let first = workspace
            .import_library_source("lesson.md", b"Algebra".to_vec())
            .unwrap();
        let second = workspace
            .import_library_source("lesson.md", b"Geometry".to_vec())
            .unwrap();

        assert_eq!(first.relative_path, "Documents/lesson.md");
        assert_eq!(second.relative_path, "Documents/lesson-2.md");
        assert!(
            workspace
                .import_library_source("binary.dat", vec![0, 1, 2])
                .is_err()
        );
        assert!(
            workspace
                .import_library_source("../escape.md", b"no".to_vec())
                .is_err()
        );
    }
}
