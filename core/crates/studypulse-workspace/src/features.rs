use std::collections::{BTreeMap, HashSet};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{TaskItem, Workspace, WorkspaceError};

fn now_string() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn required(value: &str, field: &str) -> crate::Result<()> {
    if value.trim().is_empty() {
        return Err(WorkspaceError::MalformedData {
            path: "Data/feature.jsonl".into(),
            detail: format!("{field} must not be empty"),
        });
    }
    Ok(())
}

fn string_or_default<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CoachGoalStatus {
    Active,
    Paused,
    Achieved,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoachGoalSubject {
    pub id: Uuid,
    pub subject: String,
    pub baseline_score: f64,
    pub target_score: f64,
    pub full_score: f64,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoachGoal {
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub subjects: Vec<CoachGoalSubject>,
    #[serde(default, alias = "examID")]
    pub exam_id: Option<Uuid>,
    #[serde(default, alias = "comprehensiveExamID")]
    pub comprehensive_exam_id: Option<Uuid>,
    pub start_date: String,
    pub target_date: String,
    pub daily_available_minutes: i64,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub constraints: String,
    pub status: CoachGoalStatus,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl CoachGoal {
    pub fn validate(&self) -> crate::Result<()> {
        required(&self.title, "goal title")?;
        required(&self.start_date, "startDate")?;
        required(&self.target_date, "targetDate")?;
        if self.daily_available_minutes <= 0 || self.daily_available_minutes > 24 * 60 {
            return Err(WorkspaceError::MalformedData {
                path: "Data/coach_data.jsonl".into(),
                detail: "dailyAvailableMinutes must be between 1 and 1440".into(),
            });
        }
        if self.version < 1 || self.subjects.is_empty() {
            return Err(WorkspaceError::MalformedData {
                path: "Data/coach_data.jsonl".into(),
                detail: "coach goal must have a version and at least one subject".into(),
            });
        }
        for subject in &self.subjects {
            required(&subject.subject, "subject")?;
            if subject.full_score <= 0.0 || subject.weight < 0.0 {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/coach_data.jsonl".into(),
                    detail: "subject fullScore must be positive and weight non-negative".into(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoachRisk {
    pub id: Uuid,
    pub title: String,
    pub severity: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoachEvidence {
    pub source: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoachPrediction {
    pub subject: String,
    pub predicted: f64,
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub target_score: f64,
    pub confidence: f64,
    pub sample_size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoachAnalysis {
    pub id: Uuid,
    #[serde(alias = "goalID")]
    pub goal_id: Uuid,
    pub goal_version: i64,
    pub calculated_at: String,
    pub decision: String,
    pub weighted_predicted: f64,
    pub weighted_lower_bound: f64,
    pub weighted_upper_bound: f64,
    pub success_probability: f64,
    #[serde(default)]
    pub predictions: Vec<CoachPrediction>,
    #[serde(default)]
    pub risks: Vec<Value>,
    #[serde(default)]
    pub evidence: Vec<Value>,
    #[serde(default)]
    pub data_fingerprint: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl CoachAnalysis {
    pub fn validate(&self) -> crate::Result<()> {
        if self.goal_version < 1 || !(0.0..=1.0).contains(&self.success_probability) {
            return Err(WorkspaceError::MalformedData {
                path: "Data/coach_data.jsonl".into(),
                detail: "coach analysis version or probability is invalid".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CoachProposalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
    Superseded,
}

/// iOS stores the full stop-condition object while the first desktop Agent
/// contract also allowed a short text. Untagged decoding keeps both forms
/// round-trippable without weakening the enclosing proposal validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum CoachStopCondition {
    Text(String),
    Structured(Value),
}

impl Default for CoachStopCondition {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl CoachStopCondition {
    fn as_text(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Structured(value) => value.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoachProposalItem {
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub subject: String,
    pub start_date: String,
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub stop_condition: CoachStopCondition,
    pub importance: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoachProposal {
    pub id: Uuid,
    #[serde(alias = "goalID")]
    pub goal_id: Uuid,
    pub goal_version: i64,
    #[serde(alias = "analysisID")]
    pub analysis_id: Uuid,
    pub conclusion: String,
    pub rationale: String,
    #[serde(default)]
    pub items: Vec<CoachProposalItem>,
    pub status: CoachProposalStatus,
    pub created_at: String,
    pub expires_at: String,
    #[serde(default)]
    pub resolved_at: Option<String>,
    #[serde(default)]
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub alternative: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl CoachProposal {
    pub fn validate(&self) -> crate::Result<()> {
        required(&self.conclusion, "proposal conclusion")?;
        if self.goal_version < 1 {
            return Err(WorkspaceError::MalformedData {
                path: "Data/coach_data.jsonl".into(),
                detail: "proposal goalVersion must be positive".into(),
            });
        }
        for item in &self.items {
            required(&item.title, "proposal item title")?;
            if !(1..=5).contains(&item.importance) {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/coach_data.jsonl".into(),
                    detail: "proposal item importance must be between 1 and 5".into(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoachChat {
    pub id: Uuid,
    #[serde(default, alias = "goalID")]
    pub goal_id: Option<Uuid>,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoachConversationMessage {
    pub id: Uuid,
    #[serde(default, alias = "goalID")]
    pub goal_id: Option<Uuid>,
    #[serde(alias = "chatID")]
    pub chat_id: Uuid,
    pub role: String,
    pub content: String,
    pub created_at: String,
    #[serde(default)]
    pub todo_suggestions: Vec<CoachProposalItem>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoachDataRow {
    pub kind: String,
    pub payload: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoachData {
    pub goals: Vec<CoachGoal>,
    pub analyses: Vec<CoachAnalysis>,
    pub proposals: Vec<CoachProposal>,
    pub chats: Vec<CoachChat>,
    pub messages: Vec<CoachConversationMessage>,
    pub unknown_rows: Vec<Value>,
    pub row_extras: BTreeMap<String, BTreeMap<String, Value>>,
}

impl CoachData {
    pub fn validate(&self) -> crate::Result<()> {
        let goal_ids: HashSet<_> = self.goals.iter().map(|value| value.id).collect();
        let proposal_ids: HashSet<_> = self.proposals.iter().map(|value| value.id).collect();
        for goal in &self.goals {
            goal.validate()?;
        }
        for analysis in &self.analyses {
            analysis.validate()?;
            if !goal_ids.contains(&analysis.goal_id) {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/coach_data.jsonl".into(),
                    detail: format!("analysis references missing goal {}", analysis.goal_id),
                });
            }
        }
        for proposal in &self.proposals {
            proposal.validate()?;
            if !goal_ids.contains(&proposal.goal_id) {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/coach_data.jsonl".into(),
                    detail: format!("proposal references missing goal {}", proposal.goal_id),
                });
            }
            if proposal.status == CoachProposalStatus::Approved && proposal.items.is_empty() {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/coach_data.jsonl".into(),
                    detail: "approved proposal must contain task items".into(),
                });
            }
        }
        for chat in &self.chats {
            if let Some(goal_id) = chat.goal_id
                && !goal_ids.contains(&goal_id)
            {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/coach_data.jsonl".into(),
                    detail: format!("chat references missing goal {goal_id}"),
                });
            }
        }
        for message in &self.messages {
            if !self.chats.iter().any(|chat| chat.id == message.chat_id) {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/coach_data.jsonl".into(),
                    detail: format!("message references missing chat {}", message.chat_id),
                });
            }
        }
        let _ = proposal_ids;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExamGoal {
    pub id: Uuid,
    pub exam_name: String,
    pub subject: String,
    pub exam_date: String,
    pub current_score: f64,
    pub target_score: f64,
    pub full_score: f64,
    #[serde(default)]
    pub phase_id: Option<Uuid>,
    pub created_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl ExamGoal {
    pub fn validate(&self) -> crate::Result<()> {
        required(&self.exam_name, "examName")?;
        required(&self.subject, "subject")?;
        if self.full_score <= 0.0
            || self.current_score < 0.0
            || self.current_score > self.full_score
            || self.target_score < 0.0
            || self.target_score > self.full_score
        {
            return Err(WorkspaceError::MalformedData {
                path: "Data/exam_goals.jsonl".into(),
                detail: "exam score bounds are invalid".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExamWeakPoint {
    pub id: Uuid,
    pub topic: String,
    pub mastery: f64,
    pub possible_score_gain: f64,
    pub priority: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExamPlanPhase {
    pub id: Uuid,
    pub name: String,
    pub day_range: String,
    pub goal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DailyExamTask {
    pub id: Uuid,
    pub day_offset: i64,
    pub date: String,
    pub subject: String,
    pub duration_minutes: i64,
    pub task_title: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExamPlan {
    pub id: Uuid,
    #[serde(alias = "examGoalID")]
    pub exam_goal_id: Uuid,
    pub improvement_target: f64,
    pub summary: String,
    #[serde(default)]
    pub weak_points: Vec<ExamWeakPoint>,
    #[serde(default)]
    pub phases: Vec<ExamPlanPhase>,
    #[serde(default)]
    pub daily_tasks: Vec<DailyExamTask>,
    #[serde(default, deserialize_with = "string_or_default")]
    pub model_info: String,
    pub created_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl ExamPlan {
    pub fn validate(&self) -> crate::Result<()> {
        required(&self.summary, "plan summary")?;
        for point in &self.weak_points {
            if !(0.0..=1.0).contains(&point.mastery)
                || point.possible_score_gain < 0.0
                || point.priority < 1
            {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/exam_plans.jsonl".into(),
                    detail: "weak point bounds are invalid".into(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExamSimulationStatus {
    Preparing,
    Running,
    Grading,
    Analyzing,
    Completed,
    Abandoned,
    AnalysisFailed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExamQuestionKind {
    MultipleChoice,
    FreeResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExamQuestion {
    pub id: Uuid,
    pub kind: ExamQuestionKind,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub correct_answer: Option<String>,
    #[serde(default)]
    pub explanation: String,
    pub points: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExamQuestionRecord {
    pub question_id: Uuid,
    pub first_viewed_at: Option<String>,
    pub last_viewed_at: Option<String>,
    pub total_view_seconds: i64,
    pub visit_count: i64,
    pub skip_count: i64,
    pub answer_change_count: i64,
    pub first_answer: Option<String>,
    pub final_answer: Option<String>,
    pub submitted_at: Option<String>,
    pub is_correct: Option<bool>,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExamSimulationEventKind {
    Started,
    QuestionEntered,
    QuestionLeft,
    AnswerChanged,
    Skipped,
    Submitted,
    TimedOut,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExamSimulationEvent {
    pub id: Uuid,
    pub kind: ExamSimulationEventKind,
    pub timestamp: String,
    pub question_id: Option<Uuid>,
    pub question_index: Option<i64>,
    pub previous_answer: Option<String>,
    pub answer: Option<String>,
    pub remaining_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExamRoleAnalysis {
    pub role: String,
    pub confidence: f64,
    pub evidence: Vec<String>,
    pub risk: String,
    pub strategies: Vec<String>,
    pub is_stable: bool,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExamSimulation {
    pub id: Uuid,
    pub subject: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub duration_seconds: i64,
    pub status: ExamSimulationStatus,
    #[serde(default)]
    pub questions: Vec<ExamQuestion>,
    #[serde(default)]
    pub question_records: Vec<ExamQuestionRecord>,
    #[serde(default)]
    pub events: Vec<ExamSimulationEvent>,
    pub total_score: Option<f64>,
    pub analysis: Option<ExamRoleAnalysis>,
    pub last_error: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl ExamSimulation {
    pub fn validate(&self) -> crate::Result<()> {
        required(&self.subject, "subject")?;
        if self.duration_seconds <= 0 || self.duration_seconds > 24 * 60 * 60 {
            return Err(WorkspaceError::MalformedData {
                path: "Data/exam_simulations.jsonl".into(),
                detail: "durationSeconds is out of bounds".into(),
            });
        }
        if self.questions.len() > 100 || self.question_records.len() > 100 {
            return Err(WorkspaceError::MalformedData {
                path: "Data/exam_simulations.jsonl".into(),
                detail: "simulation contains too many questions".into(),
            });
        }
        if !self.questions.is_empty() && self.questions.len() != 10 {
            return Err(WorkspaceError::MalformedData {
                path: "Data/exam_simulations.jsonl".into(),
                detail: "a generated simulation must contain exactly 10 questions".into(),
            });
        }
        let question_ids: HashSet<_> = self.questions.iter().map(|value| value.id).collect();
        for question in &self.questions {
            required(&question.prompt, "question prompt")?;
            if question.points <= 0.0 {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/exam_simulations.jsonl".into(),
                    detail: "question points must be positive".into(),
                });
            }
            if question.kind == ExamQuestionKind::MultipleChoice && question.options.len() < 2 {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/exam_simulations.jsonl".into(),
                    detail: "multiple-choice questions need at least two options".into(),
                });
            }
        }
        for record in &self.question_records {
            if !question_ids.contains(&record.question_id) {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/exam_simulations.jsonl".into(),
                    detail: format!(
                        "answer record references missing question {}",
                        record.question_id
                    ),
                });
            }
        }
        if matches!(
            self.status,
            ExamSimulationStatus::Completed | ExamSimulationStatus::AnalysisFailed
        ) && self.ended_at.is_none()
        {
            return Err(WorkspaceError::MalformedData {
                path: "Data/exam_simulations.jsonl".into(),
                detail: "completed simulation must have endedAt".into(),
            });
        }
        if let Some(analysis) = &self.analysis
            && (!(0.0..=1.0).contains(&analysis.confidence)
                || !(2..=4).contains(&analysis.evidence.len())
                || !(2..=4).contains(&analysis.strategies.len()))
        {
            return Err(WorkspaceError::MalformedData {
                path: "Data/exam_simulations.jsonl".into(),
                detail: "exam behavior analysis bounds are invalid".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoachAnalysisResponse {
    pub conclusion: String,
    pub rationale: String,
    pub should_continue: bool,
    #[serde(default)]
    pub items: Vec<CoachProposalItem>,
    #[serde(default)]
    pub alternative: Option<String>,
    #[serde(default)]
    pub decision: String,
    #[serde(default)]
    pub weighted_predicted: f64,
    #[serde(default)]
    pub weighted_lower_bound: f64,
    #[serde(default)]
    pub weighted_upper_bound: f64,
    #[serde(default)]
    pub success_probability: f64,
    #[serde(default)]
    pub predictions: Vec<CoachPrediction>,
    #[serde(default)]
    pub risks: Vec<Value>,
    #[serde(default)]
    pub evidence: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExamGradeResponse {
    pub total_score: f64,
    pub analysis: ExamRoleAnalysis,
    #[serde(default)]
    pub question_results: Vec<ExamQuestionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExamQuestionResult {
    pub question_id: Uuid,
    pub is_correct: bool,
    pub score: f64,
    #[serde(default)]
    pub feedback: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReversePlannerResponse {
    pub improvement_target: f64,
    pub summary: String,
    pub weak_points: Vec<ExamWeakPoint>,
    pub phases: Vec<ExamPlanPhase>,
    pub daily_tasks: Vec<DailyExamTask>,
    #[serde(default)]
    pub model_info: String,
}

pub fn parse_structured_json<T: for<'de> Deserialize<'de>>(raw: &str) -> crate::Result<T> {
    let value = raw.trim();
    let value = if let Some(stripped) = value.strip_prefix("```json") {
        stripped.strip_suffix("```").unwrap_or(stripped).trim()
    } else if let Some(stripped) = value.strip_prefix("```") {
        stripped.strip_suffix("```").unwrap_or(stripped).trim()
    } else {
        value
    };
    serde_json::from_str(value).map_err(|error| WorkspaceError::MalformedData {
        path: "Agent/structured-output.json".into(),
        detail: format!("structured model output is invalid JSON: {error}"),
    })
}

pub fn default_simulation(subject: String, now: Option<String>) -> ExamSimulation {
    ExamSimulation {
        id: Uuid::new_v4(),
        subject,
        created_at: now.unwrap_or_else(now_string),
        started_at: None,
        ended_at: None,
        duration_seconds: 20 * 60,
        status: ExamSimulationStatus::Preparing,
        questions: Vec::new(),
        question_records: Vec::new(),
        events: Vec::new(),
        total_score: None,
        analysis: None,
        last_error: None,
        extra: BTreeMap::new(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DailyReportPoint {
    pub date: String,
    pub study_minutes: i64,
    pub session_count: i64,
    pub mood_score: Option<f64>,
    pub energy_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearningReport {
    pub range_days: i64,
    pub from_date: String,
    pub to_date: String,
    pub total_study_minutes: i64,
    pub session_count: i64,
    pub average_session_minutes: f64,
    pub subject_distribution: BTreeMap<String, i64>,
    pub intensity_distribution: BTreeMap<String, i64>,
    pub grade_count: i64,
    pub average_score_rate: Option<f64>,
    pub mistake_count: i64,
    pub exam_count: i64,
    pub top_subject: Option<String>,
    pub weakest_subject: Option<String>,
    pub daily_study_minutes: Vec<DailyReportPoint>,
    pub diary_count: i64,
    pub average_mood_score: Option<f64>,
    pub average_energy_score: Option<f64>,
}

pub fn learning_report(workspace: &Workspace, range_days: i64) -> crate::Result<LearningReport> {
    let days = range_days.clamp(1, 366);
    let end = Utc::now().date_naive();
    let start = end - chrono::Days::new((days - 1) as u64);
    let from_date = start.to_string();
    let to_date = end.to_string();
    let from_date_for_filter = from_date.clone();
    let to_date_for_filter = to_date.clone();
    let in_range = |value: &str| {
        let key = value.get(..10).unwrap_or(value);
        key >= from_date_for_filter.as_str() && key <= to_date_for_filter.as_str()
    };
    let sessions = workspace.read_study_sessions()?;
    let grades = workspace.read_grades()?;
    let mistakes = workspace.read_mistakes()?;
    let exams = workspace.read_exams()?;
    let comprehensive_exams = workspace.read_comprehensive_exams()?;
    let diaries = workspace.read_diary_entries()?;
    let mut daily: BTreeMap<String, DailyReportPoint> = (0..days)
        .map(|offset| {
            let date = (start + chrono::Days::new(offset as u64)).to_string();
            (
                date.clone(),
                DailyReportPoint {
                    date,
                    study_minutes: 0,
                    session_count: 0,
                    mood_score: None,
                    energy_score: None,
                },
            )
        })
        .collect();
    let mut total_seconds = 0;
    let mut intensity_distribution = BTreeMap::new();
    for session in sessions.iter().filter(|value| in_range(&value.start_date)) {
        let key = session
            .start_date
            .get(..10)
            .unwrap_or(&session.start_date)
            .to_owned();
        if let Some(point) = daily.get_mut(&key) {
            point.study_minutes += session.duration_seconds.max(0) / 60;
            point.session_count += 1;
        }
        total_seconds += session.duration_seconds.max(0);
        let intensity = format!("{:?}", session.intensity);
        *intensity_distribution.entry(intensity).or_insert(0) += 1;
    }
    let mut subject_distribution = BTreeMap::new();
    let mut score_sum = 0.0;
    let mut score_count = 0;
    for grade in grades.iter().filter(|value| in_range(&value.date)) {
        *subject_distribution
            .entry(grade.subject.clone())
            .or_insert(0) += 1;
        if let Some(full_score) = grade.full_score.filter(|value| *value > 0.0) {
            score_sum += grade.score / full_score * 100.0;
            score_count += 1;
        }
    }
    for mistake in mistakes.iter().filter(|value| in_range(&value.date)) {
        if !mistake.subject.is_empty() {
            *subject_distribution
                .entry(mistake.subject.clone())
                .or_insert(0) += 1;
        }
    }
    let mut mood_sum = 0.0;
    let mut energy_sum = 0.0;
    let mut diary_count = 0;
    for diary in diaries.iter().filter(|value| in_range(&value.date)) {
        let key = diary.date.get(..10).unwrap_or(&diary.date);
        if let Some(point) = daily.get_mut(key) {
            point.mood_score = Some(point.mood_score.unwrap_or(0.0) + diary.mood_score as f64);
            point.energy_score =
                Some(point.energy_score.unwrap_or(0.0) + diary.energy_score as f64);
        }
        mood_sum += diary.mood_score as f64;
        energy_sum += diary.energy_score as f64;
        diary_count += 1;
    }
    for point in daily.values_mut() {
        let count = diaries
            .iter()
            .filter(|value| {
                value.date.get(..10) == Some(point.date.as_str()) && in_range(&value.date)
            })
            .count();
        if count > 0 {
            point.mood_score = point.mood_score.map(|value| value / count as f64);
            point.energy_score = point.energy_score.map(|value| value / count as f64);
        }
    }
    let top_subject = subject_distribution
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(name, _)| name.clone());
    let weakest_subject = grades
        .iter()
        .filter(|value| in_range(&value.date))
        .filter_map(|grade| {
            grade
                .full_score
                .map(|full| (grade.subject.clone(), grade.score / full))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(name, _)| name);
    Ok(LearningReport {
        range_days: days,
        from_date,
        to_date,
        total_study_minutes: total_seconds / 60,
        session_count: sessions
            .iter()
            .filter(|value| in_range(&value.start_date))
            .count() as i64,
        average_session_minutes: if total_seconds > 0 {
            total_seconds as f64
                / 60.0
                / sessions
                    .iter()
                    .filter(|value| in_range(&value.start_date))
                    .count()
                    .max(1) as f64
        } else {
            0.0
        },
        subject_distribution,
        intensity_distribution,
        grade_count: grades.iter().filter(|value| in_range(&value.date)).count() as i64,
        average_score_rate: (score_count > 0).then_some(score_sum / score_count as f64),
        mistake_count: mistakes
            .iter()
            .filter(|value| in_range(&value.date))
            .count() as i64,
        exam_count: exams
            .iter()
            .filter(|value| in_range(&value.exam_date))
            .count() as i64
            + comprehensive_exams
                .iter()
                .filter(|value| in_range(&value.exam_date))
                .count() as i64,
        top_subject,
        weakest_subject,
        daily_study_minutes: daily.into_values().collect(),
        diary_count,
        average_mood_score: (diary_count > 0).then_some(mood_sum / diary_count as f64),
        average_energy_score: (diary_count > 0).then_some(energy_sum / diary_count as f64),
    })
}

pub fn coach_row_payload<T: Serialize>(kind: &str, value: &T) -> crate::Result<Value> {
    let payload = serde_json::to_vec(value)?;
    Ok(serde_json::json!({ "kind": kind, "payload": STANDARD.encode(payload) }))
}

pub fn decode_coach_payload<T: for<'de> Deserialize<'de>>(row: &CoachDataRow) -> crate::Result<T> {
    let bytes = STANDARD
        .decode(&row.payload)
        .map_err(|error| WorkspaceError::MalformedData {
            path: "Data/coach_data.jsonl".into(),
            detail: format!("coach payload is not valid Base64: {error}"),
        })?;
    serde_json::from_slice(&bytes).map_err(|error| WorkspaceError::MalformedData {
        path: "Data/coach_data.jsonl".into(),
        detail: format!("coach payload is invalid JSON: {error}"),
    })
}

pub fn make_coach_row<T: Serialize>(kind: &str, value: &T) -> crate::Result<CoachDataRow> {
    let payload = serde_json::to_vec(value)?;
    Ok(CoachDataRow {
        kind: kind.into(),
        payload: STANDARD.encode(payload),
        extra: BTreeMap::new(),
    })
}

pub fn validate_report_range(range_days: i64) -> crate::Result<()> {
    if !(1..=366).contains(&range_days) {
        return Err(WorkspaceError::MalformedData {
            path: "report".into(),
            detail: "report range must be between 1 and 366 days".into(),
        });
    }
    Ok(())
}

pub fn expired(expires_at: &str) -> bool {
    DateTime::parse_from_rfc3339(expires_at)
        .map(|value| value.with_timezone(&Utc) < Utc::now())
        .unwrap_or(true)
}

pub fn proposal_task(
    proposal: &CoachProposal,
    item: &CoachProposalItem,
) -> crate::Result<TaskItem> {
    let due = if item.start_date.contains('T') {
        item.start_date.clone()
    } else {
        format!("{}T09:00:00Z", item.start_date)
    };
    let stop_condition = item.stop_condition.as_text();
    let execution = serde_json::json!({ "objective": item.objective, "stopCondition": stop_condition, "coachGoalId": proposal.goal_id, "coachProposalId": proposal.id });
    Ok(TaskItem {
        id: Uuid::new_v4(),
        title: item.title.clone(),
        task_type: crate::TaskType::Homework,
        due_date: due.clone(),
        reminder_date: due,
        subject: item.subject.clone(),
        importance: item.importance.clamp(1, 5),
        notes: item.objective.clone(),
        is_completed: false,
        reminder_event_id: None,
        reminder_calendar_id: None,
        created_at: now_string(),
        phase_id: None,
        coach_execution_data: Some(STANDARD.encode(serde_json::to_vec(&execution)?)),
        coach_goal_id: Some(proposal.goal_id),
        coach_proposal_id: Some(proposal.id),
        extra: BTreeMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn coach_rows_keep_ios_base64_payload_shape() {
        let goal = CoachGoal {
            id: Uuid::new_v4(),
            title: "Reach algebra target".into(),
            subjects: vec![CoachGoalSubject {
                id: Uuid::new_v4(),
                subject: "Algebra".into(),
                baseline_score: 60.0,
                target_score: 80.0,
                full_score: 100.0,
                weight: 1.0,
            }],
            exam_id: None,
            comprehensive_exam_id: None,
            start_date: "2026-08-01".into(),
            target_date: "2026-09-01".into(),
            daily_available_minutes: 45,
            purpose: "Improve fundamentals".into(),
            constraints: "Weekdays only".into(),
            status: CoachGoalStatus::Active,
            version: 1,
            created_at: now_string(),
            updated_at: now_string(),
            extra: BTreeMap::new(),
        };
        let row = make_coach_row("goal", &goal).unwrap();
        assert_eq!(row.kind, "goal");
        assert!(!row.payload.contains("Reach algebra"));
        assert_eq!(decode_coach_payload::<CoachGoal>(&row).unwrap(), goal);
    }

    #[test]
    fn structured_json_rejects_non_json_and_accepts_fenced_json() {
        let parsed: CoachAnalysisResponse = parse_structured_json("```json\n{\"conclusion\":\"ok\",\"rationale\":\"evidence\",\"shouldContinue\":true}\n```").unwrap();
        assert_eq!(parsed.conclusion, "ok");
        assert!(parse_structured_json::<CoachAnalysisResponse>("not json").is_err());
    }

    #[test]
    fn ios_coach_id_coding_keys_are_accepted() {
        let exam_id = Uuid::new_v4();
        let comprehensive_id = Uuid::new_v4();
        let goal: CoachGoal = serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(),
            "title": "Target",
            "subjects": [{"id": Uuid::new_v4(), "subject": "Math", "baselineScore": 50.0, "targetScore": 80.0, "fullScore": 100.0, "weight": 1.0}],
            "examID": exam_id,
            "comprehensiveExamID": comprehensive_id,
            "startDate": "2026-08-01T00:00:00Z",
            "targetDate": "2026-09-01T00:00:00Z",
            "dailyAvailableMinutes": 60,
            "status": "active",
            "version": 1,
            "createdAt": "2026-08-01T00:00:00Z",
            "updatedAt": "2026-08-01T00:00:00Z"
        })).unwrap();
        assert_eq!(goal.exam_id, Some(exam_id));
        assert_eq!(goal.comprehensive_exam_id, Some(comprehensive_id));
    }

    #[test]
    fn simulation_defaults_to_twenty_minutes() {
        let simulation = default_simulation("Physics".into(), Some("2026-08-02T00:00:00Z".into()));
        assert_eq!(simulation.duration_seconds, 1_200);
        assert_eq!(simulation.status, ExamSimulationStatus::Preparing);
        assert!(simulation.questions.is_empty());
    }

    #[test]
    fn empty_report_has_stable_daily_boundaries() {
        let temp = tempdir().unwrap();
        let workspace = Workspace::create(temp.path()).unwrap();
        let report = learning_report(&workspace, 7).unwrap();
        assert_eq!(report.range_days, 7);
        assert_eq!(report.daily_study_minutes.len(), 7);
        assert_eq!(report.session_count, 0);
        assert_eq!(report.average_score_rate, None);
    }
}
