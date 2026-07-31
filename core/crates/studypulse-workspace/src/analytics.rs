use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};

use crate::{
    ExamFull, InvestmentTarget, MistakeNoteFull, ReviewState, StudyPhase, StudySession, SubTask,
    TaskItem, TimeInvestmentSubject,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SrsReviewResult {
    pub state: ReviewState,
    pub next_review_date: String,
}

/// SM-2 compatible review update matching the iOS quality values 1/3/4/5.
pub fn apply_srs(
    state: Option<&ReviewState>,
    quality: i64,
    difficulty: i64,
    now: DateTime<Utc>,
) -> SrsReviewResult {
    let mut current = state.cloned().unwrap_or(ReviewState {
        repetitions: 0,
        ease_factor: 2.5,
        interval_days: 0,
        next_review_date: now.to_rfc3339(),
        last_review_date: None,
        lapses: 0,
        extra: Default::default(),
    });
    let quality = quality.clamp(1, 5);
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
    let multiplier = match difficulty {
        1 => 0.5,
        2 => 0.75,
        4 => 1.3,
        5 => 1.6,
        _ => 1.0,
    };
    current.interval_days = ((current.interval_days as f64) * multiplier)
        .round()
        .max(1.0) as i64;
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
    mistakes
        .iter()
        .filter(|mistake| {
            mistake
                .review_state
                .as_ref()
                .and_then(|state| DateTime::parse_from_rfc3339(&state.next_review_date).ok())
                .is_some_and(|date| date.with_timezone(&Utc) <= now)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
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
    let mut results = Vec::new();
    let children: HashMap<_, Vec<_>> = sub_tasks.iter().fold(HashMap::new(), |mut map, task| {
        map.entry(task.parent_sub_task_id)
            .or_default()
            .push(task.id);
        map
    });
    for subject in subjects {
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
    let matching = sessions.iter().filter(|session| {
        session.completed
            && session.duration_seconds > 0
            && matches(session.investment_target.as_ref())
    });
    let mut direct_seconds = 0;
    let mut session_count = 0;
    for session in matching {
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
    let mut days = HashSet::new();
    for session in sessions
        .iter()
        .filter(|session| session.completed && session.duration_seconds > 0)
    {
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
    let today = now.date_naive();
    let open_task_count = tasks.iter().filter(|task| !task.is_completed).count();
    let completed_task_count = tasks.iter().filter(|task| task.is_completed).count();
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

    #[test]
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
}
