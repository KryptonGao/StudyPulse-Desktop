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
    AgentNotebook, ComprehensiveExamFull, DiaryEntry, ExamFull, FileEntry, GoalReward, Grade,
    IosRecord, MistakeNoteFull, Result, Routine, RoutineInstance, SafeRelativePath, SearchMatch,
    StudyPhase, StudySession, SubTask, Subject, TaskItem, TimeInvestmentSubject, WorkspaceError,
    WorkspaceInfo, platform::is_link_like, safe_path::ensure_no_symlink_components,
};

const WORKSPACE_SCHEMA_VERSION: u32 = 1;
const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;
const MAX_SEARCH_RESULTS: usize = 50;
const MAX_MEDIA_FILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceMetadata {
    format_identifier: String,
    id: Uuid,
    schema_version: u32,
}

#[derive(Debug)]
struct WorkspaceInner {
    root: PathBuf,
    info: WorkspaceInfo,
    write_lock: Mutex<()>,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    inner: Arc<WorkspaceInner>,
}

impl Workspace {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref();
        fs::create_dir_all(root)?;
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
        ] {
            let path = root.join("Data").join(file);
            if !path.exists() {
                OpenOptions::new().create_new(true).write(true).open(path)?;
            }
        }
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

    pub fn info(&self) -> WorkspaceInfo {
        self.inner.info.clone()
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

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

    pub fn list_library_files(&self) -> Result<Vec<FileEntry>> {
        let mut entries = Vec::new();
        for top_level in ["Documents", "Notes"] {
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

    pub fn list_selected_library_files(&self, source_paths: &[String]) -> Result<Vec<FileEntry>> {
        let available = self.list_library_files()?;
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

    pub fn search_library(&self, query: &str) -> Result<Vec<SearchMatch>> {
        let entries = self.list_library_files()?;
        self.search_library_entries(query, entries)
    }

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
        Ok(text.chars().take(max_chars).collect())
    }

    /// Persist a generated Agent artifact below the Workspace root.
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
        let relative = format!("Agent/artifacts/{run_id}/{artifact_id}.{extension}");
        SafeRelativePath::parse(&relative)?;
        let path = self.root().join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _guard = self.inner.write_lock.lock();
        atomic_write(&path, contents)?;
        Ok(relative)
    }

    pub fn read_agent_memory(&self, scope: &str) -> Result<serde_json::Value> {
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
        let path = self.root().join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(value)?;
        let _guard = self.inner.write_lock.lock();
        atomic_write(&path, &bytes)
    }

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

    pub fn read_tasks(&self) -> Result<Vec<TaskItem>> {
        let path = self.root().join("Data/tasks.jsonl");
        let file = fs::File::open(&path)?;
        let mut tasks = Vec::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
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

    pub fn append_task(&self, task: TaskItem) -> Result<()> {
        self.upsert_task(task)
    }

    pub fn upsert_task(&self, task: TaskItem) -> Result<()> {
        task.validate()?;
        self.upsert_jsonl_record("Data/tasks.jsonl", task, |value| value.id)
    }

    pub fn delete_task(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<TaskItem>("Data/tasks.jsonl", id)
    }

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

    pub fn read_subjects(&self) -> Result<Vec<Subject>> {
        let path = self.root().join("Data/subjects.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        serde_json::from_slice(&fs::read(path)?).map_err(Into::into)
    }

    pub fn upsert_subject(&self, subject: Subject) -> Result<()> {
        let mut values = self.read_subjects()?;
        upsert_value(&mut values, subject, |value| value.id);
        self.write_json_value("Data/subjects.json", &values)
    }

    pub fn delete_subject(&self, id: Uuid) -> Result<()> {
        let mut values = self.read_subjects()?;
        values.retain(|value| value.id != id);
        self.write_json_value("Data/subjects.json", &values)
    }

    pub fn read_phases(&self) -> Result<Vec<StudyPhase>> {
        self.read_jsonl_records("Data/phases.jsonl")
    }

    pub fn upsert_phase(&self, value: StudyPhase) -> Result<()> {
        self.upsert_jsonl_record("Data/phases.jsonl", value, |value| value.id)
    }

    pub fn delete_phase(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<StudyPhase>("Data/phases.jsonl", id)
    }

    pub fn read_grades(&self) -> Result<Vec<Grade>> {
        self.read_jsonl_records("Data/grades.jsonl")
    }

    pub fn upsert_grade(&self, value: Grade) -> Result<()> {
        self.upsert_jsonl_record("Data/grades.jsonl", value, |value| value.id)
    }

    pub fn delete_grade(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<Grade>("Data/grades.jsonl", id)
    }

    pub fn read_mistakes(&self) -> Result<Vec<MistakeNoteFull>> {
        self.read_jsonl_records("Data/mistakes.jsonl")
    }

    pub fn upsert_mistake(&self, value: MistakeNoteFull) -> Result<()> {
        self.upsert_jsonl_record("Data/mistakes.jsonl", value, |value| value.id)
    }

    pub fn delete_mistake(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<MistakeNoteFull>("Data/mistakes.jsonl", id)
    }

    pub fn read_diary_entries(&self) -> Result<Vec<DiaryEntry>> {
        let values: Vec<DiaryEntry> = self.read_jsonl_records("Data/diary_entries.jsonl")?;
        for value in &values {
            value.validate()?;
        }
        Ok(values)
    }

    pub fn upsert_diary_entry(&self, value: DiaryEntry) -> Result<()> {
        value.validate()?;
        self.upsert_jsonl_record("Data/diary_entries.jsonl", value, |entry| entry.id)
    }

    pub fn delete_diary_entry(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<DiaryEntry>("Data/diary_entries.jsonl", id)
    }

    pub fn read_exams(&self) -> Result<Vec<ExamFull>> {
        self.read_jsonl_records("Data/exams.jsonl")
    }

    pub fn upsert_exam(&self, value: ExamFull) -> Result<()> {
        self.upsert_jsonl_record("Data/exams.jsonl", value, |value| value.id)
    }

    pub fn delete_exam(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<ExamFull>("Data/exams.jsonl", id)
    }

    pub fn read_comprehensive_exams(&self) -> Result<Vec<ComprehensiveExamFull>> {
        self.read_jsonl_records("Data/comprehensive_exams.jsonl")
    }

    pub fn upsert_comprehensive_exam(&self, value: ComprehensiveExamFull) -> Result<()> {
        self.upsert_jsonl_record("Data/comprehensive_exams.jsonl", value, |value| value.id)
    }

    pub fn delete_comprehensive_exam(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<ComprehensiveExamFull>("Data/comprehensive_exams.jsonl", id)
    }

    pub fn read_routines(&self) -> Result<Vec<Routine>> {
        self.read_jsonl_records("Data/routines.jsonl")
    }

    pub fn upsert_routine(&self, value: Routine) -> Result<()> {
        self.upsert_jsonl_record("Data/routines.jsonl", value, |value| value.id)
    }

    pub fn delete_routine(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<Routine>("Data/routines.jsonl", id)
    }

    pub fn read_routine_instances(&self) -> Result<Vec<RoutineInstance>> {
        self.read_jsonl_records("Data/routine_instances.jsonl")
    }

    pub fn upsert_routine_instance(&self, value: RoutineInstance) -> Result<()> {
        self.upsert_jsonl_record("Data/routine_instances.jsonl", value, |value| value.id)
    }

    pub fn delete_routine_instance(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<RoutineInstance>("Data/routine_instances.jsonl", id)
    }

    pub fn read_study_sessions(&self) -> Result<Vec<StudySession>> {
        self.read_jsonl_records("Data/study_sessions.jsonl")
    }

    pub fn upsert_study_session(&self, value: StudySession) -> Result<()> {
        self.upsert_jsonl_record("Data/study_sessions.jsonl", value, |value| value.id)
    }

    pub fn delete_study_session(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<StudySession>("Data/study_sessions.jsonl", id)
    }

    pub fn read_time_investment_subjects(&self) -> Result<Vec<TimeInvestmentSubject>> {
        self.read_jsonl_records("Data/time_investment_subjects.jsonl")
    }

    pub fn upsert_time_investment_subject(&self, value: TimeInvestmentSubject) -> Result<()> {
        self.upsert_jsonl_record("Data/time_investment_subjects.jsonl", value, |value| {
            value.id
        })
    }

    pub fn delete_time_investment_subject(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<TimeInvestmentSubject>("Data/time_investment_subjects.jsonl", id)
    }

    pub fn read_subtasks(&self) -> Result<Vec<SubTask>> {
        self.read_jsonl_records("Data/time_investment_subtasks.jsonl")
    }

    pub fn upsert_subtask(&self, value: SubTask) -> Result<()> {
        self.upsert_jsonl_record("Data/time_investment_subtasks.jsonl", value, |value| {
            value.id
        })
    }

    pub fn delete_subtask(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<SubTask>("Data/time_investment_subtasks.jsonl", id)
    }

    pub fn read_goal_rewards(&self) -> Result<Vec<GoalReward>> {
        self.read_jsonl_records("Data/goal_rewards.jsonl")
    }

    pub fn upsert_goal_reward(&self, value: GoalReward) -> Result<()> {
        self.upsert_jsonl_record("Data/goal_rewards.jsonl", value, |value| value.id)
    }

    pub fn delete_goal_reward(&self, id: Uuid) -> Result<()> {
        self.delete_jsonl_record::<GoalReward>("Data/goal_rewards.jsonl", id)
    }

    pub fn delete_jsonl_record<T: DeserializeOwned + Serialize>(
        &self,
        relative: &str,
        id: Uuid,
    ) -> Result<()> {
        let mut records: Vec<IosRecord<T>> = self.read_jsonl_envelopes(relative)?;
        records.retain(|record| record.id != id);
        self.write_jsonl_records(relative, &records)
    }

    pub fn media_path(&self, relative: &str) -> Result<PathBuf> {
        let path = format!("Media/{relative}");
        let safe = SafeRelativePath::parse(&path)?;
        ensure_no_symlink_components(self.root(), safe.as_path())?;
        Ok(self.root().join(safe.as_path()))
    }

    pub fn read_media(&self, relative: &str) -> Result<Vec<u8>> {
        let path = self.media_path(relative)?;
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.len() > MAX_MEDIA_FILE_BYTES {
            return Err(WorkspaceError::InvalidPath(
                "media file is missing or exceeds the 64 MiB limit".into(),
            ));
        }
        Ok(fs::read(path)?)
    }

    pub fn write_media(&self, relative: &str, contents: &[u8]) -> Result<String> {
        let path = self.media_path(relative)?;
        let relative = SafeRelativePath::parse(&format!("Media/{relative}"))?;
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

    fn read_jsonl_records<T: DeserializeOwned>(&self, relative: &str) -> Result<Vec<T>> {
        Ok(self
            .read_jsonl_envelopes(relative)?
            .into_iter()
            .map(|record| record.value)
            .collect())
    }

    fn read_jsonl_envelopes<T: DeserializeOwned>(
        &self,
        relative: &str,
    ) -> Result<Vec<IosRecord<T>>> {
        let path = self.root().join(relative);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(&path)?;
        let mut records = Vec::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let raw: serde_json::Value =
                serde_json::from_str(&line).map_err(|error| WorkspaceError::MalformedData {
                    path: format!("{relative}:{}", index + 1),
                    detail: error.to_string(),
                })?;
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
            let record: IosRecord<T> =
                serde_json::from_str(&line).map_err(|error| WorkspaceError::MalformedData {
                    path: format!("{relative}:{}", index + 1),
                    detail: error.to_string(),
                })?;
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

    fn write_jsonl_records<T: Serialize>(&self, relative: &str, records: &[T]) -> Result<()> {
        let mut bytes = Vec::new();
        for record in records {
            serde_json::to_writer(&mut bytes, record)?;
            bytes.push(b'\n');
        }
        let _guard = self.inner.write_lock.lock();
        atomic_write(&self.root().join(relative), &bytes)
    }

    fn upsert_jsonl_record<T, F>(&self, relative: &str, value: T, id: F) -> Result<()>
    where
        T: Clone + DeserializeOwned + Serialize,
        F: Fn(&T) -> Uuid,
    {
        let mut records: Vec<IosRecord<T>> = self.read_jsonl_envelopes(relative)?;
        let value_id = id(&value);
        let envelope_extra = records
            .iter()
            .find(|record| record.id == value_id)
            .map(|record| record.extra.clone())
            .unwrap_or_default();
        let envelope = IosRecord {
            dto_version: 1,
            id: value_id,
            updated_at: Some(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            value,
            extra: envelope_extra,
        };
        if let Some(existing) = records.iter_mut().find(|record| record.id == value_id) {
            *existing = envelope;
        } else {
            records.push(envelope);
        }
        self.write_jsonl_records(relative, &records)
    }

    fn write_json_value<T: Serialize>(&self, relative: &str, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        let _guard = self.inner.write_lock.lock();
        atomic_write(&self.root().join(relative), &bytes)
    }

    pub fn read_agent_notebooks(&self) -> Result<Vec<AgentNotebook>> {
        let path = self.root().join("Agent/notebooks.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let notebooks: Vec<AgentNotebook> = serde_json::from_slice(&fs::read(path)?)?;
        for notebook in &notebooks {
            notebook.validate()?;
        }
        Ok(notebooks)
    }

    pub fn write_agent_notebooks(&self, notebooks: Vec<AgentNotebook>) -> Result<()> {
        let mut ids = std::collections::HashSet::new();
        for notebook in &notebooks {
            notebook.validate()?;
            if !ids.insert(notebook.id) {
                return Err(WorkspaceError::MalformedData {
                    path: "Agent/notebooks.json".into(),
                    detail: format!("duplicate notebook id: {}", notebook.id),
                });
            }
            self.list_selected_library_files(&notebook.source_paths)?;
        }
        let bytes = serde_json::to_vec_pretty(&notebooks)?;
        let _guard = self.inner.write_lock.lock();
        atomic_write(&self.root().join("Agent/notebooks.json"), &bytes)
    }

    pub(crate) fn exclusive_write(&self) -> parking_lot::MutexGuard<'_, ()> {
        self.inner.write_lock.lock()
    }
}

fn is_hidden_or_link(entry: &walkdir::DirEntry) -> bool {
    if entry.depth() > 0 && entry.file_name().to_string_lossy().starts_with('.') {
        return true;
    }
    fs::symlink_metadata(entry.path())
        .map(|metadata| is_link_like(&metadata))
        .unwrap_or(true)
}

fn is_probably_text(path: &Path) -> Result<bool> {
    let mut file = fs::File::open(path)?;
    let mut bytes = vec![0_u8; 8 * 1024];
    let count = std::io::Read::read(&mut file, &mut bytes)?;
    bytes.truncate(count);
    Ok(!bytes.contains(&0) && std::str::from_utf8(&bytes).is_ok())
}

fn wire_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn unique_source_name(root: &Path, file_name: &str) -> Result<String> {
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
    let value_id = id(&value);
    if let Some(existing) = values.iter_mut().find(|existing| id(existing) == value_id) {
        *existing = value;
    } else {
        values.push(value);
    }
}

fn system_time_string(value: SystemTime) -> String {
    let value: DateTime<Utc> = value.into();
    value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| WorkspaceError::InvalidPath(path.display().to_string()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".studypulse-write-{}.tmp", Uuid::new_v4()));
    {
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
    fn rejects_symbolic_link_escape() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let workspace = Workspace::create(temp.path().join("Study")).unwrap();
        symlink(outside.path(), workspace.root().join("Documents/outside")).unwrap();
        assert!(workspace.resolve_existing("Documents/outside").is_err());
    }

    #[test]
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
