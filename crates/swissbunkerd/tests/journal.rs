use swissbunkerd::journal::{JobId, Journal, Stage};

fn temp_journal() -> (tempfile::TempDir, Journal) {
    let dir = tempfile::tempdir().unwrap();
    let j = Journal::open(&dir.path().join("journal.db")).unwrap();
    (dir, j)
}

#[test]
fn a_job_that_never_started_has_no_resume_point() {
    let (_d, j) = temp_journal();
    assert_eq!(
        j.resume_point(&JobId("wikipedia_it".into()), Stage::Download)
            .unwrap(),
        None
    );
}

#[test]
fn progress_is_recoverable() {
    let (_d, j) = temp_journal();
    let job = JobId("wikipedia_it".into());
    j.mark(&job, Stage::Download, 1_000_000, 40_000_000)
        .unwrap();
    assert_eq!(
        j.resume_point(&job, Stage::Download).unwrap(),
        Some(1_000_000)
    );
}

#[test]
fn progress_survives_reopening() {
    // The whole point of the journal: a killed process must not lose its place. Dropping and
    // reopening is the closest a test gets to pulling the cable.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.db");
    let job = JobId("wikipedia_it".into());
    {
        let j = Journal::open(&path).unwrap();
        j.mark(&job, Stage::Download, 5_000, 10_000).unwrap();
    }
    let j = Journal::open(&path).unwrap();
    assert_eq!(j.resume_point(&job, Stage::Download).unwrap(), Some(5_000));
}

#[test]
fn marking_the_same_position_twice_is_not_an_error() {
    let (_d, j) = temp_journal();
    let job = JobId("x".into());
    j.mark(&job, Stage::Download, 42, 100).unwrap();
    j.mark(&job, Stage::Download, 42, 100).unwrap();
    assert_eq!(j.resume_point(&job, Stage::Download).unwrap(), Some(42));
}

#[test]
fn stages_are_independent() {
    let (_d, j) = temp_journal();
    let job = JobId("x".into());
    j.mark(&job, Stage::Download, 100, 100).unwrap();
    j.complete(&job, Stage::Download).unwrap();
    assert!(j.is_complete(&job, Stage::Download).unwrap());
    assert!(!j.is_complete(&job, Stage::Index).unwrap());
    assert_eq!(j.resume_point(&job, Stage::Index).unwrap(), None);
}

#[test]
fn jobs_are_independent() {
    let (_d, j) = temp_journal();
    j.mark(&JobId("a".into()), Stage::Download, 10, 100)
        .unwrap();
    assert_eq!(
        j.resume_point(&JobId("b".into()), Stage::Download).unwrap(),
        None
    );
}

#[test]
fn a_completed_stage_reports_complete_after_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.db");
    let job = JobId("x".into());
    {
        let j = Journal::open(&path).unwrap();
        j.complete(&job, Stage::Extract).unwrap();
    }
    let j = Journal::open(&path).unwrap();
    assert!(j.is_complete(&job, Stage::Extract).unwrap());
}

#[test]
fn a_failure_is_recorded_without_losing_the_resume_point() {
    // Discarding the position on error would turn every transient network fault into a
    // restart from zero — on a multi-hour download that is the difference between a product
    // and a toy.
    let (_d, j) = temp_journal();
    let job = JobId("x".into());
    j.mark(&job, Stage::Download, 900, 1000).unwrap();
    j.failed(&job, Stage::Download, "connection reset").unwrap();
    assert_eq!(j.resume_point(&job, Stage::Download).unwrap(), Some(900));
    assert!(!j.is_complete(&job, Stage::Download).unwrap());
    assert_eq!(
        j.last_error(&job, Stage::Download).unwrap().as_deref(),
        Some("connection reset")
    );
}

#[test]
fn recording_progress_clears_a_previous_error() {
    // A job that resumed successfully is no longer failing, and the UI must not keep showing
    // an error for something that is visibly working.
    let (_d, j) = temp_journal();
    let job = JobId("x".into());
    j.failed(&job, Stage::Download, "connection reset").unwrap();
    j.mark(&job, Stage::Download, 950, 1000).unwrap();
    assert_eq!(j.last_error(&job, Stage::Download).unwrap(), None);
}

#[test]
fn completing_a_stage_after_a_failure_clears_the_error() {
    let (_d, j) = temp_journal();
    let job = JobId("x".into());
    j.failed(&job, Stage::Index, "disk full").unwrap();
    j.complete(&job, Stage::Index).unwrap();
    assert!(j.is_complete(&job, Stage::Index).unwrap());
    assert_eq!(j.last_error(&job, Stage::Index).unwrap(), None);
}

#[test]
fn progress_reports_both_position_and_total() {
    // The wizard shows an ETA, which needs the denominator as well as the numerator.
    let (_d, j) = temp_journal();
    let job = JobId("x".into());
    j.mark(&job, Stage::Download, 250, 1000).unwrap();
    let p = j
        .progress(&job, Stage::Download)
        .unwrap()
        .expect("progress missing");
    assert_eq!((p.position, p.total), (250, 1000));
    assert!(!p.done);
}

#[test]
fn all_progress_lists_every_recorded_stage() {
    // The dashboard renders the whole pipeline at once, so it needs one call rather than one
    // per stage per corpus.
    let (_d, j) = temp_journal();
    let a = JobId("a".into());
    let b = JobId("b".into());
    j.mark(&a, Stage::Download, 1, 10).unwrap();
    j.complete(&a, Stage::Download).unwrap();
    j.mark(&a, Stage::Index, 5, 10).unwrap();
    j.mark(&b, Stage::Download, 3, 10).unwrap();

    let all = j.all_progress().unwrap();
    assert_eq!(all.len(), 3);
    assert!(all
        .iter()
        .any(|p| p.job.0 == "a" && p.stage == Stage::Index && p.position == 5));
    assert!(all
        .iter()
        .any(|p| p.job.0 == "b" && p.stage == Stage::Download));
}

#[test]
fn a_stage_can_be_reset_to_start_over() {
    // Needed when a hash check fails: the bytes on disk are wrong, so resuming from the
    // recorded offset would append good data onto bad and fail the hash again for ever.
    let (_d, j) = temp_journal();
    let job = JobId("x".into());
    j.mark(&job, Stage::Download, 900, 1000).unwrap();
    j.reset(&job, Stage::Download).unwrap();
    assert_eq!(j.resume_point(&job, Stage::Download).unwrap(), None);
    assert!(!j.is_complete(&job, Stage::Download).unwrap());
}
