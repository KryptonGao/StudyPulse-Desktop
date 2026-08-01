use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub id: String,
    pub root_path: String,
    pub schema_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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
    pub fn validate(&self) -> crate::Result<()> {
        if self.title.trim().is_empty() {
            return Err(crate::WorkspaceError::MalformedData {
                path: "Agent/notebooks.json".into(),
                detail: "notebook title must not be empty".into(),
            });
        }
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
pub enum AgentMessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    pub id: Uuid,
    pub role: AgentMessageRole,
    pub content: String,
    pub created_at: String,
}

impl AgentMessage {
    fn validate(&self) -> crate::Result<()> {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskType {
    Homework,
    Reading,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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
    3
}

impl TaskItem {
    pub fn validate(&self) -> crate::Result<()> {
        if self.title.trim().is_empty() {
            return Err(crate::WorkspaceError::MalformedData {
                path: "Data/tasks.jsonl".into(),
                detail: "task title must not be empty".into(),
            });
        }
        if !(1..=5).contains(&self.importance) {
            return Err(crate::WorkspaceError::MalformedData {
                path: "Data/tasks.jsonl".into(),
                detail: "importance must be between 1 and 5".into(),
            });
        }
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
    1
}

/// A daily reflection entry compatible with the iOS `DiaryEntry` payload.
/// Multiple entries may share the same calendar date; analytics aggregates
/// their mood and energy values per day.
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
    3
}

impl DiaryEntry {
    pub fn validate(&self) -> crate::Result<()> {
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
pub struct ComprehensiveExam {
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Canonical iOS-compatible study domain values. Dates deliberately cross the
/// persistence and FFI boundaries as validated RFC3339 strings so the same
/// JSONL files can be consumed by Rust, Swift and the future Windows client.
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
    3
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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
pub struct ExamTimeSlot {
    pub start_time: String,
    pub end_time: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RoutineType {
    MistakeReview,
    Flashcard,
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionIntensity {
    Peak,
    DeepFocus,
    Steady,
    Light,
    Recovery,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudySessionSource {
    #[default]
    Timer,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HeartRateSample {
    pub id: Uuid,
    pub timestamp: String,
    pub bpm: f64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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
pub enum InvestmentTarget {
    Subject(Uuid),
    SubTask(Uuid),
}

impl Serialize for InvestmentTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Associated<'a> {
            #[serde(rename = "_0")]
            id: &'a Uuid,
        }
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            Self::Subject(id) => map.serialize_entry("subject", &Associated { id })?,
            Self::SubTask(id) => map.serialize_entry("subTask", &Associated { id })?,
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for InvestmentTarget {
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
            let value = value
                .get("_0")
                .and_then(Value::as_str)
                .or_else(|| value.as_str())
                .ok_or_else(|| E::custom("investment target UUID is missing"))?;
            value
                .parse()
                .map_err(|_| E::custom("investment target UUID is invalid"))
        }

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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
pub enum TimeInvestmentTheme {
    Ocean,
    Coral,
    Violet,
    Sunshine,
    Mint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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
pub struct FileEntry {
    pub relative_path: String,
    pub is_directory: bool,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub relative_path: String,
    pub line_number: Option<u32>,
    pub snippet: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
