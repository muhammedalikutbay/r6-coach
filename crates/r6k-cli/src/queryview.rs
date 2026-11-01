//! `r6k query`: runs the v2 query engine over the cached matches and prints the result as JSON.

use anyhow::{bail, Context, Result};
use r6k_core::query::{query, Filter};
use r6k_core::{read_full_match_file, Baseline, FullMatch};
use std::path::Path;

pub fn run(
    range: &str,
    mode: &str,
    side: &str,
    operators: &[String],
    maps: &[String],
    cache: &Path,
) -> Result<()> {
    let filter: Filter = serde_json::from_value(serde_json::json!({
        "range": range, "mode": mode, "side": side, "operators": operators, "maps": maps
    }))
    .context("bad filter value (range: last1|last5|last20|season|all, mode: all|ranked|unranked|quick, side: both|attack|defense)")?;

    let mut matches: Vec<FullMatch> = Vec::new();
    for e in std::fs::read_dir(cache).with_context(|| format!("reading {}", cache.display()))? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "json") {
            match read_full_match_file(&p) {
                Ok(m) => matches.push(m),
                Err(e) => log::warn!("{}: {e}", p.display()),
            }
        }
    }
    if matches.is_empty() {
        bail!("no cached matches in {}", cache.display());
    }
    matches.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let baseline = Baseline::embedded().context("embedded baseline is invalid")?;
    let result = query(&filter, &matches, &baseline);
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
