//! Golden-trace regression against MATLAB references.
//!
//! Each `test-projects/NN-*.json` has a matching `NN-*.m` that reconstructs the
//! system in MATLAB, discretizes continuous blocks with `c2d(..., 'zoh')`, runs
//! the same per-sample block-diagram recursion the firmware executes, and writes
//! `NN-*.ref.csv` (a trace keyed by backend node id). These tests replay the
//! backend on the same project and compare every signal sample-by-sample.
//!
//! Regenerate the references with MATLAB, e.g.:
//!   matlab -batch "cd('test-projects'); eval(fileread('04-2nd-order-system.m'))"
//!
//! If a reference CSV is absent (MATLAB not run yet), the case is skipped rather
//! than failed, so the suite stays green without MATLAB installed.

use std::path::{Path, PathBuf};

/// Absolute tolerance. Continuous ZOH is exact at the sample instants, and the
/// discrete engine is exact, so agreement is far tighter than this; the margin
/// only absorbs the reference file's 12-significant-digit text precision.
const TOLERANCE: f64 = 1e-6;

fn projects_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("test-projects")
}

struct Reference {
    columns: Vec<String>,
    rows: Vec<Vec<f64>>,
}

fn load_reference(path: &Path) -> Reference {
    let text = std::fs::read_to_string(path).expect("reference csv must be readable");
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header = lines.next().expect("reference csv must have a header");
    let columns = header.split(',').map(|name| name.trim().to_string()).collect();
    let rows = lines
        .map(|line| {
            line.split(',')
                .map(|value| value.trim().parse::<f64>().expect("reference values must be numeric"))
                .collect()
        })
        .collect();
    Reference { columns, rows }
}

fn compare_against_reference(project: &str) {
    let dir = projects_dir();
    let csv = dir.join(format!("{project}.ref.csv"));
    if !csv.exists() {
        eprintln!(
            "skipping {project}: no {project}.ref.csv \
             (run its .m file in MATLAB to generate one)"
        );
        return;
    }

    let json =
        std::fs::read_to_string(dir.join(format!("{project}.json"))).expect("project json readable");
    let output = ctrl_backend::simulate_project_json(&json).expect("simulation must succeed");
    let reference = load_reference(&csv);

    assert_eq!(
        reference.rows.len(),
        output.times.len(),
        "{project}: sample count differs (reference {}, backend {})",
        reference.rows.len(),
        output.times.len()
    );

    for (column, name) in reference.columns.iter().enumerate() {
        if column == 0 {
            // Time column.
            for (k, row) in reference.rows.iter().enumerate() {
                let delta = (row[0] - output.times[k]).abs();
                assert!(
                    delta <= TOLERANCE,
                    "{project} t[{k}]: reference {} vs backend {} (delta {delta})",
                    row[0],
                    output.times[k]
                );
            }
            continue;
        }

        let series = output
            .values_by_node_id
            .get(name)
            .unwrap_or_else(|| panic!("{project}: backend produced no signal for node `{name}`"));

        for (k, row) in reference.rows.iter().enumerate() {
            let delta = (row[column] - series[k]).abs();
            assert!(
                delta <= TOLERANCE,
                "{project} `{name}`[{k}] (t={:.4}): reference {} vs backend {} (delta {delta})",
                output.times[k],
                row[column],
                series[k]
            );
        }
    }

    eprintln!(
        "{project}: matched {} samples across {} signals within {TOLERANCE:e}",
        reference.rows.len(),
        reference.columns.len() - 1
    );
}

#[test]
fn golden_double_integrator() {
    compare_against_reference("01-double-integrator");
}

#[test]
fn golden_feedback_tf() {
    compare_against_reference("02-feedback-TF");
}

#[test]
fn golden_tf_test() {
    compare_against_reference("03-TF-test");
}

#[test]
fn golden_second_order_system() {
    compare_against_reference("04-2nd-order-system");
}

// ---------------------------------------------------------------------------
// Firmware contract vectors
// ---------------------------------------------------------------------------
//
// `NN-*.plan.dcp` and `NN-*.f32.csv` are the artifacts the firmware is graded
// against: the exact bytes it will load, and the exact f32 trace its kernels
// must reproduce. They are committed, so a change to parameter packing, kernel
// arithmetic, or the wire format shows up here as a diff rather than as a
// mystery on the bench.
//
// Regenerate deliberately, never to make this test pass:
//   cargo run --manifest-path backend/Cargo.toml -- \
//     --emit-plan  test-projects/NN-*.plan.dcp test-projects/NN-*.json
//   cargo run --manifest-path backend/Cargo.toml -- \
//     --emit-trace test-projects/NN-*.f32.csv  test-projects/NN-*.json

const PROJECTS: [&str; 5] = [
    "01-double-integrator",
    "02-feedback-TF",
    "03-TF-test",
    "04-2nd-order-system",
    // Plant alone, driven by a step. Exists because the firmware runtime has to
    // be able to run a *plant* as a plan with no new code - the claim the
    // two-board HIL loop rests on. Its coefficients are lifted unchanged from
    // 04 so the two are directly comparable.
    "05-plant-only",
];

#[test]
fn committed_plans_match_freshly_compiled_bytes() {
    for project in PROJECTS {
        let dir = projects_dir();
        let json = std::fs::read_to_string(dir.join(format!("{project}.json")))
            .expect("project json readable");
        let dag = ctrl_backend::parse_project_json(&json).expect("project must parse");
        let plan = ctrl_backend::plan::build_control_plan(&dag).expect("plan must build");

        let committed = std::fs::read(dir.join(format!("{project}.plan.dcp")))
            .expect("committed plan must exist");
        let fresh = ctrl_backend::plan::encode(&plan);

        assert_eq!(
            committed.len(),
            fresh.len(),
            "{project}: plan size changed ({} -> {} bytes). The wire format or \
             the packed parameters moved; bump DCP_FORMAT_VERSION or \
             KERNEL_SET_VERSION and regenerate the vectors.",
            committed.len(),
            fresh.len()
        );
        assert_eq!(
            committed, fresh,
            "{project}: compiled plan bytes differ from the committed vector"
        );
    }
}

#[test]
fn committed_f32_traces_match_the_reference_executor() {
    for project in PROJECTS {
        let dir = projects_dir();
        let bytes = std::fs::read(dir.join(format!("{project}.plan.dcp")))
            .expect("committed plan must exist");
        let plan = ctrl_backend::plan::decode(&bytes).expect("committed plan must decode");

        let expected = load_reference(&dir.join(format!("{project}.f32.csv")));
        let trace = ctrl_backend::exec::run(&plan, expected.rows.len())
            .expect("plan execution must succeed");

        for (column, name) in expected.columns.iter().enumerate().skip(1) {
            let series = &trace.signals[column - 1];
            for (k, row) in expected.rows.iter().enumerate() {
                let delta = (row[column] - series[k] as f64).abs();
                assert!(
                    delta <= 1.0e-9,
                    "{project} `{name}`[{k}]: committed {} vs executor {} (delta {delta:e})",
                    row[column],
                    series[k]
                );
            }
        }

        eprintln!(
            "{project}: {} samples x {} signals reproduced from the committed plan",
            expected.rows.len(),
            expected.columns.len() - 1
        );
    }
}
