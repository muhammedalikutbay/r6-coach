//! `r6k bench`: measures how fast and how much memory the pure Rust pipeline uses.
//!
//! Each iteration parses every cached match JSON, then runs the advice engine (which merges the
//! baseline with the history and produces the stats) once for every match. Timings use
//! `std::time::Instant`; memory is the process working set read with `GetProcessMemoryInfo`.
//! The sidecar (Go) parser is not part of this loop; time it separately.

use anyhow::{Context, Result};
use r6k_core::{advise, read_match_file, Baseline, Match};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Parses `cache` and times `iterations` full passes over the matches.
pub fn run(cache: &Path, iterations: u32) -> Result<()> {
    let files = json_files(cache)?;
    anyhow::ensure!(!files.is_empty(), "no .json files in {}", cache.display());
    let baseline = Baseline::embedded().context("embedded baseline is invalid")?;
    let mem_start = working_set();

    // One untimed pass to warm up the allocator and the page cache.
    let warm = parse_all(&files)?;
    black_box(advise_all(&warm, &baseline));
    drop(warm);

    let mut parse_total = Duration::ZERO;
    let mut advise_total = Duration::ZERO;
    for _ in 0..iterations {
        let t0 = Instant::now();
        let matches = parse_all(&files)?;
        let t1 = Instant::now();
        black_box(advise_all(&matches, &baseline));
        let t2 = Instant::now();
        parse_total += t1 - t0;
        advise_total += t2 - t1;
    }

    let n_matches = files.len();
    let calls = f64::from(iterations) * n_matches as f64;
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!("r6k bench ({profile} build)");
    println!(
        "matches: {n_matches}, iterations: {iterations}, cache: {}",
        cache.display()
    );
    println!();
    println!("{:<14} {:>12} {:>16}", "phase", "total", "per match");
    println!(
        "{:<14} {:>9.1} ms {:>13.1} us",
        "parse JSON",
        ms(parse_total),
        us(parse_total) / calls
    );
    println!(
        "{:<14} {:>9.1} ms {:>13.1} us",
        "stats+advice",
        ms(advise_total),
        us(advise_total) / calls
    );
    let all = parse_total + advise_total;
    println!(
        "{:<14} {:>9.1} ms {:>13.1} us",
        "all",
        ms(all),
        us(all) / calls
    );
    println!(
        "per iteration (30 matches): {:.2} ms",
        ms(all) / f64::from(iterations)
    );
    println!();
    match (mem_start, working_set()) {
        (Some(start), Some((cur, peak))) => {
            println!(
                "working set: start {:.1} MB, now {:.1} MB, peak {:.1} MB",
                mb(start.0),
                mb(cur),
                mb(peak)
            );
        }
        _ => println!("working set: not available on this platform"),
    }
    Ok(())
}

/// Sorted `*.json` paths in the cache folder.
fn json_files(cache: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(cache)
        .with_context(|| format!("reading {}", cache.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    Ok(files)
}

fn parse_all(files: &[PathBuf]) -> Result<Vec<Match>> {
    let mut matches: Vec<Match> = files
        .iter()
        .map(|p| read_match_file(p).with_context(|| format!("parsing {}", p.display())))
        .collect::<Result<_>>()?;
    matches.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    Ok(matches)
}

/// Advice for every match, each one compared with the full history.
fn advise_all(matches: &[Match], baseline: &Baseline) -> usize {
    matches
        .iter()
        .map(|m| advise(m, matches, baseline).len())
        .sum()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// `(current, peak)` working set in bytes, or `None` when the API is unavailable.
#[cfg(windows)]
fn working_set() -> Option<(usize, usize)> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: `counters` is a valid, correctly sized PROCESS_MEMORY_COUNTERS and the
    // pseudo-handle from GetCurrentProcess needs no cleanup.
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    (ok != 0).then_some((counters.WorkingSetSize, counters.PeakWorkingSetSize))
}

#[cfg(not(windows))]
fn working_set() -> Option<(usize, usize)> {
    None
}
