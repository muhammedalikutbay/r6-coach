//! `r6k match`: prints the full scoreboard and round summary of one match.

use anyhow::{bail, Context, Result};
use r6k_core::{
    names, player_match_stats, read_full_match_file, utc_to_local_display, FullMatch, FullRound,
    PlayerMatchStats, Side, WinCondition,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run(target: &str, replays: &Path, dissect: Option<&Path>, cache: &Path) -> Result<()> {
    let m = find(target, replays, dissect, cache)?;
    print_match(&m);
    Ok(())
}

fn find(target: &str, _replays: &Path, dissect: Option<&Path>, cache: &Path) -> Result<FullMatch> {
    // A match folder path: use (or create) its cached dissect JSON.
    let as_path = PathBuf::from(target);
    if as_path.is_dir() {
        let name = as_path.file_name().context("bad folder path")?.to_string_lossy().to_string();
        let json = cache.join(format!("{name}.json"));
        if !json.exists() {
            let dissect = dissect.context("folder is not cached and no --dissect given")?;
            std::fs::create_dir_all(cache)?;
            let out = Command::new(dissect).arg(&as_path).arg("-o").arg(&json).output()?;
            if !out.status.success() {
                let _ = std::fs::remove_file(&json);
                bail!("dissect failed: {}", String::from_utf8_lossy(&out.stderr));
            }
        }
        return read_full_match_file(&json).map_err(Into::into);
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(cache)
        .with_context(|| format!("reading cache {}", cache.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();

    // 1. folder name (exact stem or substring)
    let by_name: Vec<&PathBuf> = files
        .iter()
        .filter(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().contains(target)))
        .collect();
    if by_name.len() == 1 {
        return read_full_match_file(by_name[0]).map_err(Into::into);
    }
    // 2. match ID prefix
    let mut hits = Vec::new();
    for f in &files {
        match read_full_match_file(f) {
            Ok(m) if m.match_id.starts_with(target) => hits.push(m),
            Ok(_) => {}
            Err(e) => log::warn!("{}: {e}", f.display()),
        }
    }
    match hits.len() {
        1 => Ok(hits.remove(0)),
        0 => bail!("no match with id prefix or folder name {target}"),
        n => bail!("{n} matches start with {target}; give more characters"),
    }
}

fn win_condition_label(w: WinCondition) -> &'static str {
    match w {
        WinCondition::Elimination => "elimination",
        WinCondition::DefuserDisabled => "defuser disabled",
        WinCondition::BombDefused => "bomb defused",
        WinCondition::Planted => "planted",
        WinCondition::Time => "time",
        WinCondition::Unknown => "?",
    }
}

fn side_label(s: Side) -> &'static str {
    match s {
        Side::Attack => "ATK",
        Side::Defense => "DEF",
        Side::Unknown => "?",
    }
}

fn print_match(m: &FullMatch) {
    println!(
        "{} | {} | {} | {} | {}\n{} {}-{} ({} rounds)",
        m.match_id,
        utc_to_local_display(&m.timestamp),
        m.match_type,
        m.gamemode,
        names::map_name(m.map_id),
        if m.won { "WIN" } else { "LOSS" },
        m.score_us,
        m.score_them,
        m.rounds.len()
    );

    let stats = player_match_stats(m);
    let mine = m.my_team_index();
    for (label, ours) in [("YOUR TEAM", true), ("ENEMY TEAM", false)] {
        println!(
            "\n{label}\n{:<18} {:>3} {:>3} {:>5} {:>3} {:>3} {:>3} {:>3} {:>3} {:>5} {:>5}  Operators (rounds)",
            "Player", "K", "D", "HS%", "FK", "FD", "MK", "CW", "CL", "KOST", "Surv"
        );
        let (mut tk, mut td) = (0, 0);
        for s in stats.iter().filter(|s| (Some(s.team_index) == mine) == ours) {
            tk += s.kills;
            td += s.deaths;
            println!(
                "{:<18} {:>3} {:>3} {:>4.0}% {:>3} {:>3} {:>3} {:>3} {:>3} {:>4.0}% {:>4.0}%  {}{}",
                s.username,
                s.kills,
                s.deaths,
                s.headshot_pct(),
                s.first_kills,
                s.first_deaths,
                s.multi_kills.total(),
                s.clutches_won,
                s.clutches_lost,
                s.kost_pct(),
                s.survival_pct(),
                operators(m, s),
                if s.profile_id == m.me_profile_id { "  <- you" } else { "" }
            );
        }
        println!("{:<18} {tk:>3} {td:>3}", "Total");
    }

    println!("\nRounds");
    for r in &m.rounds {
        print_round(m, r, &stats);
    }
}

fn operators(m: &FullMatch, s: &PlayerMatchStats) -> String {
    let mut ops: BTreeMap<u64, u32> = BTreeMap::new();
    for r in &m.rounds {
        if let Some(p) = r.players.iter().find(|p| p.profile_id == s.profile_id) {
            *ops.entry(p.operator_id).or_default() += 1;
        }
    }
    let mut ops: Vec<_> = ops.into_iter().collect();
    ops.sort_by_key(|a| std::cmp::Reverse(a.1));
    ops.iter()
        .map(|(id, n)| format!("{} ({n})", names::operator_name(*id)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn print_round(m: &FullMatch, r: &FullRound, stats: &[PlayerMatchStats]) {
    let name = |id: &str| {
        stats
            .iter()
            .find(|s| s.profile_id == id)
            .map_or(id.to_string(), |s| s.username.clone())
    };
    let mine = m.my_team_index();
    let result = match (r.winner_team, mine) {
        (Some(w), Some(me)) => {
            if w == me {
                "won "
            } else {
                "lost"
            }
        }
        _ => "?   ",
    };
    println!(
        "  R{:<2} {} {} by {:<16} site: {}",
        r.number,
        side_label(r.my_side),
        result,
        win_condition_label(r.win_condition),
        r.site
    );
    for k in &r.kills {
        println!(
            "      {:>3}:{:02}  {} -> {}{}",
            k.time_s / 60,
            k.time_s % 60,
            name(&k.killer),
            name(&k.victim),
            if k.headshot { "  (headshot)" } else { "" }
        );
    }
}
