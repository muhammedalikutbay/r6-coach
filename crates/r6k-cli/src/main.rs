//! Developer CLI for R6Koc.
//!
//!   r6k ingest <MatchReplay dir> --dissect <exe> [--cache <dir>] [--out <file>]
//!   r6k baseline --show

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use r6k_core::{names, Baseline, Match};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_CACHE: &str = r"C:\Users\Kutbay\Desktop\R6Koc-veri\dissect";

#[derive(Parser)]
#[command(name = "r6k", about = "R6Koc developer CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse every match folder and print a summary table.
    Ingest {
        /// Folder that contains one sub-folder per match (the MatchReplay folder).
        replays: PathBuf,
        /// Path to the r6-dissect executable.
        #[arg(long)]
        dissect: PathBuf,
        /// Where parsed JSON files are cached (one per match folder).
        #[arg(long, default_value = DEFAULT_CACHE)]
        cache: PathBuf,
        /// Also write the converted matches as JSON to this file.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Inspect the embedded personal baseline (data/baseline.json).
    Baseline {
        /// Print the 10 most played operators (rounds, K/D, HS%).
        #[arg(long)]
        show: bool,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match Cli::parse().cmd {
        Cmd::Ingest {
            replays,
            dissect,
            cache,
            out,
        } => ingest(&replays, &dissect, &cache, out.as_deref()),
        Cmd::Baseline { show } => baseline(show),
    }
}

fn baseline(show: bool) -> Result<()> {
    let b = Baseline::embedded().context("embedded baseline is invalid")?;
    println!(
        "baseline: {} operators, {} maps, {} gamemodes (captured {})",
        b.operators.len(),
        b.maps.len(),
        b.gamemodes.len(),
        b.captured_at
    );
    if show {
        println!("operator         rounds   K/D    HS%");
        for (slug, c) in b.top_operators(10) {
            println!(
                "{:<16} {:>6}   {:>5.2}  {:>5.1}%",
                slug,
                c.rounds_played,
                c.kd(),
                c.headshot_pct()
            );
        }
    }
    Ok(())
}

fn ingest(replays: &Path, dissect: &Path, cache: &Path, out: Option<&Path>) -> Result<()> {
    std::fs::create_dir_all(cache)
        .with_context(|| format!("creating cache {}", cache.display()))?;
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(replays)
        .with_context(|| format!("reading {}", replays.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    log::info!("{} match folders in {}", dirs.len(), replays.display());

    let mut matches: Vec<Match> = Vec::new();
    for dir in &dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let json = cache.join(format!("{name}.json"));
        let cached = std::fs::metadata(&json)
            .map(|m| m.len() > 0)
            .unwrap_or(false);
        if cached {
            log::debug!("{name}: using cache");
        } else {
            log::info!("{name}: running dissect");
            let status = Command::new(dissect)
                .arg(dir)
                .arg("-o")
                .arg(&json)
                .output()
                .with_context(|| format!("starting {}", dissect.display()))?;
            if !status.status.success() {
                let _ = std::fs::remove_file(&json);
                log::error!(
                    "{name}: dissect failed: {}",
                    String::from_utf8_lossy(&status.stderr)
                );
                continue;
            }
        }
        match r6k_core::read_match_file(&json) {
            Ok(m) => matches.push(m),
            Err(e) => log::error!("{name}: {e}"),
        }
    }
    if matches.is_empty() {
        bail!("no matches could be read");
    }
    matches.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    print_table(&matches);
    if let Some(path) = out {
        std::fs::write(path, serde_json::to_string_pretty(&matches)?)?;
        log::info!("wrote {}", path.display());
    }
    Ok(())
}

fn print_table(matches: &[Match]) {
    println!(
        "{:<16} {:<18} {:<4} {:>3} {:>3} {:>3} {:>5}  Operators (rounds)",
        "Date (UTC)", "Map", "Res", "K", "D", "A", "HS%"
    );
    for m in matches {
        let mut ops: BTreeMap<u64, u32> = BTreeMap::new();
        for r in &m.rounds {
            *ops.entry(r.operator_id).or_default() += 1;
        }
        let mut ops: Vec<_> = ops.into_iter().collect();
        ops.sort_by_key(|a| std::cmp::Reverse(a.1));
        let ops = ops
            .iter()
            .map(|(id, n)| format!("{} ({n})", names::operator_name(*id)))
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{:<16} {:<18} {:<4} {:>3} {:>3} {:>3} {:>4.0}%  {}",
            m.timestamp
                .replace('T', " ")
                .chars()
                .take(16)
                .collect::<String>(),
            names::map_name(m.map_id),
            if m.won { "Win" } else { "Loss" },
            m.kills(),
            m.deaths(),
            m.assists(),
            m.headshot_pct(),
            ops
        );
    }
    let (k, d): (u32, u32) = matches
        .iter()
        .fold((0, 0), |a, m| (a.0 + m.kills(), a.1 + m.deaths()));
    let wins = matches.iter().filter(|m| m.won).count();
    println!(
        "\n{} matches, {} wins, K/D {:.2} ({k}/{d})",
        matches.len(),
        wins,
        if d == 0 {
            k as f64
        } else {
            k as f64 / d as f64
        }
    );
}
