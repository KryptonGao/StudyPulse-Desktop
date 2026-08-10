//! Serializable Workspace models and their boundary validation.
//!
//! These structs mirror the shared iOS wire shape: Rust names are mapped to
//! camelCase, dates remain RFC3339 strings, and unknown object keys are kept in
//! `extra`. Defaults preserve older files; they do not weaken new-record rules.
use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;
use uuid::Uuid;

// Agent notebooks are a separate pretty-JSON index because conversation order
// and selected sources are user-facing metadata rather than record streams.
// Their source paths are validated against the current library at write time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Identifies the canonical root and schema accepted by a Workspace handle.
/// The path is informational; filesystem operations use the canonical root
/// held by `Workspace` rather than trusting this serialized value.
pub struct WorkspaceInfo {
    pub id: String,
    pub root_path: String,
    pub schema_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Notebook metadata plus the source allow-list used by Agent reads.
/// Missing conversation fields default to empty values for legacy notebooks.
pub struct AgentNotebook {
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub source_paths: Vec<String>,
    #[serde(default)]
    pub messages: Vec<AgentMessage>,
    #[serde(default)]
    pub last_goal: String,
    #[serde(default)]
    pub last_answer: String,
    pub updated_at: String,
}

impl AgentNotebook {
    /// Validate notebook content and nested messages before persistence.
    /// Parsing dates at the boundary keeps later sorting and analytics strict.
    pub fn validate(&self) -> crate::Result<()> {
        // Empty titles make Notebook selection ambiguous in the UI.
        if self.title.trim().is_empty() {
            return Err(crate::WorkspaceError::MalformedData {
                path: "Agent/notebooks.json".into(),
                detail: "notebook title must not be empty".into(),
            });
        }
        // Notebook index ordering depends on a parseable update timestamp.
        chrono::DateTime::parse_from_rfc3339(&self.updated_at).map_err(|error| {
            crate::WorkspaceError::MalformedData {
                path: "Agent/notebooks.json".into(),
                detail: format!("invalid updatedAt: {error}"),
            }
        })?;
        for message in &self.messages {
            message.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// The two roles persisted in a notebook conversation.
pub enum AgentMessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// One immutable notebook message with a stable UUID and creation timestamp.
/// Tool events remain in Agent run logs rather than this user-facing history.
pub struct AgentMessage {
    pub id: Uuid,
    pub role: AgentMessageRole,
    pub content: String,
    pub created_at: String,
}

impl AgentMessage {
    /// Reject empty content and non-RFC3339 creation timestamps.
    fn validate(&self) -> crate::Result<()> {
        // Empty messages are not useful transcript entries.
        if self.content.trim().is_empty() {
            return Err(crate::WorkspaceError::MalformedData {
                path: "Agent/notebooks.json".into(),
                detail: "message content must not be empty".into(),
            });
        }
        chrono::DateTime::parse_from_rfc3339(&self.created_at).map_err(|error| {
            crate::WorkspaceError::MalformedData {
                path: "Agent/notebooks.json".into(),
                detail: format!("invalid message createdAt: {error}"),
            }
        })?;
        Ok(())
    }
}

// Tasks remain a JSONL domain so completion changes can target one UUID and
// preserve unknown fields from another client through the shared envelope.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// The storage-level task category shared with the iOS client.
pub enum TaskType {
    Homework,
    Reading,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// A task record stored as the `value` inside `Data/tasks.jsonl`.
/// Optional coach references connect generated tasks without changing the
/// base task shape, while `extra` preserves newer client fields.
pub struct TaskItem {
    pub id: Uuid,
    pub title: String,
    #[serde(rename = "type")]
    pub task_type: TaskType,
    pub due_date: String,
    pub reminder_date: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default = "default_importance")]
    pub importance: u8,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub is_completed: bool,
    #[serde(default)]
    pub reminder_event_id: Option<String>,
    #[serde(default)]
    pub reminder_calendar_id: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub phase_id: Option<Uuid>,
    #[serde(default)]
    pub coach_execution_data: Option<String>,
    #[serde(default)]
    pub coach_goal_id: Option<Uuid>,
    #[serde(default)]
    pub coach_proposal_id: Option<Uuid>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

const fn default_importance() -> u8 {
    // Existing records without this field receive the neutral product default.
    3
}

impl TaskItem {
    /// Validate scheduling fields and the Base64 coach payload at the boundary.
    pub fn validate(&self) -> crate::Result<()> {
        // A blank title would create an unresolvable task in every client.
        if self.title.trim().is_empty() {
            return Err(crate::WorkspaceError::MalformedData {
                path: "Data/tasks.jsonl".into(),
                detail: "task title must not be empty".into(),
            });
        }
        // Importance is a five-point UI scale, not an arbitrary integer.
        if !(1..=5).contains(&self.importance) {
            return Err(crate::WorkspaceError::MalformedData {
                path: "Data/tasks.jsonl".into(),
                detail: "importance must be between 1 and 5".into(),
            });
        }
        // All three scheduling timestamps must parse before the task can be
        // sorted or handed to a calendar integration.
        chrono::DateTime::parse_from_rfc3339(&self.due_date).map_err(|error| {
            crate::WorkspaceError::MalformedData {
                path: "Data/tasks.jsonl".into(),
                detail: format!("invalid dueDate: {error}"),
            }
        })?;
        chrono::DateTime::parse_from_rfc3339(&self.reminder_date).map_err(|error| {
            crate::WorkspaceError::MalformedData {
                path: "Data/tasks.jsonl".into(),
                detail: format!("invalid reminderDate: {error}"),
            }
        })?;
        chrono::DateTime::parse_from_rfc3339(&self.created_at).map_err(|error| {
            crate::WorkspaceError::MalformedData {
                path: "Data/tasks.jsonl".into(),
                detail: format!("invalid createdAt: {error}"),
            }
        })?;
        if let Some(data) = &self.coach_execution_data {
            STANDARD
                .decode(data)
                .map_err(|error| crate::WorkspaceError::MalformedData {
                    path: "Data/tasks.jsonl".into(),
                    detail: format!("invalid coachExecutionData Base64: {error}"),
                })?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Common JSONL envelope used by all record-oriented Workspace files.
/// The duplicated `id` enables indexing and conflict checks; readers verify it
/// matches `value.id`. Flattened extras preserve fields unknown to this build.
pub struct IosRecord<T> {
    #[serde(default = "default_dto_version")]
    pub dto_version: u32,
    pub id: Uuid,
    #[serde(default)]
    pub updated_at: Option<String>,
    pub value: T,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

const fn default_dto_version() -> u32 {
    // Version one is the original envelope format and remains the read default.
    1
}

// Diary and mistake records share the same date-compatible, iOS-readable JSONL
// storage. Analytics decides how multiple entries on one day are aggregated.
/// A daily reflection entry compatible with the iOS `DiaryEntry` payload.
/// Multiple entries may share the same calendar date; analytics aggregates
/// their mood and energy values per day.
/// Scores remain bounded integers at the model boundary so averages never
/// silently include invalid UI values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiaryEntry {
    pub id: Uuid,
    pub date: String,
    #[serde(default = "default_diary_score")]
    pub mood_score: i64,
    #[serde(default = "default_diary_score")]
    pub energy_score: i64,
    #[serde(default)]
    pub energy_tag: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub phase_id: Option<Uuid>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

const fn default_diary_score() -> i64 {
    // Neutral score keeps old diary entries centered when the field is absent.
    3
}

impl DiaryEntry {
    /// Validate score bounds and timestamps used by persistence and trends.
    pub fn validate(&self) -> crate::Result<()> {
        // Mood and energy share the five-point UI scale.
        if !(1..=5).contains(&self.mood_score) {
            return Err(crate::WorkspaceError::MalformedData {
                path: "Data/diary_entries.jsonl".into(),
                detail: "moodScore must be between 1 and 5".into(),
            });
        }
        if !(1..=5).contains(&self.energy_score) {
            return Err(crate::WorkspaceError::MalformedData {
                path: "Data/diary_entries.jsonl".into(),
                detail: "energyScore must be between 1 and 5".into(),
            });
        }
        // Validate all date fields with one parser so timezone offsets are
        // accepted consistently and errors name the exact field.
        for (field, value) in [
            ("date", &self.date),
            ("createdAt", &self.created_at),
            ("updatedAt", &self.updated_at),
        ] {
            chrono::DateTime::parse_from_rfc3339(value).map_err(|error| {
                crate::WorkspaceError::MalformedData {
                    path: "Data/diary_entries.jsonl".into(),
                    detail: format!("invalid {field}: {error}"),
                }
            })?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Minimal mistake projection used when only identity fields are needed.
pub struct MistakeNote {
    pub id: Uuid,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subject: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Minimal exam projection retained for generic record access.
pub struct Exam {
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub subject: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Minimal comprehensive-exam projection for generic record access.
pub struct ComprehensiveExam {
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

// The following study-domain values are intentionally string-date based. That
// keeps serde/FFI behavior stable across Rust, Swift, and Windows clients while
// validation remains the single place that enforces parseable timestamps.
/// Canonical iOS-compatible study domain values. Dates deliberately cross the
/// persistence and FFI boundaries as validated RFC3339 strings so the same
/// JSONL files can be consumed by Rust, Swift and the future Windows client.
/// The flattened extras are part of the compatibility contract for schema
/// additions not yet modeled by this crate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudyPhase {
    pub id: Uuid,
    pub name: String,
    pub start_date: String,
    pub end_date: String,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub archived_at: Option<String>,
    #[serde(default)]
    pub goals: Vec<PhaseGoal>,
    pub created_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// A goal attached to a study phase; the score uses the subject's scale.
pub struct PhaseGoal {
    pub id: Uuid,
    pub subject: String,
    #[serde(default)]
    pub target_score: f64,
    #[serde(default)]
    pub notes: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Subject metadata used to normalize grades and render display labels.
pub struct Subject {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub full_score: f64,
    pub display_name: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// A dated subject grade with optional exam and phase links.
/// Missing `full_score` values are resolved by analytics from the subject or
/// the conventional 100-point scale.
pub struct Grade {
    pub id: Uuid,
    pub subject: String,
    pub score: f64,
    #[serde(default)]
    pub raw_score: Option<f64>,
    #[serde(default)]
    pub ranking: Option<i64>,
    #[serde(default = "default_importance_i64")]
    pub importance: i64,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub image_file_name: Option<String>,
    pub date: String,
    #[serde(default)]
    pub exam_name: String,
    #[serde(default)]
    pub exam_id: Option<Uuid>,
    #[serde(default)]
    pub full_score: Option<f64>,
    #[serde(default)]
    pub phase_id: Option<Uuid>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

const fn default_importance_i64() -> i64 {
    // This form is used by older exam and grade records.
    // Keeping it in one const function makes serde defaults auditable.
    3
}

// Review history is nested in a mistake rather than stored as a second record
// type, allowing one wrong question to round-trip its complete SRS context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// The persisted state used by the SM-2-compatible review calculation.
/// `next_review_date` is compared as an RFC3339 instant; `extra` keeps future
/// scheduler metadata intact during an upsert.
pub struct ReviewState {
    pub repetitions: i64,
    pub ease_factor: f64,
    pub interval_days: i64,
    pub next_review_date: String,
    #[serde(default)]
    pub last_review_date: Option<String>,
    #[serde(default)]
    pub lapses: i64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// One normalized review result used to build daily activity points.
pub struct MasteryHistoryEntry {
    pub id: Uuid,
    pub timestamp: String,
    pub score: f64,
    pub quality: i64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// A handwritten answer snapshot associated with a mistake review.
/// Image data is intentionally opaque here; media/path policy belongs to the
/// Workspace boundary rather than the value model.
pub struct HandwritingAnswerEntry {
    pub id: Uuid,
    pub timestamp: String,
    pub image_data: String,
    pub quality: i64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Full wrong-question record, including optional SRS and review histories.
/// Missing collections default empty for legacy iOS payloads and all unknown
/// keys remain round-trippable through `extra`.
pub struct MistakeNoteFull {
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub subject: String,
    pub original_question: String,
    #[serde(default)]
    pub source: String,
    pub date: String,
    pub error_reason: String,
    pub wrong_solution: String,
    pub correct_solution: String,
    #[serde(default)]
    pub question_images: Vec<String>,
    #[serde(default)]
    pub reason_images: Vec<String>,
    #[serde(default)]
    pub wrong_solution_images: Vec<String>,
    #[serde(default)]
    pub correct_solution_images: Vec<String>,
    #[serde(default)]
    pub review_state: Option<ReviewState>,
    #[serde(default)]
    pub phase_id: Option<Uuid>,
    #[serde(default)]
    pub exposure_count: i64,
    #[serde(default)]
    pub mastery_score: f64,
    #[serde(default)]
    pub mastery_history: Vec<MasteryHistoryEntry>,
    #[serde(default)]
    pub handwriting_history: Vec<HandwritingAnswerEntry>,
    #[serde(default)]
    pub difficulty: i64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub audio_file_name: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// An exam's optional start/end time window.
pub struct ExamTimeSlot {
    pub start_time: String,
    pub end_time: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// One checklist item belonging to an exam.
pub struct ExamChecklistItem {
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub is_checked: bool,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Post-exam reflection, with links back to the mistakes it explains.
pub struct ExamReview {
    pub id: Uuid,
    pub reviewed_at: String,
    #[serde(default)]
    pub what_was_tested: String,
    #[serde(default)]
    pub what_went_wrong: String,
    #[serde(default)]
    pub what_learned: String,
    #[serde(default)]
    pub next_strategy: String,
    #[serde(default)]
    pub linked_mistake_ids: Vec<Uuid>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Complete subject-specific exam record used by planning and snapshots.
pub struct ExamFull {
    pub id: Uuid,
    pub name: String,
    pub exam_date: String,
    #[serde(default)]
    pub exam_end_date: Option<String>,
    #[serde(default = "default_importance_i64")]
    pub importance: i64,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub exam_name: String,
    #[serde(default)]
    pub mastery_degree: i64,
    #[serde(default)]
    pub time_slot: Option<ExamTimeSlot>,
    #[serde(default)]
    pub phase_id: Option<Uuid>,
    #[serde(default)]
    pub checklist: Vec<ExamChecklistItem>,
    #[serde(default)]
    pub location_school: String,
    #[serde(default)]
    pub location_classroom: String,
    #[serde(default)]
    pub location_seat: String,
    #[serde(default)]
    pub countdown_notify_days: Option<Vec<i64>>,
    #[serde(default)]
    pub exam_review: Option<ExamReview>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Exam record that spans several subjects and may provide per-subject slots.
pub struct ComprehensiveExamFull {
    pub id: Uuid,
    pub name: String,
    pub exam_date: String,
    #[serde(default)]
    pub exam_end_date: Option<String>,
    #[serde(default = "default_importance_i64")]
    pub importance: i64,
    #[serde(default)]
    pub subject: Vec<String>,
    #[serde(default)]
    pub exam_name: String,
    #[serde(default)]
    pub mastery_degree: i64,
    #[serde(default)]
    pub subject_time_slots: Option<BTreeMap<String, ExamTimeSlot>>,
    #[serde(default)]
    pub phase_id: Option<Uuid>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

// Routines describe recurrence; instances describe a date-specific occurrence.
// Keeping them separate prevents a schedule edit from rewriting history.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Recurrence category for a scheduled study routine.
pub enum RoutineType {
    MistakeReview,
    Flashcard,
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// User-authored recurrence definition. Instances are materialized separately
/// so editing a routine does not rewrite historical occurrences.
pub struct Routine {
    pub id: Uuid,
    pub title: String,
    pub r#type: RoutineType,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub weekdays: Vec<i64>,
    pub start_time: String,
    pub end_time: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub created_at: String,
    #[serde(default)]
    pub phase_id: Option<Uuid>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

const fn default_true() -> bool {
    // A newly decoded routine is active unless an older file says otherwise.
    // The explicit default is part of legacy-file compatibility.
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// A dated materialization of a routine, including completion state.
pub struct RoutineInstance {
    pub id: Uuid,
    pub routine_id: Uuid,
    pub title: String,
    pub r#type: RoutineType,
    #[serde(default)]
    pub subject: Option<String>,
    pub start_time: String,
    pub end_time: String,
    pub date: String,
    pub date_key: String,
    #[serde(default)]
    pub is_completed: bool,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub spawned_mistake_count: i64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

// Session values are append-like history with optional health/target metadata.
// Unknown keys remain available for clients that add richer sensor fields.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Coarse effort label captured with a study session.
pub enum SessionIntensity {
    Peak,
    DeepFocus,
    Steady,
    Light,
    Recovery,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Origin of a session; timer is the compatibility default for old records.
pub enum StudySessionSource {
    #[default]
    Timer,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Heart-rate sample attached to a session when health data is available.
pub struct HeartRateSample {
    pub id: Uuid,
    pub timestamp: String,
    pub bpm: f64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// User annotation explaining perceived difficulty during a session.
pub struct DifficultyAnnotation {
    pub id: Uuid,
    pub timestamp: String,
    #[serde(default)]
    pub heart_rate: Option<f64>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub subject_id: Option<Uuid>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
/// Polymorphic target used by time investment and goal reward records.
/// Custom serde mirrors Swift's associated-value representation, including
/// the `_0` wrapper, while accepting the legacy bare-UUID form on read.
pub enum InvestmentTarget {
    Subject(Uuid),
    SubTask(Uuid),
}

impl Serialize for InvestmentTarget {
    /// Emit the Swift-compatible tagged object rather than a Rust enum name.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Associated<'a> {
            #[serde(rename = "_0")]
            id: &'a Uuid,
        }
        // Emit exactly one tagged branch with the `_0` associated-value wrapper.
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            Self::Subject(id) => map.serialize_entry("subject", &Associated { id })?,
            Self::SubTask(id) => map.serialize_entry("subTask", &Associated { id })?,
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for InvestmentTarget {
    /// Accept both the current associated-value object and its legacy scalar
    /// UUID representation, while rejecting ambiguous two-target objects.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            subject: Option<Value>,
            #[serde(rename = "subTask")]
            sub_task: Option<Value>,
        }

        fn decode_id<E: serde::de::Error>(value: Value) -> Result<Uuid, E> {
            // Swift Codable writes `_0`; older payloads wrote the UUID directly.
            let value = value
                .get("_0")
                .and_then(Value::as_str)
                .or_else(|| value.as_str())
                .ok_or_else(|| E::custom("investment target UUID is missing"))?;
            value
                .parse()
                .map_err(|_| E::custom("investment target UUID is invalid"))
        }

        // Decode the complete object first so the exactly-one-branch rule can
        // be enforced before constructing the enum.
        let wire = Wire::deserialize(deserializer)?;
        match (wire.subject, wire.sub_task) {
            (Some(id), None) => Ok(Self::Subject(decode_id(id)?)),
            (None, Some(id)) => Ok(Self::SubTask(decode_id(id)?)),
            _ => Err(serde::de::Error::custom(
                "investment target must contain exactly one subject or subTask",
            )),
        }
    }
}

// Investment records use custom associated-value serde because that is the
// shape produced by Swift Codable for the shared enum.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// A completed or in-progress focus interval with optional health and target
/// metadata. The original timestamp and optional timezone remain separate so
/// UTC analytics can be deterministic without discarding display context.
pub struct StudySession {
    pub id: Uuid,
    pub start_date: String,
    pub duration_seconds: i64,
    pub intensity: SessionIntensity,
    pub completed: bool,
    #[serde(default)]
    pub heart_rate_samples: Option<Vec<HeartRateSample>>,
    #[serde(default)]
    pub difficulty_annotations: Option<Vec<DifficultyAnnotation>>,
    #[serde(default)]
    pub investment_target: Option<InvestmentTarget>,
    #[serde(default)]
    pub source: StudySessionSource,
    #[serde(default)]
    pub time_zone_identifier: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// Decorative theme assigned to a time-investment subject.
pub enum TimeInvestmentTheme {
    Ocean,
    Coral,
    Violet,
    Sunshine,
    Mint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Root category for time investment aggregation.
pub struct TimeInvestmentSubject {
    pub id: Uuid,
    pub name: String,
    pub symbol_name: String,
    pub theme: TimeInvestmentTheme,
    pub start_date: String,
    pub sort_order: i64,
    pub created_at: String,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Nested time-investment category; `parent_sub_task_id` forms a tree.
pub struct SubTask {
    pub id: Uuid,
    pub subject_id: Uuid,
    #[serde(default)]
    pub parent_sub_task_id: Option<Uuid>,
    pub name: String,
    pub sort_order: i64,
    pub created_at: String,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Threshold reward attached to a subject or nested investment target.
pub struct GoalReward {
    pub id: Uuid,
    pub title: String,
    pub symbol_name: String,
    pub target: InvestmentTarget,
    pub threshold_seconds: i64,
    pub created_at: String,
    #[serde(default)]
    pub unlocked_at: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Library entry exposed to the Agent and frontend, always relative to root.
pub struct FileEntry {
    pub relative_path: String,
    pub is_directory: bool,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// Bounded search result; a missing line number means the path itself matched.
pub struct SearchMatch {
    pub relative_path: String,
    pub line_number: Option<u32>,
    pub snippet: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // The custom enum serde must match Swift's associated-value object and still
    // accept the legacy bare UUID representation.
    fn investment_target_matches_swift_synthesized_codable_shape() {
        let id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let value = serde_json::to_value(InvestmentTarget::Subject(id)).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"subject": {"_0": id.to_string()}})
        );
        assert_eq!(
            serde_json::from_value::<InvestmentTarget>(value).unwrap(),
            InvestmentTarget::Subject(id)
        );
        let legacy = serde_json::json!({"subTask": id.to_string()});
        assert_eq!(
            serde_json::from_value::<InvestmentTarget>(legacy).unwrap(),
            InvestmentTarget::SubTask(id)
        );
    }
}
