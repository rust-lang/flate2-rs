use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const KNOWN_GOOD_COMMIT: &str = "b9afa93d70e19a213a3594190e31fb39d83aba0e";
const KNOWN_GOOD_COMMIT_ENV: &str = "FLATE2_BACKEND_BENCH_KNOWN_GOOD_COMMIT";
const DRIVER_LABEL_ENV: &str = "FLATE2_BACKEND_BENCH_LABEL";
const DRIVER_COMPARE_UNINIT_CFG: &str = "--cfg flate2_compare_uninit_cases";
const DRIVER_UNINIT_CFG: &str = "--cfg flate2_has_uninit_api";
const MIN_MEASUREMENT_SLACK_FACTOR: f64 = 0.10;

#[derive(Clone)]
struct MeasurementRecord {
    backend: String,
    case: String,
    iterations_per_sample: usize,
    samples: usize,
    ns_per_byte: f64,
    measurement_uncertainty: f64,
}

struct BenchmarkResult {
    case: String,
    iterations_per_sample: usize,
    samples: usize,
    ns_per_byte: f64,
    measurement_uncertainty: f64,
    baseline_iterations_per_sample: usize,
    baseline_samples: usize,
    baseline_ns_per_byte: f64,
    baseline_measurement_uncertainty: f64,
}

#[derive(Clone, Copy)]
struct BackendConfig {
    name: &'static str,
    driver_feature: &'static str,
    compare_uninit_against_legacy_baseline: bool,
}

fn known_good_commit() -> String {
    env::var(KNOWN_GOOD_COMMIT_ENV).unwrap_or_else(|_| KNOWN_GOOD_COMMIT.to_owned())
}

fn parse_measurement_record(line: &str) -> MeasurementRecord {
    let mut fields = line.split(',');
    let backend = fields
        .next()
        .expect("missing backend field in benchmark CSV")
        .trim()
        .to_owned();
    let case = fields
        .next()
        .expect("missing case field in benchmark CSV")
        .trim()
        .to_owned();
    let iterations_per_sample = fields
        .next()
        .expect("missing iterations_per_sample field in benchmark CSV")
        .trim()
        .parse()
        .expect("invalid iterations_per_sample field in benchmark CSV");
    let samples = fields
        .next()
        .expect("missing samples field in benchmark CSV")
        .trim()
        .parse()
        .expect("invalid samples field in benchmark CSV");
    let ns_per_byte = fields
        .next()
        .expect("missing ns_per_byte field in benchmark CSV")
        .trim()
        .parse()
        .expect("invalid ns_per_byte field in benchmark CSV");
    let measurement_uncertainty = fields
        .next()
        .expect("missing measurement_uncertainty field in benchmark CSV")
        .trim()
        .parse()
        .expect("invalid measurement_uncertainty field in benchmark CSV");
    assert!(
        fields.next().is_none(),
        "unexpected trailing benchmark CSV fields"
    );
    MeasurementRecord {
        backend,
        case,
        iterations_per_sample,
        samples,
        ns_per_byte,
        measurement_uncertainty,
    }
}

fn merge_measurements(
    backend: &str,
    current: &[MeasurementRecord],
    baseline: &[MeasurementRecord],
) -> Vec<BenchmarkResult> {
    current
        .iter()
        .map(|current| {
            let baseline = baseline
                .iter()
                .find(|baseline| baseline.backend == backend && baseline.case == current.case)
                .unwrap_or_else(|| {
                    panic!(
                        "missing baseline for backend={backend}, case={}",
                        current.case
                    )
                });
            BenchmarkResult {
                case: current.case.clone(),
                iterations_per_sample: current.iterations_per_sample,
                samples: current.samples,
                ns_per_byte: current.ns_per_byte,
                measurement_uncertainty: current.measurement_uncertainty,
                baseline_iterations_per_sample: baseline.iterations_per_sample,
                baseline_samples: baseline.samples,
                baseline_ns_per_byte: baseline.ns_per_byte,
                baseline_measurement_uncertainty: baseline.measurement_uncertainty,
            }
        })
        .collect()
}

fn allowed_slowdown_factor(result: &BenchmarkResult) -> f64 {
    1.0 + measurement_slack_factor(result)
}

fn allowed_ns_per_byte(result: &BenchmarkResult) -> f64 {
    result.baseline_ns_per_byte * allowed_slowdown_factor(result)
}

fn slowdown_factor(result: &BenchmarkResult) -> f64 {
    result.ns_per_byte / result.baseline_ns_per_byte
}

fn measurement_slack_factor(result: &BenchmarkResult) -> f64 {
    (result.measurement_uncertainty + result.baseline_measurement_uncertainty)
        .max(MIN_MEASUREMENT_SLACK_FACTOR)
}

fn failure_summary(result: &BenchmarkResult) -> String {
    format!(
        "{}: {:.2}x slowdown of {:.2}x allowed, measured {:.3} ns/byte, baseline {:.3} ns/byte",
        result.case,
        slowdown_factor(result),
        allowed_slowdown_factor(result),
        result.ns_per_byte,
        result.baseline_ns_per_byte,
    )
}

fn status_for(result: &BenchmarkResult) -> &'static str {
    if result.ns_per_byte <= allowed_ns_per_byte(result) {
        "pass"
    } else {
        "fail"
    }
}

fn results_dir() -> PathBuf {
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("target"));
    target_dir.join("backend-bench")
}

fn results_csv_path(backend: &str) -> PathBuf {
    results_dir().join(format!("{backend}.csv"))
}

fn repo_relative_display_path(path: &Path) -> String {
    path.strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")))
        .unwrap_or(path)
        .display()
        .to_string()
}

fn write_results_csv(backend: &str, results: &[BenchmarkResult]) -> PathBuf {
    let dir = results_dir();
    fs::create_dir_all(&dir).unwrap();

    let mut csv = String::from(
        "backend,case,iterations_per_sample,samples,ns_per_byte,measurement_uncertainty,baseline_iterations_per_sample,baseline_samples,baseline_ns_per_byte,baseline_measurement_uncertainty,allowed_slowdown_factor,allowed_ns_per_byte,slowdown_factor,measurement_slack_factor,status\n",
    );
    for result in results {
        csv.push_str(&format!(
            "{backend},{},{},{},{:.9},{:.6},{},{},{:.9},{:.6},{:.6},{:.9},{:.6},{:.6},{}\n",
            result.case,
            result.iterations_per_sample,
            result.samples,
            result.ns_per_byte,
            result.measurement_uncertainty,
            result.baseline_iterations_per_sample,
            result.baseline_samples,
            result.baseline_ns_per_byte,
            result.baseline_measurement_uncertainty,
            allowed_slowdown_factor(result),
            allowed_ns_per_byte(result),
            slowdown_factor(result),
            measurement_slack_factor(result),
            status_for(result),
        ));
    }

    let path = results_csv_path(backend);
    fs::write(&path, csv).unwrap();
    path
}

fn write_measurement_csv(path: &Path, records: &[MeasurementRecord], comment: Option<&str>) {
    let parent = path
        .parent()
        .expect("measurement CSV output must have a parent directory");
    fs::create_dir_all(parent).unwrap();

    let mut csv = String::new();
    if let Some(comment) = comment {
        csv.push('#');
        csv.push(' ');
        csv.push_str(comment);
        csv.push('\n');
    }
    csv.push_str(
        "backend,case,iterations_per_sample,samples,ns_per_byte,measurement_uncertainty\n",
    );
    for record in records {
        csv.push_str(&format!(
            "{},{},{},{},{:.9},{:.6}\n",
            record.backend,
            record.case,
            record.iterations_per_sample,
            record.samples,
            record.ns_per_byte,
            record.measurement_uncertainty,
        ));
    }

    fs::write(path, csv).unwrap();
}

fn status(message: impl AsRef<str>) {
    eprintln!("[backend-regression] {}", message.as_ref());
}

fn unique_temp_dir(backend: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    env::temp_dir().join(format!(
        "flate2-backend-regression-{backend}-{}-{nanos}",
        process::id()
    ))
}

fn checked_output(command: &mut Command, context: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|err| panic!("failed to {}: {}", context, err));
    assert!(
        output.status.success(),
        "failed to {context} (status {}):\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

fn checked_output_with_inherited_stderr(command: &mut Command, context: &str) -> Output {
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|err| panic!("failed to {}: {}", context, err));
    let output = child
        .wait_with_output()
        .unwrap_or_else(|err| panic!("failed to {}: {}", context, err));
    assert!(
        output.status.success(),
        "failed to {context} (status {}):\nstdout:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
    );
    output
}

struct KnownGoodWorktree {
    repo_root: PathBuf,
    path: PathBuf,
}

impl KnownGoodWorktree {
    fn create(repo_root: &Path, commit: &str, path: &Path) -> Self {
        checked_output(
            Command::new("git")
                .arg("-C")
                .arg(repo_root)
                .arg("cat-file")
                .arg("-e")
                .arg(format!("{commit}^{{commit}}")),
            &format!(
                "verify known-good commit {commit} is available locally; fetch more history if needed"
            ),
        );
        checked_output(
            Command::new("git")
                .arg("-C")
                .arg(repo_root)
                .arg("worktree")
                .arg("add")
                .arg("--detach")
                .arg("--force")
                .arg(path)
                .arg(commit),
            &format!("create worktree for known-good commit {commit}"),
        );
        Self {
            repo_root: repo_root.to_path_buf(),
            path: path.to_path_buf(),
        }
    }
}

impl Drop for KnownGoodWorktree {
    fn drop(&mut self) {
        if let Err(err) = Command::new("git")
            .arg("-C")
            .arg(&self.repo_root)
            .arg("worktree")
            .arg("remove")
            .arg("--force")
            .arg(&self.path)
            .output()
        {
            eprintln!(
                "failed to remove temporary worktree {}: {}",
                self.path.display(),
                err
            );
        }
    }
}

fn escaped_toml_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "\\\\")
}

fn driver_manifest(crate_path: &Path) -> String {
    format!(
        r#"[package]
name = "flate2-backend-driver"
version = "0.1.0"
edition = "2021"

[dependencies]
flate2 = {{ path = "{}", default-features = false }}

[features]
default = []
"rust_backend" = ["flate2/rust_backend"]
"zlib-rs" = ["flate2/zlib-rs"]
"zlib" = ["flate2/zlib"]
"zlib-ng" = ["flate2/zlib-ng"]
"zlib-ng-compat" = ["flate2/zlib-ng-compat"]
"#,
        escaped_toml_path(crate_path)
    )
}

fn driver_source_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
        .join("backend-regression-driver.rs")
}

fn crate_has_uninit_api(crate_path: &Path) -> bool {
    fs::read_to_string(crate_path.join("src").join("mem.rs"))
        .map(|mem_rs| {
            mem_rs.contains("pub fn compress_uninit(")
                && mem_rs.contains("pub fn decompress_uninit(")
        })
        .unwrap_or(false)
}

fn commit_has_uninit_api(repo_root: &Path, commit: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("show")
        .arg(format!("{commit}:src/mem.rs"))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|mem_rs| {
            mem_rs.contains("pub fn compress_uninit(")
                && mem_rs.contains("pub fn decompress_uninit(")
        })
        .unwrap_or(false)
}

fn driver_rustflags(include_uninit_cases: bool, has_uninit_api: bool) -> Option<String> {
    if !include_uninit_cases && !has_uninit_api {
        return None;
    }

    let mut extra_flags = Vec::new();
    if include_uninit_cases {
        extra_flags.push(DRIVER_COMPARE_UNINIT_CFG);
    }
    if has_uninit_api {
        extra_flags.push(DRIVER_UNINIT_CFG);
    }
    let extra_flags = extra_flags.join(" ");

    match env::var("RUSTFLAGS") {
        Ok(existing) if !existing.trim().is_empty() => Some(format!("{existing} {extra_flags}")),
        _ => Some(extra_flags),
    }
}

fn run_driver(
    backend: BackendConfig,
    crate_path: &Path,
    driver_path: &Path,
    cargo_target_dir: &Path,
    label: &str,
    compare_uninit: bool,
    context: &str,
) -> Vec<MeasurementRecord> {
    fs::create_dir_all(driver_path.join("src")).unwrap();
    fs::write(driver_path.join("Cargo.toml"), driver_manifest(crate_path)).unwrap();
    fs::copy(
        driver_source_path(),
        driver_path.join("src").join("main.rs"),
    )
    .unwrap();

    let mut command = Command::new("cargo");
    command
        .arg("run")
        .arg("--release")
        .arg("--features")
        .arg(backend.driver_feature)
        .current_dir(driver_path)
        .env("CARGO_TARGET_DIR", cargo_target_dir)
        .env(DRIVER_LABEL_ENV, label);
    if let Some(rustflags) = driver_rustflags(compare_uninit, crate_has_uninit_api(crate_path)) {
        command.env("RUSTFLAGS", rustflags);
    }

    let output = checked_output_with_inherited_stderr(&mut command, context);

    String::from_utf8(output.stdout)
        .expect("driver output must be valid UTF-8")
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(parse_measurement_record)
        .collect()
}

fn measure_current(
    backend: BackendConfig,
    temp_root: &Path,
    compare_uninit: bool,
) -> Vec<MeasurementRecord> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    status(format!(
        "running driver for current {} checkout at {}",
        backend.name,
        repo_root.display()
    ));
    run_driver(
        backend,
        repo_root,
        &temp_root.join("current-driver"),
        &temp_root.join("current-target"),
        "current checkout",
        compare_uninit,
        &format!("run current driver for {}", backend.name),
    )
}

fn measure_known_good(
    backend: BackendConfig,
    temp_root: &Path,
    commit: &str,
    compare_uninit: bool,
) -> Vec<MeasurementRecord> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let worktree_path = temp_root.join("known-good");
    status(format!(
        "creating detached worktree for known-good commit {} at {}",
        commit,
        worktree_path.display()
    ));
    let _worktree = KnownGoodWorktree::create(repo_root, commit, &worktree_path);
    status(format!(
        "running driver for {} at known-good commit {}",
        backend.name, commit
    ));
    run_driver(
        backend,
        &worktree_path,
        &temp_root.join("baseline-driver"),
        &temp_root.join("baseline-target"),
        &format!("known-good commit {commit}"),
        compare_uninit,
        &format!(
            "run baseline driver for {} at commit {commit}",
            backend.name
        ),
    )
}

#[cfg(feature = "zlib-ng")]
fn backend_config() -> BackendConfig {
    BackendConfig {
        name: "zlib-ng",
        driver_feature: "zlib-ng",
        compare_uninit_against_legacy_baseline: true,
    }
}

#[cfg(all(not(feature = "zlib-ng"), feature = "zlib-ng-compat"))]
fn backend_config() -> BackendConfig {
    BackendConfig {
        name: "zlib-ng-compat",
        driver_feature: "zlib-ng-compat",
        compare_uninit_against_legacy_baseline: true,
    }
}

#[cfg(all(
    not(feature = "zlib-ng"),
    not(feature = "zlib-ng-compat"),
    feature = "zlib-rs"
))]
fn backend_config() -> BackendConfig {
    BackendConfig {
        name: "zlib-rs",
        driver_feature: "zlib-rs",
        compare_uninit_against_legacy_baseline: true,
    }
}

#[cfg(all(
    not(feature = "zlib-ng"),
    not(feature = "zlib-ng-compat"),
    not(feature = "zlib-rs"),
    any(
        feature = "zlib",
        feature = "zlib-default",
        feature = "cloudflare_zlib"
    )
))]
fn backend_config() -> BackendConfig {
    BackendConfig {
        name: "zlib",
        driver_feature: "zlib",
        compare_uninit_against_legacy_baseline: true,
    }
}

#[cfg(all(
    not(feature = "zlib-ng"),
    not(feature = "zlib-ng-compat"),
    not(feature = "zlib-rs"),
    not(feature = "zlib"),
    not(feature = "zlib-default"),
    not(feature = "cloudflare_zlib")
))]
fn backend_config() -> BackendConfig {
    BackendConfig {
        name: "rust_backend",
        driver_feature: "rust_backend",
        compare_uninit_against_legacy_baseline: false,
    }
}

#[test]
#[ignore]
fn backend_regression_bench() {
    let backend = backend_config();
    let commit = known_good_commit();
    let temp_root = unique_temp_dir(backend.name);
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let compare_uninit = crate_has_uninit_api(repo_root)
        && (commit_has_uninit_api(repo_root, &commit)
            || backend.compare_uninit_against_legacy_baseline);

    status(format!(
        "starting backend regression benchmark for {} against known-good commit {}",
        backend.name, commit
    ));
    status("generating known-good baseline measurements");
    if !compare_uninit {
        status(
            "skipping uninit benchmark cases because they are not supported by both current and known-good revisions",
        );
    }
    let baselines = measure_known_good(backend, &temp_root, &commit, compare_uninit);
    status(format!(
        "benchmarking current {} backend via driver",
        backend.name
    ));
    let current = measure_current(backend, &temp_root, compare_uninit);
    let results = merge_measurements(backend.name, &current, &baselines);

    status("writing benchmark artifacts");
    let dir = results_dir();
    write_measurement_csv(
        &dir.join(format!("{}-baseline.csv", backend.name)),
        &baselines,
        Some(&format!("Generated on the fly from commit {commit}.")),
    );
    write_measurement_csv(
        &dir.join(format!("{}-current.csv", backend.name)),
        &current,
        Some("Generated from the current checkout."),
    );
    let results_csv_path = write_results_csv(backend.name, &results);
    status(format!(
        "wrote benchmark results to {}",
        repo_relative_display_path(&results_csv_path)
    ));

    fs::remove_dir_all(&temp_root).unwrap_or_else(|err| {
        panic!(
            "failed to remove temporary benchmark directory {}: {}",
            temp_root.display(),
            err
        )
    });

    let failures: Vec<_> = results
        .iter()
        .filter(|result| result.ns_per_byte > allowed_ns_per_byte(result))
        .collect();
    assert!(
        failures.is_empty(),
        "backend regression benchmark failures for {} against known-good commit {}:\n  {}",
        backend.name,
        commit,
        failures
            .iter()
            .map(|result| failure_summary(result))
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    status("benchmark completed without threshold failures");
}
