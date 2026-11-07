//! Which clips to delete: the last N matches, an optional disk cap, starred clips never.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct ClipMeta {
    pub id: i64,
    pub match_id: String,
    /// When the clip was cut (Unix ms); the newest clip of a match gives the match its age.
    pub created_ms: i64,
    pub starred: bool,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// Keep clips of this many newest matches; 0 keeps all.
    pub keep_matches: u32,
    /// Total size limit in bytes (starred clips count, but are never deleted); `None` = no limit.
    pub disk_cap_bytes: Option<u64>,
}

/// IDs of the clips to delete.
pub fn plan_deletions(clips: &[ClipMeta], policy: Policy) -> Vec<i64> {
    let mut doomed: Vec<i64> = Vec::new();

    // 1. Matches beyond the newest N lose their unstarred clips.
    if policy.keep_matches > 0 {
        let mut newest: HashMap<&str, i64> = HashMap::new();
        for c in clips {
            let e = newest.entry(c.match_id.as_str()).or_insert(i64::MIN);
            *e = (*e).max(c.created_ms);
        }
        let mut order: Vec<(&str, i64)> = newest.into_iter().collect();
        // Newest first; the ID breaks ties so the result is deterministic.
        order.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let keep: Vec<&str> = order.iter().take(policy.keep_matches as usize).map(|(m, _)| *m).collect();
        doomed.extend(clips.iter().filter(|c| !c.starred && !keep.contains(&c.match_id.as_str())).map(|c| c.id));
    }

    // 2. Over the disk cap: drop the oldest unstarred clips that are left.
    if let Some(cap) = policy.disk_cap_bytes {
        let alive = |c: &&ClipMeta| !doomed.contains(&c.id);
        let mut total: u64 = clips.iter().filter(alive).map(|c| c.size).sum();
        if total > cap {
            let mut candidates: Vec<&ClipMeta> = clips.iter().filter(alive).filter(|c| !c.starred).collect();
            candidates.sort_by_key(|c| (c.created_ms, c.id));
            let mut extra = Vec::new();
            for c in candidates {
                if total <= cap {
                    break;
                }
                total = total.saturating_sub(c.size);
                extra.push(c.id);
            }
            doomed.extend(extra);
        }
    }
    doomed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: i64, m: &str, created: i64, starred: bool, size: u64) -> ClipMeta {
        ClipMeta { id, match_id: m.into(), created_ms: created, starred, size }
    }

    #[test]
    fn keeps_the_newest_n_matches() {
        let clips = vec![
            clip(1, "old", 100, false, 10),
            clip(2, "mid", 200, false, 10),
            clip(3, "mid", 210, false, 10),
            clip(4, "new", 300, false, 10),
        ];
        let mut d = plan_deletions(&clips, Policy { keep_matches: 2, disk_cap_bytes: None });
        d.sort_unstable();
        assert_eq!(d, vec![1]);
        assert!(plan_deletions(&clips, Policy { keep_matches: 0, disk_cap_bytes: None }).is_empty());
    }

    #[test]
    fn starred_clips_survive_both_rules() {
        let clips = vec![clip(1, "old", 100, true, 100), clip(2, "old", 101, false, 100), clip(3, "new", 300, false, 100)];
        let d = plan_deletions(&clips, Policy { keep_matches: 1, disk_cap_bytes: Some(10) });
        assert!(!d.contains(&1));
        assert!(d.contains(&2) && d.contains(&3));
    }

    #[test]
    fn disk_cap_drops_the_oldest_first_and_stops_when_under() {
        let clips = vec![clip(1, "a", 100, false, 40), clip(2, "a", 110, false, 40), clip(3, "b", 200, false, 40)];
        let d = plan_deletions(&clips, Policy { keep_matches: 0, disk_cap_bytes: Some(90) });
        assert_eq!(d, vec![1]);
        let d = plan_deletions(&clips, Policy { keep_matches: 0, disk_cap_bytes: Some(1000) });
        assert!(d.is_empty());
    }

    #[test]
    fn match_rule_runs_before_the_cap() {
        // After dropping the old match the rest already fits: the cap deletes nothing more.
        let clips = vec![clip(1, "old", 100, false, 80), clip(2, "new", 300, false, 80)];
        let d = plan_deletions(&clips, Policy { keep_matches: 1, disk_cap_bytes: Some(100) });
        assert_eq!(d, vec![1]);
    }
}
