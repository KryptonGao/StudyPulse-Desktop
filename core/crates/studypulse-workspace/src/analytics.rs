//! Pure calculations over already-loaded Workspace values.
//!
//! No function in this module writes files or consults process state beyond
//! the `now` argument.  That keeps trends, streaks, and SRS updates reusable
//! from the desktop UI, tests, widgets, and future background callers while
//! making the UTC/date boundaries explicit at each call site.
//!
//! Trend ranges are inclusive and are clamped before daily buckets are built.
//! Diary dimensions are averaged per day, mastery history contributes review
//! activity on its own timestamp, and session minutes contribute only when a
//! session is completed. Subject direction uses the most recent three grades,
//! while SRS due/upcoming counts compare parsed UTC instants to the injected
//! reference time. These choices are product semantics, not presentation hints.
//!
//! The functions tolerate malformed dates while reading historical values, but
//! model validation prevents new malformed values from being written. That
//! asymmetry keeps dashboards resilient to old data without weakening the write
//! boundary or changing the meaning of valid records.
//!
//! Callers should pass one consistent UTC `now` through related calculations so
//! due counts, streaks, and date windows agree at midnight boundaries.
use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};

use crate::{
    DiaryEntry, ExamFull, Grade, InvestmentTarget, MistakeNoteFull, ReviewState, StudyPhase,
    StudySession, SubTask, Subject, TaskItem, TimeInvestmentSubject,
};

#[derive(Debug, Clone, PartialEq)]
/// Result of one review: the updated state plus the canonical next date.
pub struct SrsReviewResult {
    pub state: ReviewState,
    pub next_review_date: String,
}

#[derive(Debug, Clone, PartialEq)]
/// Counts for mistakes enrolled in SRS, split into due and near-future work.
pub struct SrsOverview {
    pub due_count: usize,
    pub upcoming_count: usize,
    pub total_enrolled: usize,
}

#[derive(Debug, Clone, PartialEq)]
/// One calendar-day bucket used by the trends graph and streak calculation.
/// Activity points intentionally combine unlike signals into a stable product
/// metric: minutes + reviews + five points per recorded grade.
pub struct DailyTrendPoint {
    pub date: String,
    pub study_minutes: i64,
    pub activity_points: i64,
    pub completed_session_count: usize,
    pub review_count: usize,
    pub grade_count: usize,
    pub mood_score: Option<f64>,
    pub energy_score: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
/// Grade and wrong-question summary for a single subject in the selected range.
pub struct SubjectTrend {
    pub subject: String,
    pub display_name: String,
    pub average_score_rate: f64,
    pub latest_score_rate: f64,
    pub average_ranking: Option<f64>,
    pub latest_ranking: Option<i64>,
    pub grade_count: usize,
    pub mistake_count: usize,
    pub due_mistake_count: usize,
    pub trend: String,
    pub needs_attention: bool,
}

#[derive(Debug, Clone, PartialEq)]
/// Complete bounded trends response, including daily points and SRS counts.
pub struct TrendsSnapshot {
    pub start_date: String,
    pub end_date: String,
    pub active_days: usize,
    pub current_streak: i64,
    pub total_study_minutes: i64,
    pub average_mood: Option<f64>,
    pub average_energy: Option<f64>,
    pub daily_points: Vec<DailyTrendPoint>,
    pub subjects: Vec<SubjectTrend>,
    pub srs: SrsOverview,
}

fn date_key(value: &str) -> Option<chrono::NaiveDate> {
    // Date grouping uses the instant's calendar date after parsing its offset;
    // malformed records are ignored here because model validation owns writes.
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.date_naive())
}

fn score_rate(grade: &Grade, subjects: &HashMap<String, &Subject>) -> f64 {
    // Prefer the grade's captured scale, then the current subject definition,
    // and finally 100 for old records that supplied neither value.
    let full_score = grade
        .full_score
        .or_else(|| {
            subjects
                .get(&grade.subject)
                .map(|subject| subject.full_score)
        })
        .filter(|value| *value > 0.0)
        .unwrap_or(100.0);
    (grade.score / full_score).clamp(0.0, 1.0)
}

pub fn srs_overview(mistakes: &[MistakeNoteFull], now: DateTime<Utc>) -> SrsOverview {
    // The seven-day window is exclusive of due items: an item due now belongs
    // to `due_count`, not both buckets.
    let upcoming_cutoff = now + Duration::days(7);
    let mut result = SrsOverview {
        due_count: 0,
        upcoming_count: 0,
        total_enrolled: 0,
    };
    for mistake in mistakes {
        // A missing review state means the mistake has not entered the SRS
        // queue and therefore should not inflate enrollment counts.
        let Some(state) = mistake.review_state.as_ref() else {
            continue;
        };
        result.total_enrolled += 1;
        // Parse failures do not become due items because there is no trustworthy
        // instant to compare; the persisted validator reports them on writes.
        let Some(next) = DateTime::parse_from_rfc3339(&state.next_review_date)
            .ok()
            .map(|date| date.with_timezone(&Utc))
        else {
            continue;
        };
        if next <= now {
            // Due is inclusive, so an item scheduled exactly at `now` is ready.
            result.due_count += 1;
        } else if next <= upcoming_cutoff {
            result.upcoming_count += 1;
        }
    }
    result
}

fn activity_streak(points: &[DailyTrendPoint], today: chrono::NaiveDate) -> i64 {
    // Work from a set of active dates so multiple signals on one day count once
    // and a no-activity current day still allows yesterday's streak to display.
    let active: HashSet<_> = points
        .iter()
        .filter(|point| point.activity_points > 0)
        .filter_map(|point| chrono::NaiveDate::parse_from_str(&point.date, "%Y-%m-%d").ok())
        .collect();
    // A current inactive day does not erase yesterday's consecutive run.
    let mut cursor = today;
    if !active.contains(&cursor) {
        cursor -= Duration::days(1);
    }
    let mut streak = 0;
    // Walk backward until the first gap; the returned value is a day count.
    while active.contains(&cursor) {
        streak += 1;
        cursor -= Duration::days(1);
    }
    streak
}

pub fn learning_trends(
    now: DateTime<Utc>,
    range_days: u32,
    diaries: &[DiaryEntry],
    grades: &[Grade],
    subjects: &[Subject],
    mistakes: &[MistakeNoteFull],
    sessions: &[StudySession],
) -> TrendsSnapshot {
    // Clamp the range at the pure-function boundary so callers cannot request
    // an unbounded allocation or a zero-day snapshot.
    let days = range_days.clamp(1, 90) as i64;
    let end = now.date_naive();
    let start = end - Duration::days(days - 1);
    // Subject names are the cross-file join key used by grades, mistakes, and
    // display metadata; the grade's own subject string remains authoritative.
    let subject_map: HashMap<_, _> = subjects
        .iter()
        .map(|subject| (subject.name.clone(), subject))
        .collect();

    // Pre-seed every date; consumers can render continuous axes without
    // inventing missing zero-activity points themselves.
    let mut daily: BTreeMap<chrono::NaiveDate, DailyTrendPoint> = (0..days)
        .map(|offset| {
            let date = start + Duration::days(offset);
            (
                date,
                DailyTrendPoint {
                    date: date.to_string(),
                    study_minutes: 0,
                    activity_points: 0,
                    completed_session_count: 0,
                    review_count: 0,
                    grade_count: 0,
                    mood_score: None,
                    energy_score: None,
                },
            )
        })
        .collect();
    // Diaries are collected first because several entries may share a date and
    // must be averaged only after all values for that date are known.
    let mut diary_scores: HashMap<chrono::NaiveDate, Vec<(i64, i64)>> = HashMap::new();

    for diary in diaries {
        // Date parsing is intentionally non-panicking for read-only dashboards.
        let Some(day) = date_key(&diary.date) else {
            continue;
        };
        if let Some(scores) = diary_scores.get_mut(&day) {
            scores.push((diary.mood_score, diary.energy_score));
        } else if daily.contains_key(&day) {
            diary_scores.insert(day, vec![(diary.mood_score, diary.energy_score)]);
        }
    }
    for session in sessions
        .iter()
        .filter(|session| session.completed && session.duration_seconds > 0)
    {
        // Only completed positive-duration sessions represent actual study;
        // drafts and zero-length timer events do not contribute activity.
        let Some(day) = date_key(&session.start_date) else {
            continue;
        };
        let Some(point) = daily.get_mut(&day) else {
            continue;
        };
        point.study_minutes += session.duration_seconds / 60;
        point.completed_session_count += 1;
    }
    for grade in grades {
        // A grade contributes one activity event regardless of score magnitude;
        // score quality is reported separately in SubjectTrend.
        let Some(day) = date_key(&grade.date) else {
            continue;
        };
        if let Some(point) = daily.get_mut(&day) {
            point.grade_count += 1;
        }
    }
    for mistake in mistakes {
        // Review activity is sourced from mastery history rather than the
        // mistake creation date, which keeps later reviews on their real days.
        for history in &mistake.mastery_history {
            let Some(day) = date_key(&history.timestamp) else {
                continue;
            };
            if let Some(point) = daily.get_mut(&day) {
                point.review_count += 1;
            }
        }
    }
    for (day, scores) in diary_scores {
        // Only dates in the pre-seeded range can have been inserted above, so a
        // missing bucket here means the record was outside the requested range.
        if let Some(point) = daily.get_mut(&day) {
            let count = scores.len() as f64;
            // Mood and energy are independent means; one missing dimension in
            // a future payload should not be substituted for the other.
            point.mood_score =
                Some(scores.iter().map(|(mood, _)| *mood as f64).sum::<f64>() / count);
            point.energy_score =
                Some(scores.iter().map(|(_, energy)| *energy as f64).sum::<f64>() / count);
        }
    }
    let mut daily_points: Vec<_> = daily.into_values().collect();
    for point in &mut daily_points {
        // Keep this weighting in one place so active-day and streak semantics
        // use the same definition as the UI summary.
        point.activity_points =
            point.study_minutes + point.review_count as i64 + point.grade_count as i64 * 5;
    }

    // Build subject-local grade views after daily aggregation so the two outputs
    // can evolve independently without mutating the input slices.
    let mut grade_groups: HashMap<String, Vec<&Grade>> = HashMap::new();
    for grade in grades {
        if date_key(&grade.date).is_some_and(|day| day >= start && day <= end) {
            grade_groups
                .entry(grade.subject.clone())
                .or_default()
                .push(grade);
        }
    }
    let due = due_mistakes(mistakes, now);
    let mut subjects_result = Vec::new();
    for (subject, mut values) in grade_groups {
        // Sorting before selecting the last three makes the direction metric
        // deterministic even when the input files are append-order mixed.
        values.sort_by_key(|grade| date_key(&grade.date));
        // Normalize each score before averaging; mixed full-score exams remain
        // comparable within one subject.
        let rates: Vec<_> = values
            .iter()
            .map(|grade| score_rate(grade, &subject_map))
            .collect();
        let average = rates.iter().sum::<f64>() / rates.len() as f64;
        let recent: Vec<_> = rates.iter().rev().take(3).copied().collect();
        let first = recent.last().copied().unwrap_or(average);
        let last = recent.first().copied().unwrap_or(average);
        let change = last - first;
        // Thresholds deliberately ignore small noise around a steady result.
        let trend = if change > 0.05 {
            "rising"
        } else if change < -0.05 {
            "falling"
        } else {
            "steady"
        };
        // Rankings are optional and are averaged only over records that supply
        // them, rather than treating missing data as zero.
        let rankings: Vec<_> = values.iter().filter_map(|grade| grade.ranking).collect();
        let subject_mistakes: Vec<_> = mistakes
            .iter()
            .filter(|mistake| {
                mistake.subject == subject
                    && date_key(&mistake.date).is_some_and(|day| day >= start && day <= end)
            })
            .collect();
        let due_mistakes = due
            .iter()
            .filter(|mistake| mistake.subject == subject)
            .count();
        subjects_result.push(SubjectTrend {
            display_name: subject_map
                .get(&subject)
                .map(|value| value.display_name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| subject.clone()),
            subject,
            average_score_rate: average,
            latest_score_rate: *rates.last().unwrap_or(&average),
            average_ranking: (!rankings.is_empty())
                .then(|| rankings.iter().sum::<i64>() as f64 / rankings.len() as f64),
            latest_ranking: values.iter().rev().find_map(|grade| grade.ranking),
            grade_count: values.len(),
            mistake_count: subject_mistakes.len(),
            due_mistake_count: due_mistakes,
            trend: trend.into(),
            // Attention requires enough recent evidence and is triggered by
            // either low recent mastery or a meaningful downward change.
            needs_attention: recent.len() >= 2
                && (recent.iter().sum::<f64>() / (recent.len() as f64) < 0.7 || change < -0.15),
        });
    }
    subjects_result.sort_by(|left, right| left.subject.cmp(&right.subject));
    // Global mood/energy means use only days with data, not the zero-filled
    // buckets introduced for a continuous chart.
    let mood_values: Vec<_> = daily_points
        .iter()
        .filter_map(|point| point.mood_score)
        .collect();
    let energy_values: Vec<_> = daily_points
        .iter()
        .filter_map(|point| point.energy_score)
        .collect();
    // Compute SRS once from the same `now` used for the range and streak.
    let srs = srs_overview(mistakes, now);
    // Construct the snapshot last so all derived counts use the same bounded
    // daily vector and the same SRS reference instant.
    TrendsSnapshot {
        start_date: start.to_string(),
        end_date: end.to_string(),
        active_days: daily_points
            .iter()
            .filter(|point| point.activity_points > 0)
            .count(),
        current_streak: activity_streak(&daily_points, end),
        total_study_minutes: daily_points.iter().map(|point| point.study_minutes).sum(),
        average_mood: (!mood_values.is_empty())
            .then(|| mood_values.iter().sum::<f64>() / mood_values.len() as f64),
        average_energy: (!energy_values.is_empty())
            .then(|| energy_values.iter().sum::<f64>() / energy_values.len() as f64),
        daily_points,
        subjects: subjects_result,
        srs,
    }
}

/// SM-2 compatible review update matching the iOS quality values 1/3/4/5.
pub fn apply_srs(
    state: Option<&ReviewState>,
    quality: i64,
    difficulty: i64,
    now: DateTime<Utc>,
) -> SrsReviewResult {
    // Start new cards with the same SM-2 defaults as the iOS implementation;
    // cloning an existing state also preserves its unknown extension fields.
    let mut current = state.cloned().unwrap_or(ReviewState {
        repetitions: 0,
        ease_factor: 2.5,
        interval_days: 0,
        next_review_date: now.to_rfc3339(),
        last_review_date: None,
        lapses: 0,
        extra: Default::default(),
    });
    // The wire contract uses 1/3/4/5, but clamp malformed callers before the
    // match so any out-of-range value follows the hardest-success path safely.
    let quality = quality.clamp(1, 5);
    // Quality 1 is Again, 3 Hard, 4 Good, and 5 Easy; quality 2 follows the
    // default branch after clamping to retain the historical compatibility rule.
    match quality {
        1 => {
            current.repetitions = 0;
            current.interval_days = 1;
            current.lapses += 1;
            current.ease_factor = (current.ease_factor - 0.20).max(1.3);
        }
        3 => {
            current.repetitions += 1;
            current.interval_days = match current.repetitions {
                1 => 1,
                2 => 4,
                _ => ((current.interval_days.max(1) as f64) * 1.2).round() as i64,
            };
            current.ease_factor = (current.ease_factor - 0.15).max(1.3);
        }
        4 => {
            current.repetitions += 1;
            current.interval_days = match current.repetitions {
                1 => 1,
                2 => 6,
                _ => ((current.interval_days.max(1) as f64) * current.ease_factor).round() as i64,
            };
        }
        _ => {
            current.repetitions += 1;
            current.interval_days = match current.repetitions {
                1 => 4,
                2 => 7,
                _ => ((current.interval_days.max(1) as f64) * current.ease_factor * 1.3).round()
                    as i64,
            };
            current.ease_factor = (current.ease_factor + 0.15).min(3.0);
        }
    }
    // Difficulty adjusts the interval after the quality-specific SM-2 update;
    // unknown difficulty values intentionally retain a neutral multiplier.
    let multiplier = match difficulty {
        1 => 0.5,
        2 => 0.75,
        4 => 1.3,
        5 => 1.6,
        _ => 1.0,
    };
    // Round after applying difficulty so intervals remain integral days and are
    // never reduced below one day.
    current.interval_days = ((current.interval_days as f64) * multiplier)
        .round()
        .max(1.0) as i64;
    // Review dates are based on UTC midnight rather than the review instant so
    // all clients agree on the same calendar day around timezone boundaries.
    let next = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc()
        + Duration::days(current.interval_days);
    current.next_review_date = next.to_rfc3339();
    current.last_review_date = Some(now.to_rfc3339());
    SrsReviewResult {
        state: current,
        next_review_date: next.to_rfc3339(),
    }
}

pub fn due_mistakes(mistakes: &[MistakeNoteFull], now: DateTime<Utc>) -> Vec<&MistakeNoteFull> {
    // Invalid or missing next dates are ignored here; validation rejects them
    // on writes, while read-only analytics remains resilient to old data.
    mistakes
        .iter()
        .filter(|mistake| {
            // Review state is optional: `None` means not enrolled, not due.
            mistake
                .review_state
                .as_ref()
                .and_then(|state| DateTime::parse_from_rfc3339(&state.next_review_date).ok())
                .is_some_and(|date| date.with_timezone(&Utc) <= now)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
/// Direct and aggregate time spent for one investment target.
pub struct TimeInvestmentSummary {
    pub target_id: String,
    pub direct_seconds: i64,
    pub total_seconds: i64,
    pub session_count: usize,
}

pub fn investment_summary(
    subjects: &[TimeInvestmentSubject],
    sub_tasks: &[SubTask],
    sessions: &[StudySession],
) -> Vec<TimeInvestmentSummary> {
    // Build the child index once so each subtask can include its descendants
    // without repeatedly scanning the full list.
    let mut results = Vec::new();
    let children: HashMap<_, Vec<_>> = sub_tasks.iter().fold(HashMap::new(), |mut map, task| {
        map.entry(task.parent_sub_task_id)
            .or_default()
            .push(task.id);
        map
    });
    for subject in subjects {
        // Subject totals include sessions targeted directly at the subject and
        // sessions targeted at any immediate/deep descendant subtask.
        let matching_subtasks: HashSet<_> = sub_tasks
            .iter()
            .filter(|task| task.subject_id == subject.id)
            .map(|task| task.id)
            .collect();
        results.push(summary_for_target(
            format!("subject:{}", subject.id),
            sessions,
            |target| match target {
                Some(InvestmentTarget::Subject(id)) if *id == subject.id => true,
                Some(InvestmentTarget::SubTask(id)) => matching_subtasks.contains(id),
                _ => false,
            },
        ));
    }
    for task in sub_tasks {
        // Walk downward from the current task; cycles are harmless because the
        // set prevents revisiting a UUID, while preserving the existing model's
        // tree-oriented aggregation semantics.
        let mut included = HashSet::from([task.id]);
        let mut pending = vec![task.id];
        while let Some(parent) = pending.pop() {
            for child in children.get(&Some(parent)).into_iter().flatten() {
                if included.insert(*child) {
                    pending.push(*child);
                }
            }
        }
        results.push(summary_for_target(
            format!("subTask:{}", task.id),
            sessions,
            |target| matches!(target, Some(InvestmentTarget::SubTask(id)) if included.contains(id)),
        ));
    }
    results
}

fn summary_for_target(
    target_id: String,
    sessions: &[StudySession],
    matches: impl Fn(Option<&InvestmentTarget>) -> bool,
) -> TimeInvestmentSummary {
    // Only completed, positive sessions count as investment. `direct_seconds`
    // is currently also the total because this function receives one closure
    // that already captures the target's descendants.
    let matching = sessions.iter().filter(|session| {
        session.completed
            && session.duration_seconds > 0
            && matches(session.investment_target.as_ref())
    });
    let mut direct_seconds = 0;
    let mut session_count = 0;
    for session in matching {
        // A completed timer may be reopened/upserted, but only its positive
        // completed duration represents real investment.
        direct_seconds += session.duration_seconds;
        session_count += 1;
    }
    TimeInvestmentSummary {
        target_id,
        direct_seconds,
        total_seconds: direct_seconds,
        session_count,
    }
}

pub fn current_streak(sessions: &[StudySession], now: DateTime<Utc>) -> i64 {
    // Expand sessions across every calendar date they touch, so an interval
    // crossing midnight contributes to both days without double-counting a day.
    let mut days = HashSet::new();
    for session in sessions
        .iter()
        .filter(|session| session.completed && session.duration_seconds > 0)
    {
        // Invalid start dates fall back to `now`, preserving the existing
        // read-only streak behavior without panicking on legacy data.
        let start = DateTime::parse_from_rfc3339(&session.start_date)
            .map(|date| date.with_timezone(&Utc))
            .unwrap_or(now);
        let end = start + Duration::seconds(session.duration_seconds);
        let mut day = start.date_naive();
        while day <= end.date_naive() {
            days.insert(day);
            day += Duration::days(1);
        }
    }
    // Keep yesterday visible when the current day has not started a session.
    let mut cursor = now.date_naive();
    if !days.contains(&cursor) {
        cursor -= Duration::days(1);
    }
    let mut streak = 0;
    while days.contains(&cursor) {
        streak += 1;
        cursor -= Duration::days(1);
    }
    streak
}

#[derive(Debug, Clone, PartialEq)]
/// Home snapshot assembled from local tasks, SRS, exams, and sessions.
pub struct TodaySnapshot {
    pub date: String,
    pub open_task_count: usize,
    pub completed_task_count: usize,
    pub study_minutes: i64,
    pub due_mistake_count: usize,
    pub upcoming_exams: Vec<ExamFull>,
    pub streak_days: i64,
    pub assigned_seconds: i64,
    pub suggestions: Vec<String>,
}

pub fn today_snapshot(
    now: DateTime<Utc>,
    tasks: &[TaskItem],
    mistakes: &[MistakeNoteFull],
    exams: &[ExamFull],
    sessions: &[StudySession],
    investment_seconds: i64,
    _phases: &[StudyPhase],
) -> TodaySnapshot {
    // `now` is injected to keep the snapshot deterministic in tests and to
    // make all date comparisons use one UTC reference instant.
    let today = now.date_naive();
    // Task counts are intentionally global to the Workspace; phase filtering is
    // a caller concern and `_phases` is retained for API compatibility.
    let open_task_count = tasks.iter().filter(|task| !task.is_completed).count();
    let completed_task_count = tasks.iter().filter(|task| task.is_completed).count();
    // Count sessions by their start date, matching the timer's user-facing day.
    let study_minutes = sessions
        .iter()
        .filter(|session| session.completed)
        .filter_map(|session| {
            DateTime::parse_from_rfc3339(&session.start_date)
                .ok()
                .filter(|start| start.with_timezone(&Utc).date_naive() == today)
                .map(|_| session.duration_seconds / 60)
        })
        .sum::<i64>();
    // The home window includes exams from yesterday through the next 30 days,
    // matching the UI's near-term planning horizon.
    let upcoming_exams = exams
        .iter()
        .filter(|exam| {
            DateTime::parse_from_rfc3339(&exam.exam_date)
                .ok()
                .map(|date| {
                    let date = date.with_timezone(&Utc);
                    date >= now - Duration::days(1) && date <= now + Duration::days(30)
                })
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    let due_mistake_count = due_mistakes(mistakes, now).len();
    // Suggestions are intentionally short, deterministic strings; localization
    // and presentation remain responsibilities of the caller.
    let mut suggestions = Vec::new();
    if due_mistake_count > 0 {
        suggestions.push(format!("Review {due_mistake_count} due mistakes."));
    }
    if open_task_count > 0 {
        suggestions.push(format!("Choose one of {open_task_count} open tasks."));
    }
    if study_minutes == 0 {
        suggestions.push("Start a focused study session.".into());
    }
    TodaySnapshot {
        date: now.date_naive().to_string(),
        open_task_count,
        completed_task_count,
        study_minutes,
        due_mistake_count,
        upcoming_exams,
        streak_days: current_streak(sessions, now),
        assigned_seconds: investment_seconds,
        suggestions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn diary(date: &str, mood: i64, energy: i64) -> DiaryEntry {
        serde_json::from_value(json!({
            "id": uuid::Uuid::new_v4(),
            "date": date,
            "moodScore": mood,
            "energyScore": energy,
            "createdAt": date,
            "updatedAt": date,
        }))
        .unwrap()
    }

    fn grade(subject: &str, score: f64, date: &str, ranking: i64) -> Grade {
        serde_json::from_value(json!({
            "id": uuid::Uuid::new_v4(),
            "subject": subject,
            "score": score,
            "fullScore": 100.0,
            "ranking": ranking,
            "date": date,
        }))
        .unwrap()
    }

    fn mistake(subject: &str, next_review_date: &str, review_timestamp: &str) -> MistakeNoteFull {
        serde_json::from_value(json!({
            "id": uuid::Uuid::new_v4(),
            "title": "A mistake",
            "subject": subject,
            "originalQuestion": "Question",
            "date": review_timestamp,
            "errorReason": "Reason",
            "wrongSolution": "Wrong",
            "correctSolution": "Correct",
            "reviewState": {
                "repetitions": 1,
                "easeFactor": 2.5,
                "intervalDays": 1,
                "nextReviewDate": next_review_date,
                "lastReviewDate": review_timestamp,
                "lapses": 0,
            },
            "masteryHistory": [{
                "id": uuid::Uuid::new_v4(),
                "timestamp": review_timestamp,
                "score": 0.5,
                "quality": 3,
            }],
        }))
        .unwrap()
    }

    fn session(start_date: &str, duration_seconds: i64) -> StudySession {
        StudySession {
            id: uuid::Uuid::new_v4(),
            start_date: start_date.into(),
            duration_seconds,
            intensity: crate::SessionIntensity::Steady,
            completed: true,
            heart_rate_samples: None,
            difficulty_annotations: None,
            investment_target: None,
            source: crate::StudySessionSource::Timer,
            time_zone_identifier: None,
            extra: Default::default(),
        }
    }

    #[test]
    // Verify the shared 1/3/4/5 quality mapping and the reset/growth branches.
    fn srs_again_resets_and_good_grows_interval() {
        let now = DateTime::parse_from_rfc3339("2026-07-31T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let first = apply_srs(None, 4, 0, now);
        assert_eq!(first.state.interval_days, 1);
        let second = apply_srs(Some(&first.state), 4, 0, now);
        assert_eq!(second.state.interval_days, 6);
        let again = apply_srs(Some(&second.state), 1, 0, now);
        assert_eq!(again.state.repetitions, 0);
        assert_eq!(again.state.interval_days, 1);
    }

    #[test]
    // Sessions on today and yesterday form a two-day streak from one UTC now.
    fn streak_counts_today_and_previous_days() {
        let now = DateTime::parse_from_rfc3339("2026-07-31T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let sessions = [
            StudySession {
                id: uuid::Uuid::new_v4(),
                start_date: "2026-07-30T10:00:00Z".into(),
                duration_seconds: 60,
                intensity: crate::SessionIntensity::Steady,
                completed: true,
                heart_rate_samples: None,
                difficulty_annotations: None,
                investment_target: None,
                source: crate::StudySessionSource::Timer,
                time_zone_identifier: None,
                extra: Default::default(),
            },
            StudySession {
                id: uuid::Uuid::new_v4(),
                start_date: "2026-07-31T10:00:00Z".into(),
                duration_seconds: 60,
                intensity: crate::SessionIntensity::Steady,
                completed: true,
                heart_rate_samples: None,
                difficulty_annotations: None,
                investment_target: None,
                source: crate::StudySessionSource::Timer,
                time_zone_identifier: None,
                extra: Default::default(),
            },
        ];
        assert_eq!(current_streak(&sessions, now), 2);
    }

    #[test]
    // Same-day diaries are averaged, while minutes/reviews/grades use their
    // documented activity weights.
    fn trends_average_same_day_diaries_and_apply_activity_weights() {
        let now = DateTime::parse_from_rfc3339("2026-07-31T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let snapshot = learning_trends(
            now,
            3,
            &[
                diary("2026-07-31T08:00:00Z", 2, 4),
                diary("2026-07-31T18:00:00Z", 4, 2),
            ],
            &[grade("Math", 80.0, "2026-07-31T09:00:00Z", 3)],
            &[],
            &[],
            &[session("2026-07-31T10:00:00Z", 90 * 60)],
        );
        let today = snapshot.daily_points.last().unwrap();
        assert_eq!(snapshot.start_date, "2026-07-29");
        assert_eq!(snapshot.end_date, "2026-07-31");
        assert_eq!(today.study_minutes, 90);
        assert_eq!(today.grade_count, 1);
        assert_eq!(today.activity_points, 95);
        assert_eq!(today.mood_score, Some(3.0));
        assert_eq!(today.energy_score, Some(3.0));
        assert_eq!(snapshot.active_days, 1);
    }

    #[test]
    // Subject direction, attention, and SRS counts stay scoped to the range and
    // subject key rather than leaking across records.
    fn trends_subject_direction_and_srs_counts_are_scoped() {
        let now = DateTime::parse_from_rfc3339("2026-07-31T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let snapshot = learning_trends(
            now,
            30,
            &[],
            &[
                grade("Math", 50.0, "2026-07-05T09:00:00Z", 8),
                grade("Math", 85.0, "2026-07-30T09:00:00Z", 3),
            ],
            &[],
            &[
                mistake("Math", "2026-07-30T00:00:00Z", "2026-07-30T08:00:00Z"),
                mistake("Math", "2026-08-03T00:00:00Z", "2026-07-29T08:00:00Z"),
            ],
            &[],
        );
        assert_eq!(snapshot.srs.total_enrolled, 2);
        assert_eq!(snapshot.srs.due_count, 1);
        assert_eq!(snapshot.srs.upcoming_count, 1);
        let subject = &snapshot.subjects[0];
        assert_eq!(subject.trend, "rising");
        assert!(subject.needs_attention);
        assert_eq!(subject.latest_ranking, Some(3));
        assert_eq!(subject.average_ranking, Some(5.5));
        assert_eq!(subject.due_mistake_count, 1);
    }

    #[test]
    // Model validation remains the write-side guard for invalid diary scores and
    // timestamps even though read-only analytics is tolerant.
    fn diary_validation_rejects_invalid_score_and_date() {
        let invalid_score = diary("2026-07-31T00:00:00Z", 6, 3);
        assert!(invalid_score.validate().is_err());
        let invalid_date = diary("not-a-date", 3, 3);
        assert!(invalid_date.validate().is_err());
    }
}
