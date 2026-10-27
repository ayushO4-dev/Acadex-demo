//! Turning marks into grades, SGPA and CGPA. Pure functions over the
//! institution's grading configuration, so the rules are unit-tested here.

use crate::results::{ComponentMark, Grading, SubjectMarks};
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct SubjectResult {
    /// 1 for the regular attempt, then backlog attempts.
    pub attempt: i16,
    /// regular | backlog | revaluation: the result set it was published in.
    pub kind: String,
    pub published_at: Option<DateTime<Utc>>,
    pub code: String,
    pub name: String,
    pub credits: i16,
    pub components: Vec<ComponentMark>,
    pub internal: f64,
    pub internal_max: f64,
    pub external: f64,
    pub external_max: f64,
    pub total: f64,
    pub max: f64,
    pub percentage: f64,
    pub grade: String,
    pub points: i16,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SemesterSummary {
    pub semester: i16,
    pub academic_year: i16,
    /// `None` while staged (only the exam cell sees those).
    pub published_at: Option<DateTime<Utc>>,
    /// From each subject's latest result (a cleared backlog counts).
    pub sgpa: f64,
    /// The SGPA as first published, from regular attempts only.
    pub sgpa_regular: f64,
    /// Cumulative over this and every earlier semester, latest results.
    pub cgpa: f64,
    pub credits_attempted: i64,
    pub credits_earned: i64,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct SemesterResult {
    #[serde(flatten)]
    pub summary: SemesterSummary,
    /// Each subject's latest result.
    pub subjects: Vec<SubjectResult>,
    /// Every attempt, by subject then attempt: the Regular and Backlog views.
    pub history: Vec<SubjectResult>,
}

/// The grade for one subject's marks. Below the internal or external
/// minimum it is the lowest band (the fail grade) whatever the total; bands
/// are highest first.
pub struct Grade {
    pub grade: String,
    pub points: i16,
    pub internal_ok: bool,
    pub external_ok: bool,
    pub percentage: f64,
}

impl Grade {
    pub fn passed(&self) -> bool {
        self.internal_ok && self.external_ok && self.points > 0
    }
}

pub fn grade(
    internal: f64,
    internal_max: f64,
    external: f64,
    external_max: f64,
    grading: &Grading,
) -> Grade {
    let pct = |marks: f64, max: f64| {
        if max > 0.0 {
            marks * 100.0 / max
        } else {
            100.0
        }
    };
    let internal_ok = pct(internal, internal_max) >= grading.internal_min_pct;
    let external_ok = pct(external, external_max) >= grading.external_min_pct;
    let percentage = if internal_max + external_max > 0.0 {
        (internal + external) * 100.0 / (internal_max + external_max)
    } else {
        0.0
    };
    let band = if internal_ok && external_ok {
        grading.scale.iter().find(|band| percentage >= band.min_pct)
    } else {
        None
    }
    .or(grading.scale.last());
    let (grade, points) = band.map_or(("—".to_owned(), 0), |b| (b.grade.clone(), b.points));
    Grade {
        grade,
        points,
        internal_ok,
        external_ok,
        percentage,
    }
}

pub fn grade_subject(marks: SubjectMarks, grading: &Grading) -> SubjectResult {
    let internal: f64 = marks.components.iter().filter_map(|c| c.marks).sum();
    let internal_max: f64 = marks.components.iter().map(|c| c.max).sum();
    let external_max = f64::from(marks.external_max);
    let total = internal + marks.external;
    let max = internal_max + external_max;
    let outcome = grade(
        internal,
        internal_max,
        marks.external,
        external_max,
        grading,
    );
    let passed = outcome.passed();
    let Grade {
        grade,
        points,
        percentage,
        ..
    } = outcome;

    SubjectResult {
        attempt: marks.attempt,
        kind: marks.kind,
        published_at: marks.published_at,
        code: marks.code,
        name: marks.name,
        credits: marks.credits,
        components: marks.components.0,
        internal: round(internal, 1),
        internal_max,
        external: marks.external,
        external_max,
        total: round(total, 1),
        max,
        percentage: round(percentage, 1),
        passed,
        grade,
        points,
    }
}

/// Groups subject marks (ordered by semester, subject, attempt) into graded
/// semesters. A subject's latest attempt is its result: SGPA, CGPA, credits
/// and pass all use it, so a cleared backlog replaces the fail. Failed
/// subjects count with zero points. Every attempt stays in `history`.
pub fn by_semester(rows: Vec<SubjectMarks>, grading: &Grading) -> Vec<SemesterResult> {
    let mut semesters: Vec<SemesterResult> = Vec::new();
    for row in rows {
        let (semester, year) = (row.semester, row.academic_year);
        let graded = grade_subject(row, grading);
        match semesters.last_mut() {
            Some(last) if last.summary.semester == semester => last.history.push(graded),
            _ => semesters.push(SemesterResult {
                summary: SemesterSummary {
                    semester,
                    academic_year: year,
                    published_at: None,
                    sgpa: 0.0,
                    sgpa_regular: 0.0,
                    cgpa: 0.0,
                    credits_attempted: 0,
                    credits_earned: 0,
                    passed: false,
                },
                subjects: Vec::new(),
                history: vec![graded],
            }),
        }
    }

    let (mut all_weighted, mut all_credits) = (0i64, 0i64);
    let weigh = |subjects: &[&SubjectResult]| -> (i64, i64) {
        subjects.iter().fold((0, 0), |(w, c), s| {
            (
                w + i64::from(s.credits) * i64::from(s.points),
                c + i64::from(s.credits),
            )
        })
    };
    for semester in &mut semesters {
        // Latest attempt per subject: history is ordered, so the last one wins.
        let mut latest: Vec<&SubjectResult> = Vec::new();
        for attempt in &semester.history {
            match latest.iter_mut().find(|s| s.code == attempt.code) {
                Some(slot) => *slot = attempt,
                None => latest.push(attempt),
            }
        }
        let regular: Vec<&SubjectResult> =
            semester.history.iter().filter(|s| s.attempt == 1).collect();
        let (weighted, credits) = weigh(&latest);
        all_weighted += weighted;
        all_credits += credits;

        let summary = &mut semester.summary;
        if let Some(first) = regular.first() {
            summary.published_at = first.published_at;
        } else {
            summary.published_at = latest.first().and_then(|s| s.published_at);
        }
        let (regular_weighted, regular_credits) = weigh(&regular);
        summary.sgpa = gpa(weighted, credits);
        summary.sgpa_regular = gpa(regular_weighted, regular_credits);
        summary.cgpa = gpa(all_weighted, all_credits);
        summary.credits_attempted = credits;
        summary.credits_earned = latest
            .iter()
            .filter(|s| s.passed)
            .map(|s| i64::from(s.credits))
            .sum();
        summary.passed = latest.iter().all(|s| s.passed);
        semester.subjects = latest.into_iter().cloned().collect();
    }
    semesters
}

fn gpa(weighted: i64, credits: i64) -> f64 {
    if credits == 0 {
        0.0
    } else {
        round(weighted as f64 / credits as f64, 2)
    }
}

fn round(value: f64, places: i32) -> f64 {
    let factor = 10f64.powi(places);
    (value * factor).round() / factor
}

#[cfg(test)]
mod tests {
    use crate::results::GradeBand;
    use sqlx::types::Json;

    use super::*;

    fn grading() -> Grading {
        let bands = [
            (90.0, "O", 10),
            (80.0, "A+", 9),
            (70.0, "A", 8),
            (60.0, "B+", 7),
            (50.0, "B", 6),
            (45.0, "C", 5),
            (40.0, "P", 4),
            (0.0, "F", 0),
        ];
        Grading {
            scale: bands
                .into_iter()
                .map(|(min_pct, grade, points)| GradeBand {
                    min_pct,
                    grade: grade.to_owned(),
                    points,
                })
                .collect(),
            external_min_pct: 40.0,
            internal_min_pct: 40.0,
        }
    }

    /// A subject out of 100: internal out of 40, external out of 60.
    fn marks(
        semester: i16,
        code: &str,
        credits: i16,
        internal: f64,
        external: f64,
    ) -> SubjectMarks {
        SubjectMarks {
            attempt: 1,
            kind: "regular".to_owned(),
            semester,
            academic_year: 2025,
            published_at: None,
            code: code.to_owned(),
            name: code.to_owned(),
            credits,
            external,
            external_max: 60,
            components: Json(vec![ComponentMark {
                code: "CCA".to_owned(),
                max: 40.0,
                marks: Some(internal),
            }]),
        }
    }

    #[test]
    fn grade_boundaries_are_inclusive_at_the_band_minimum() {
        let g = grading();
        let grade = |internal, external| grade_subject(marks(1, "X", 4, internal, external), &g);
        assert_eq!(grade(36.0, 54.0).grade, "O"); // exactly 90
        assert_eq!(grade(35.9, 54.0).grade, "A+"); // 89.9
        assert_eq!(grade(16.0, 24.0).grade, "P"); // exactly 40, external at its minimum
        let fail = grade(15.9, 24.0); // 39.9
        assert_eq!(
            (fail.grade.as_str(), fail.points, fail.passed),
            ("F", 0, false)
        );
    }

    #[test]
    fn below_the_external_minimum_fails_whatever_the_total() {
        // 40 + 23 = 63%, which would be B+, but 23/60 is under 40% external.
        let result = grade_subject(marks(1, "X", 4, 40.0, 23.0), &grading());
        assert_eq!(result.percentage, 63.0);
        assert_eq!(
            (result.grade.as_str(), result.points, result.passed),
            ("F", 0, false)
        );
    }

    #[test]
    fn below_the_internal_minimum_fails_too() {
        // 15 + 60 = 75%, an A on the total, but 15/40 internal is under 40%.
        let result = grade_subject(marks(1, "X", 4, 15.0, 60.0), &grading());
        assert_eq!(result.percentage, 75.0);
        assert_eq!((result.grade.as_str(), result.passed), ("F", false));
    }

    #[test]
    fn missing_internal_marks_count_as_zero() {
        let mut row = marks(1, "X", 4, 0.0, 30.0);
        row.components.0[0].marks = None;
        let result = grade_subject(row, &grading());
        assert_eq!(
            (result.internal, result.internal_max, result.total),
            (0.0, 40.0, 30.0)
        );
    }

    #[test]
    fn a_cleared_backlog_replaces_the_fail() {
        let mut backlog = marks(1, "B", 2, 20.0, 40.0); // 60%: B+, 7 points
        backlog.attempt = 2;
        backlog.kind = "backlog".to_owned();
        let semesters = by_semester(
            vec![
                marks(1, "A", 4, 36.0, 54.0), // O, 10 × 4
                marks(1, "B", 2, 10.0, 20.0), // F, 0 × 2
                backlog,
            ],
            &grading(),
        );
        let one = &semesters[0];
        assert_eq!(one.history.len(), 3, "every attempt is kept");
        assert_eq!(one.subjects.len(), 2, "one result per subject");
        assert_eq!(one.subjects[1].grade, "B+");
        assert_eq!(one.summary.sgpa, 9.0); // (40 + 14) / 6
        assert_eq!(one.summary.sgpa_regular, 6.67); // (40 + 0) / 6, as first published
        assert!(one.summary.passed);
    }

    #[test]
    fn sgpa_per_semester_and_cgpa_accumulates() {
        let semesters = by_semester(
            vec![
                marks(1, "A", 4, 36.0, 54.0), // O, 10 × 4
                marks(1, "B", 2, 28.0, 42.0), // A, 8 × 2
                marks(2, "C", 3, 10.0, 20.0), // F, 0 × 3
                marks(2, "D", 3, 30.0, 45.0), // A, 8 × 3
            ],
            &grading(),
        );
        assert_eq!(semesters.len(), 2);
        let (one, two) = (&semesters[0].summary, &semesters[1].summary);
        assert_eq!(one.sgpa, 9.33); // 56 / 6
        assert_eq!(one.cgpa, 9.33);
        assert!(one.passed);
        assert_eq!(two.sgpa, 4.0); // 24 / 6
        assert_eq!(two.cgpa, 6.67); // 80 / 12
        assert_eq!(
            (two.credits_attempted, two.credits_earned, two.passed),
            (6, 3, false)
        );
    }
}
