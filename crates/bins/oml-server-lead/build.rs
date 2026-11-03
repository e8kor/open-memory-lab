//! Captures what `/readyz` reports about this build (docs/api/http-api.md
//! "Health"): the commit and its time, read from git inside a checkout, or
//! from `OML_BUILD_COMMIT` / `OML_BUILD_COMMIT_TIME` where the build has no
//! repository (the image build); the build time, from `SOURCE_DATE_EPOCH`
//! when set so a pinned build stays reproducible, else the time this script
//! ran; and the compiler, target and profile cargo names. A value nothing can
//! supply is `unknown`, never a guess.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    watch_head();

    let commit = env("OML_BUILD_COMMIT")
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .unwrap_or_else(unknown);
    let commit_time = env("OML_BUILD_COMMIT_TIME")
        .or_else(|| {
            git(&["show", "-s", "--format=%ct", "HEAD"])
                .and_then(|s| s.parse::<i64>().ok())
                .map(utc_timestamp)
        })
        .unwrap_or_else(unknown);
    let build_time = utc_timestamp(
        env("SOURCE_DATE_EPOCH")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or_else(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
            }),
    );
    let rustc = std::env::var("RUSTC")
        .ok()
        .and_then(|rustc| run(Command::new(rustc).arg("-V")))
        .unwrap_or_else(unknown);
    let target = std::env::var("TARGET").unwrap_or_else(|_| unknown());
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| unknown());

    for (name, value) in [
        ("OML_BUILD_COMMIT", commit),
        ("OML_BUILD_COMMIT_TIME", commit_time),
        ("OML_BUILD_TIME", build_time),
        ("OML_BUILD_RUSTC", rustc),
        ("OML_BUILD_TARGET", target),
        ("OML_BUILD_PROFILE", profile),
    ] {
        println!("cargo:rustc-env={name}={value}");
    }
}

fn unknown() -> String {
    "unknown".to_owned()
}

/// A non-empty environment variable, declared as a rebuild trigger.
fn env(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={name}");
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn run(command: &mut Command) -> Option<String> {
    command
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn git(args: &[&str]) -> Option<String> {
    run(Command::new("git").args(args))
}

/// Rebuild when HEAD moves: the HEAD file of this worktree, the ref it names
/// in the common git directory, and packed-refs. Outside a checkout there is
/// nothing to watch, and the build-time variables are the only triggers.
fn watch_head() {
    let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]).map(PathBuf::from) else {
        return;
    };
    let common = git(&["rev-parse", "--git-common-dir"])
        .map(|p| absolute(&p))
        .unwrap_or_else(|| git_dir.clone());
    rerun_if_exists(&git_dir.join("HEAD"));
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        rerun_if_exists(&common.join(reference));
    }
    rerun_if_exists(&common.join("packed-refs"));
}

fn absolute(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map_or(p.clone(), |cwd| cwd.join(p))
    }
}

fn rerun_if_exists(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// Seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ` (the proleptic
/// Gregorian days-from-civil inverse, so the script needs no date crate).
fn utc_timestamp(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}
