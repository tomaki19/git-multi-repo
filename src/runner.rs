// SPDX-FileCopyrightText: 2026 Thomas Killer
// SPDX-License-Identifier: MIT

//! Runs per-repository jobs. Sequential runs stream output live and let
//! commands talk to the terminal; parallel runs buffer each repository's
//! output and print it as one block when that repository is done.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::thread;

/// One command within a job. `depends_on` points at an earlier step of the
/// same job; if that step did not succeed, this one is skipped.
#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub depends_on: Option<usize>,
}

/// Everything to do for one repository. A job with `skip` set runs nothing
/// and is reported as skipped with that reason.
#[derive(Debug, Clone)]
pub struct Job {
    pub name: String,
    pub steps: Vec<Step>,
    pub skip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    /// Exit code, or `None` if the process could not start or was killed.
    Failed(Option<i32>),
    Skipped,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub name: String,
    pub skip: Option<String>,
    pub steps: Vec<(String, Outcome)>,
}

impl Report {
    pub fn failed(&self) -> bool {
        self.steps.iter().any(|(_, outcome)| matches!(outcome, Outcome::Failed(_)))
    }
}

/// Runs all jobs and returns their reports in input order.
pub fn run(jobs: Vec<Job>, parallelism: Option<usize>) -> Vec<Report> {
    match parallelism {
        None => jobs.into_iter().map(run_streaming).collect(),
        Some(workers) => run_parallel(jobs, workers),
    }
}

/// Executes the steps of a job in order, honouring dependencies; `execute`
/// decides how a single step is run (streaming or captured).
fn run_steps(job: Job, mut execute: impl FnMut(&Step) -> Outcome) -> Report {
    let mut results: Vec<(String, Outcome)> = Vec::with_capacity(job.steps.len());
    if job.skip.is_none() {
        for step in &job.steps {
            let blocked = step.depends_on.is_some_and(|dep| results[dep].1 != Outcome::Ok);
            let outcome = if blocked { Outcome::Skipped } else { execute(step) };
            results.push((step.label.clone(), outcome));
        }
    }
    Report { name: job.name, skip: job.skip, steps: results }
}

fn command(step: &Step) -> Command {
    let mut command = Command::new(&step.argv[0]);
    command.args(&step.argv[1..]).current_dir(&step.cwd);
    command
}

fn outcome(status: ExitStatus) -> Outcome {
    if status.success() { Outcome::Ok } else { Outcome::Failed(status.code()) }
}

fn run_streaming(job: Job) -> Report {
    println!("── {} ──", job.name);
    if let Some(reason) = &job.skip {
        println!("skipped: {reason}");
    }
    run_steps(job, |step| match command(step).status() {
        Ok(status) => outcome(status),
        Err(error) => {
            eprintln!("error: cannot run {}: {error}", step.argv[0]);
            Outcome::Failed(None)
        }
    })
}

fn execute_captured(step: &Step, out: &mut Vec<u8>, err: &mut Vec<u8>) -> Outcome {
    match command(step).stdin(Stdio::null()).output() {
        Ok(output) => {
            out.extend_from_slice(&output.stdout);
            err.extend_from_slice(&output.stderr);
            outcome(output.status)
        }
        Err(error) => {
            let _ = writeln!(err, "error: cannot run {}: {error}", step.argv[0]);
            Outcome::Failed(None)
        }
    }
}

/// A fixed pool of scoped worker threads pulls jobs from a shared queue.
/// Each job's output is collected in memory and written under a lock, so
/// blocks from different repositories never interleave.
fn run_parallel(jobs: Vec<Job>, workers: usize) -> Vec<Report> {
    let count = jobs.len();
    let queue = Mutex::new(jobs.into_iter().enumerate());
    let reports: Mutex<Vec<Option<Report>>> = Mutex::new((0..count).map(|_| None).collect());
    let print_lock = Mutex::new(());
    thread::scope(|scope| {
        for _ in 0..workers.clamp(1, count.max(1)) {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().unwrap().next();
                    let Some((index, job)) = next else { break };
                    let mut out = Vec::new();
                    let mut err = Vec::new();
                    let _ = writeln!(out, "── {} ──", job.name);
                    if let Some(reason) = &job.skip {
                        let _ = writeln!(out, "skipped: {reason}");
                    }
                    let report = run_steps(job, |step| execute_captured(step, &mut out, &mut err));
                    {
                        let _guard = print_lock.lock().unwrap();
                        let mut stdout = io::stdout().lock();
                        let _ = stdout.write_all(&out);
                        let _ = stdout.flush();
                        let _ = io::stderr().write_all(&err);
                    }
                    reports.lock().unwrap()[index] = Some(report);
                }
            });
        }
    });
    reports
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|report| report.expect("every job produces a report"))
        .collect()
}

/// One line per repository. With `show_steps`, failed and skipped step
/// labels are appended (used for task runs).
pub fn summarize(reports: &[Report], show_steps: bool) -> String {
    let mut out = String::from("── summary ──\n");
    for report in reports {
        let first_failure = report.steps.iter().find_map(|(_, outcome)| match outcome {
            Outcome::Failed(code) => Some(*code),
            _ => None,
        });
        let status = match (&report.skip, first_failure) {
            (Some(reason), _) => format!("skipped ({reason})"),
            (None, Some(Some(code))) => format!("failed (exit {code})"),
            (None, Some(None)) => "failed".to_owned(),
            (None, None) => "ok".to_owned(),
        };
        out.push_str(&format!("{}: {status}", report.name));
        if show_steps {
            let failed = labels(report, |outcome| matches!(outcome, Outcome::Failed(_)));
            let skipped = labels(report, |outcome| *outcome == Outcome::Skipped);
            if !failed.is_empty() {
                out.push_str(&format!(" [failed: {failed}]"));
            }
            if !skipped.is_empty() {
                out.push_str(&format!(" [skipped: {skipped}]"));
            }
        }
        out.push('\n');
    }
    out
}

fn labels(report: &Report, pick: impl Fn(&Outcome) -> bool) -> String {
    report
        .steps
        .iter()
        .filter(|(_, outcome)| pick(outcome))
        .map(|(label, _)| label.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}


#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn step(label: &str, script: &str, depends_on: Option<usize>) -> Step {
        Step {
            label: label.into(),
            argv: vec!["sh".into(), "-c".into(), script.into()],
            cwd: std::env::temp_dir(),
            depends_on,
        }
    }

    fn job(name: &str, steps: Vec<Step>) -> Job {
        Job { name: name.into(), steps, skip: None }
    }

    fn outcomes(report: &Report) -> Vec<Outcome> {
        report.steps.iter().map(|(_, outcome)| outcome.clone()).collect()
    }

    #[test]
    fn a_failed_step_skips_its_dependents_transitively() {
        let reports = run(
            vec![job(
                "r",
                vec![
                    step("a", "exit 3", None),
                    step("b", "true", Some(0)),
                    step("c", "true", Some(1)),
                    step("d", "true", None),
                ],
            )],
            None,
        );
        assert_eq!(
            outcomes(&reports[0]),
            [Outcome::Failed(Some(3)), Outcome::Skipped, Outcome::Skipped, Outcome::Ok]
        );
        assert!(reports[0].failed());
    }

    #[test]
    fn a_skipped_job_runs_nothing_and_is_not_a_failure() {
        let reports = run(
            vec![Job {
                name: "r".into(),
                steps: vec![step("a", "exit 1", None)],
                skip: Some("directory already exists".into()),
            }],
            None,
        );
        assert!(reports[0].steps.is_empty());
        assert!(!reports[0].failed());
    }

    #[test]
    fn parallel_reports_keep_input_order() {
        let jobs = (0..8)
            .map(|i| job(&format!("r{i}"), vec![step("s", if i % 2 == 0 { "true" } else { "exit 1" }, None)]))
            .collect();
        let reports = run(jobs, Some(3));
        let names: Vec<&str> = reports.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7"]);
        assert!(!reports[0].failed());
        assert!(reports[1].failed());
    }

    #[test]
    fn parallel_steps_get_no_stdin() {
        let reports = run(vec![job("r", vec![step("read", "read line", None)])], Some(1));
        assert!(reports[0].failed());
    }

    #[test]
    fn a_missing_program_is_a_failure_without_exit_code() {
        let mut missing = step("x", "", None);
        missing.argv = vec!["definitely-not-a-program-gmr".into()];
        let reports = run(vec![job("r", vec![missing])], None);
        assert_eq!(outcomes(&reports[0]), [Outcome::Failed(None)]);
    }

    #[test]
    fn summary_lists_status_and_optionally_steps() {
        let reports = vec![
            Report { name: "a".into(), skip: None, steps: vec![("x".into(), Outcome::Ok)] },
            Report {
                name: "b".into(),
                skip: None,
                steps: vec![("x".into(), Outcome::Failed(Some(2))), ("y".into(), Outcome::Skipped)],
            },
            Report { name: "c".into(), skip: Some("directory already exists".into()), steps: vec![] },
            Report { name: "d".into(), skip: None, steps: vec![("x".into(), Outcome::Failed(None))] },
        ];
        assert_eq!(
            summarize(&reports, true),
            "── summary ──\na: ok\nb: failed (exit 2) [failed: x] [skipped: y]\nc: skipped (directory already exists)\nd: failed [failed: x]\n"
        );
        assert_eq!(
            summarize(&reports, false),
            "── summary ──\na: ok\nb: failed (exit 2)\nc: skipped (directory already exists)\nd: failed\n"
        );
    }
}
