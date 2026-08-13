//! Higher-level Coach, exam-simulation, and report values.
//!
//! These records still live in the Workspace's compatibility-oriented storage:
//! typed values use camelCase plus flattened extras, Coach rows are serialized
//! as Base64 JSON payloads, and validation rejects impossible links/bounds before
//! a file is rewritten.  The helpers at the bottom keep model-output parsing
//! deterministic and keep report calculations local to Workspace data.
//!
//! Coach records are intentionally heterogeneous on disk: each row has a kind,
//! a Base64 payload, and flattened row extras. The typed `CoachData` aggregate
//! is a convenience view, not a replacement format; unknown rows are preserved
//! during a write. Exam simulations use bounded collections and explicit links
//! so generated content cannot grow without limit or reference a missing item.
//!
//! Report helpers clamp caller ranges and use date prefixes only after the range
//! has been derived from UTC. They summarize local facts; they do not call an
//! AI provider or upload personal records.
use std::collections::{BTreeMap, HashSet};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{TaskItem, Workspace, WorkspaceError};

fn now_string() -> String {
    // Persist generated values in UTC with milliseconds, matching JSONL updates.
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn required(value: &str, field: &str) -> crate::Result<()> {
    // Shared non-empty check keeps validation errors consistent across Coach and
    // exam planning records while retaining the logical field name for callers.
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
    // Some early model responses emitted null for textual metadata; decode it
    // as the empty compatibility value instead of failing the whole plan.
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// Lifecycle state for a Coach goal.
pub enum CoachGoalStatus {
    Active,
    Paused,
    Achieved,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Per-subject baseline, target, scale, and contribution weight for a goal.
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
/// A versioned, date-bounded Coach objective with local planning constraints.
/// Versioning lets analyses and proposals prove which goal definition they used.
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
    /// Enforce usable goal text, a realistic daily-minute bound, and at least
    /// one valid subject before a goal enters `coach_data.jsonl`.
    pub fn validate(&self) -> crate::Result<()> {
        required(&self.title, "goal title")?;
        required(&self.start_date, "startDate")?;
        required(&self.target_date, "targetDate")?;
        // Minutes are constrained to one day because they are later used to
        // generate daily tasks and must not overflow a calendar day.
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
        // Subject weights may be zero, but the scale must remain positive so a
        // prediction can be normalized safely.
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
/// A concise risk item attached to a Coach analysis.
pub struct CoachRisk {
    pub id: Uuid,
    pub title: String,
    pub severity: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Human-readable source detail supporting a generated analysis.
pub struct CoachEvidence {
    pub source: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Subject-level prediction with bounds and sample-size context.
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
/// Versioned aggregate Coach decision and its evidence/risk payloads.
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
    /// Validate only the invariants needed to interpret probability and goal
    /// version; richer statistical semantics remain model/provider concerns.
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
/// Resolution state for a Coach proposal.
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
    // Empty text is the least surprising value for old rows without a condition.
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl CoachStopCondition {
    /// Convert either wire representation into the task execution text.
    fn as_text(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Structured(value) => value.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// One proposed task generated from a Coach goal.
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
/// Versioned, expiring collection of proposed actions and rationale.
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
    /// Validate the user-visible conclusion and each proposed task's bounds.
    pub fn validate(&self) -> crate::Result<()> {
        required(&self.conclusion, "proposal conclusion")?;
        if self.goal_version < 1 {
            return Err(WorkspaceError::MalformedData {
                path: "Data/coach_data.jsonl".into(),
                detail: "proposal goalVersion must be positive".into(),
            });
        }
        // Keep item validation independent so one malformed generated task is
        // reported before any proposal can be approved.
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
/// Metadata for one Coach conversation.
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
/// One message linked to a Coach chat and optionally a goal.
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
/// Physical JSONL row for Coach data; payload holds Base64-encoded typed JSON.
pub struct CoachDataRow {
    pub kind: String,
    pub payload: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq)]
/// Typed Coach view assembled from heterogeneous physical rows.
/// Unknown rows and row extras are retained so read/write is forward-compatible.
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
    /// Validate typed rows and their goal/chat foreign-key relationships before
    /// the heterogeneous collections are encoded back into JSONL.
    pub fn validate(&self) -> crate::Result<()> {
        // UUID sets make relationship checks independent of collection order.
        let goal_ids: HashSet<_> = self.goals.iter().map(|value| value.id).collect();
        let proposal_ids: HashSet<_> = self.proposals.iter().map(|value| value.id).collect();
        for goal in &self.goals {
            goal.validate()?;
        }
        // Analyses must point to an existing goal version; otherwise a displayed
        // probability would be impossible to explain to the user.
        for analysis in &self.analyses {
            analysis.validate()?;
            if !goal_ids.contains(&analysis.goal_id) {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/coach_data.jsonl".into(),
                    detail: format!("analysis references missing goal {}", analysis.goal_id),
                });
            }
        }
        // Approved proposals need concrete task items because approval is the
        // handoff point to user-confirmed task creation.
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
        // A chat may be general (`None`) but an optional goal link must resolve.
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
        // Messages are kept subordinate to a known chat to avoid orphaned
        // conversation rows after goal deletion.
        for message in &self.messages {
            if !self.chats.iter().any(|chat| chat.id == message.chat_id) {
                return Err(WorkspaceError::MalformedData {
                    path: "Data/coach_data.jsonl".into(),
                    detail: format!("message references missing chat {}", message.chat_id),
                });
            }
        }
        // Keep the proposal ID set available for future proposal-message links;
        // the current wire contract does not require such a relationship.
        let _ = proposal_ids;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Exam target used by the reverse planner.
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
    /// Validate score bounds against the declared full score.
    pub fn validate(&self) -> crate::Result<()> {
        required(&self.exam_name, "examName")?;
        required(&self.subject, "subject")?;
        // Both current and target scores use the same full-score scale; reject
        // NaN/negative/out-of-scale values before planning math sees them.
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
/// One topic-level weakness ranked by mastery and possible score gain.
pub struct ExamWeakPoint {
    pub id: Uuid,
    pub topic: String,
    pub mastery: f64,
    pub possible_score_gain: f64,
    pub priority: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Named phase in a reverse-planned study schedule.
pub struct ExamPlanPhase {
    pub id: Uuid,
    pub name: String,
    pub day_range: String,
    pub goal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// One dated task generated for an exam-plan day offset.
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
/// Reverse-planner output persisted alongside its source exam goal.
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
    /// Validate summary text and every weak-point bound before persistence.
    pub fn validate(&self) -> crate::Result<()> {
        required(&self.summary, "plan summary")?;
        // Mastery is a rate, possible gain is non-negative, and priority starts
        // at one so downstream sorting has no special zero case.
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
/// State machine for a generated exam simulation.
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
/// Supported generated-question formats.
pub enum ExamQuestionKind {
    MultipleChoice,
    FreeResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Immutable question definition used by a simulation.
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
/// Per-question interaction metrics and final answer/score state.
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
/// User interaction event retained for timing and behavior analysis.
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
/// Timestamped simulation event with optional question context.
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
/// Provider-generated role analysis with bounded evidence and strategy lists.
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
/// Complete simulator state, including questions, interaction events, and the
/// optional post-grading analysis.
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
    /// Validate simulation size, question/answer links, terminal timestamps,
    /// and the bounded shape of optional behavior analysis.
    pub fn validate(&self) -> crate::Result<()> {
        required(&self.subject, "subject")?;
        // A simulation must fit in one day; the upper bound also protects timer
        // calculations from unrealistic generated values.
        if self.duration_seconds <= 0 || self.duration_seconds > 24 * 60 * 60 {
            return Err(WorkspaceError::MalformedData {
                path: "Data/exam_simulations.jsonl".into(),
                detail: "durationSeconds is out of bounds".into(),
            });
        }
        // The 100-item safety cap covers partial/legacy data; generated exams
        // are narrowed further to exactly ten questions below.
        if self.questions.len() > 100 || self.question_records.len() > 100 {
            return Err(WorkspaceError::MalformedData {
                path: "Data/exam_simulations.jsonl".into(),
                detail: "simulation contains too many questions".into(),
            });
        }
        // An empty list is valid during Preparing; once generated, the product
        // contract is a fixed ten-question simulation.
        if !self.questions.is_empty() && self.questions.len() != 10 {
            return Err(WorkspaceError::MalformedData {
                path: "Data/exam_simulations.jsonl".into(),
                detail: "a generated simulation must contain exactly 10 questions".into(),
            });
        }
        // The ID set prevents orphan answer records and keeps grading joins
        // deterministic even if question order changes.
        let question_ids: HashSet<_> = self.questions.iter().map(|value| value.id).collect();
        for question in &self.questions {
            // Every question needs prompt/points, and multiple choice needs at
            // least two options so the UI can render a meaningful choice.
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
            // Records may be partial while the simulation is running, but their
            // question IDs must already belong to the generated question set.
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
        // Terminal states need an end instant for resume/report calculations.
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
        // Provider analysis is optional, but when present its evidence and
        // strategy list sizes are bounded to keep generated output reviewable.
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
/// Structured response expected from Coach analysis generation.
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
/// Structured grading response for an exam simulation.
pub struct ExamGradeResponse {
    pub total_score: f64,
    pub analysis: ExamRoleAnalysis,
    #[serde(default)]
    pub question_results: Vec<ExamQuestionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// One graded question result with optional feedback.
pub struct ExamQuestionResult {
    pub question_id: Uuid,
    pub is_correct: bool,
    pub score: f64,
    #[serde(default)]
    pub feedback: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Structured reverse-planner response before it is attached to an ExamGoal.
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
    // Providers often wrap JSON in Markdown fences. Strip only the outer fence
    // so the typed decoder remains the source of truth for the actual schema.
    let value = raw.trim();
    let value = if let Some(stripped) = value.strip_prefix("```json") {
        // Prefer the explicit JSON fence, then accept a generic fence emitted
        // by providers that omit the language tag.
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
    // Keep creation deterministic when a caller supplies a timestamp, while
    // defaulting new simulations to the product's 20-minute preparation state.
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

/// Generic persisted envelope for Phase 3 AI feature results.
///
/// The payload remains feature-owned JSON so an older desktop client can
/// preserve newly introduced output fields.  The envelope itself is typed,
/// validated and stored in one of the five Phase 3 JSONL collections.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiFeatureRecord {
    pub id: Uuid,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default = "default_ai_record_status")]
    pub status: String,
    #[serde(default)]
    pub payload: Value,
    /// An action key is recorded only after Core has completed its local
    /// write.  This makes retried batch confirmation idempotent.
    #[serde(default)]
    pub applied_actions: BTreeMap<String, AiActionApplication>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn default_ai_record_status() -> String {
    "draft".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiActionApplication {
    pub target_id: Uuid,
    pub applied_at: String,
    #[serde(default)]
    pub kind: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl AiFeatureRecord {
    pub fn validate(&self, file: &str) -> crate::Result<()> {
        if self.created_at.is_empty() || self.updated_at.is_empty() {
            return Err(WorkspaceError::MalformedData {
                path: format!("Data/{file}"),
                detail: "AI feature timestamps must not be empty".into(),
            });
        }
        for value in [&self.created_at, &self.updated_at] {
            DateTime::parse_from_rfc3339(value).map_err(|error| WorkspaceError::MalformedData {
                path: format!("Data/{file}"),
                detail: format!("invalid AI feature timestamp: {error}"),
            })?;
        }
        if !self.payload.is_object() {
            return Err(WorkspaceError::MalformedData {
                path: format!("Data/{file}"),
                detail: "AI feature payload must be a JSON object".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// One day in the local learning report, with averaged diary dimensions.
pub struct DailyReportPoint {
    pub date: String,
    pub study_minutes: i64,
    pub session_count: i64,
    pub mood_score: Option<f64>,
    pub energy_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// Bounded aggregate report built entirely from Workspace records.
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
    // Clamp the report horizon before reading records; the resulting date keys
    // are also used as the single filter predicate for every domain.
    let days = range_days.clamp(1, 366);
    // Reports use one current UTC date for all domains so sessions, diaries,
    // grades, mistakes, and exams cannot straddle different local horizons.
    let end = Utc::now().date_naive();
    let start = end - chrono::Days::new((days - 1) as u64);
    let from_date = start.to_string();
    let to_date = end.to_string();
    let from_date_for_filter = from_date.clone();
    let to_date_for_filter = to_date.clone();
    // Stored timestamps are RFC3339, so their first ten bytes are the canonical
    // YYYY-MM-DD key used by the report buckets.
    let in_range = |value: &str| {
        let key = value.get(..10).unwrap_or(value);
        key >= from_date_for_filter.as_str() && key <= to_date_for_filter.as_str()
    };
    // Read every source through Workspace validation before deriving aggregates;
    // the report never opens files directly or performs network work.
    let sessions = workspace.read_study_sessions()?;
    let grades = workspace.read_grades()?;
    let mistakes = workspace.read_mistakes()?;
    let exams = workspace.read_exams()?;
    let comprehensive_exams = workspace.read_comprehensive_exams()?;
    let diaries = workspace.read_diary_entries()?;
    // Pre-seed all days so the report is continuous even when no session was
    // recorded on an intermediate date.
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
    // Session totals and intensity distribution use completed and incomplete
    // records consistently with the existing report contract.
    let mut total_seconds = 0;
    let mut intensity_distribution = BTreeMap::new();
    for session in sessions.iter().filter(|value| in_range(&value.start_date)) {
        // Duration is clamped at zero for resilient reporting of old data, while
        // the displayed session count remains a count of source records.
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
        // Subject distribution counts records, not score points, so it measures
        // where the learner has generated evidence in this period.
        *subject_distribution
            .entry(grade.subject.clone())
            .or_insert(0) += 1;
        if let Some(full_score) = grade.full_score.filter(|value| *value > 0.0) {
            score_sum += grade.score / full_score * 100.0;
            score_count += 1;
        }
    }
    for mistake in mistakes.iter().filter(|value| in_range(&value.date)) {
        // Untagged mistakes still count globally but cannot be assigned to a
        // subject bucket.
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
        // Store running sums in the daily point and divide once after the loop.
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
    // Diary values are accumulated first and divided by that day's entry count
    // after all records have been visited.
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
    // Subject frequency is based on grades plus tagged mistakes; weakest score
    // uses normalized grades only because mistakes have no score denominator.
    let top_subject = subject_distribution
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(name, _)| name.clone());
    // Weakest subject is the minimum normalized grade, not the lowest raw score
    // because subjects may use different full-score scales.
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
    // Keep report fields derived from the same filtered source arrays so totals
    // and per-day points cannot drift apart.
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
    // This helper exposes the physical iOS row shape for callers that need a
    // JSON Value; the typed `make_coach_row` below is used for persistence.
    let payload = serde_json::to_vec(value)?;
    Ok(serde_json::json!({ "kind": kind, "payload": STANDARD.encode(payload) }))
}

pub fn decode_coach_payload<T: for<'de> Deserialize<'de>>(row: &CoachDataRow) -> crate::Result<T> {
    // Decode in the reverse order of `make_coach_row`: Base64 first, typed JSON
    // second, with a domain-specific malformed-data path for UI diagnostics.
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
    // Coach rows keep typed JSON opaque inside Base64 to match the iOS storage
    // contract and to allow heterogeneous kinds in one JSONL file.
    let payload = serde_json::to_vec(value)?;
    Ok(CoachDataRow {
        kind: kind.into(),
        payload: STANDARD.encode(payload),
        extra: BTreeMap::new(),
    })
}

pub fn validate_report_range(range_days: i64) -> crate::Result<()> {
    // Keep the public validation explicit even though `learning_report` clamps
    // internally; host callers receive a clear error for invalid UI input.
    if !(1..=366).contains(&range_days) {
        return Err(WorkspaceError::MalformedData {
            path: "report".into(),
            detail: "report range must be between 1 and 366 days".into(),
        });
    }
    Ok(())
}

pub fn expired(expires_at: &str) -> bool {
    // Malformed expiry is treated as expired (fail closed) rather than keeping
    // an unparseable proposal active.
    // Convert offsets to UTC before comparison so textual hours cannot change
    // the proposal's actual expiry instant.
    DateTime::parse_from_rfc3339(expires_at)
        .map(|value| value.with_timezone(&Utc) < Utc::now())
        .unwrap_or(true)
}

pub fn proposal_task(
    proposal: &CoachProposal,
    item: &CoachProposalItem,
) -> crate::Result<TaskItem> {
    // Convert a proposal item into the existing task contract without writing
    // it. The caller can then show/confirm the task before the normal upsert.
    // Date-only proposals receive the default morning time; full timestamps are
    // retained as supplied by the provider.
    let due = if item.start_date.contains('T') {
        item.start_date.clone()
    } else {
        format!("{}T09:00:00Z", item.start_date)
    };
    let stop_condition = item.stop_condition.as_text();
    // Keep the execution context in the task's validated Base64 field so the
    // Agent can resume the Coach intent without adding ad-hoc task columns.
    // Serialize only after the due date and stop-condition text are normalized.
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
    // Coach rows retain the iOS kind/Base64 payload contract through decode and
    // re-encode, including the typed goal value.
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
    // Model output may be fenced, but arbitrary non-JSON must fail closed with a
    // structured-output path instead of being guessed.
    fn structured_json_rejects_non_json_and_accepts_fenced_json() {
        let parsed: CoachAnalysisResponse = parse_structured_json("```json\n{\"conclusion\":\"ok\",\"rationale\":\"evidence\",\"shouldContinue\":true}\n```").unwrap();
        assert_eq!(parsed.conclusion, "ok");
        assert!(parse_structured_json::<CoachAnalysisResponse>("not json").is_err());
    }

    #[test]
    // Goal/analysis aliases used by Swift payloads remain accepted on decode.
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
    // New simulations start in Preparing with the fixed product duration.
    fn simulation_defaults_to_twenty_minutes() {
        let simulation = default_simulation("Physics".into(), Some("2026-08-02T00:00:00Z".into()));
        assert_eq!(simulation.duration_seconds, 1_200);
        assert_eq!(simulation.status, ExamSimulationStatus::Preparing);
        assert!(simulation.questions.is_empty());
    }

    #[test]
    // Report generation seeds an inclusive daily range even when Workspace data
    // has no sessions, so empty charts remain structurally stable.
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
