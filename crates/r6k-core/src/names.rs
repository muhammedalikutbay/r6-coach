//! Operator and map names from the embedded `data/*.json` tables.
//! Unknown IDs never fail: they get a readable fallback.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

const OPERATORS_JSON: &str = include_str!("../../../data/operators.json");
const MAPS_JSON: &str = include_str!("../../../data/maps.json");
/// Bomb site names seen per map in real replays (`tools/gen-weapons/map_sites.py`). They identify a
/// map whose id is not in `maps.json` yet: ids change per season and playlist, site names do not.
const MAP_SITES_JSON: &str = include_str!("../data/map-sites.json");

#[derive(Deserialize)]
struct OperatorTable {
    operators: Vec<Entry>,
}
#[derive(Deserialize)]
struct MapTable {
    maps: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    id: u64,
    #[serde(default)]
    key: String,
    name: String,
}

fn to_map(what: &str, parsed: serde_json::Result<Vec<Entry>>) -> HashMap<u64, String> {
    match parsed {
        Ok(v) => v.into_iter().map(|e| (e.id, e.name)).collect(),
        Err(e) => {
            log::error!("embedded {what} table is invalid: {e}");
            HashMap::new()
        }
    }
}

fn operators() -> &'static HashMap<u64, String> {
    static T: OnceLock<HashMap<u64, String>> = OnceLock::new();
    T.get_or_init(|| {
        to_map(
            "operator",
            serde_json::from_str::<OperatorTable>(OPERATORS_JSON).map(|t| t.operators),
        )
    })
}

fn maps() -> &'static HashMap<u64, String> {
    static T: OnceLock<HashMap<u64, String>> = OnceLock::new();
    T.get_or_init(|| {
        to_map(
            "map",
            serde_json::from_str::<MapTable>(MAPS_JSON).map(|t| t.maps),
        )
    })
}

fn keys(raw: &str, pick_ops: bool) -> HashMap<u64, String> {
    let entries: Vec<Entry> = if pick_ops {
        serde_json::from_str::<OperatorTable>(raw).map(|t| t.operators).unwrap_or_default()
    } else {
        serde_json::from_str::<MapTable>(raw).map(|t| t.maps).unwrap_or_default()
    };
    entries.into_iter().map(|e| (e.id, e.key)).collect()
}

/// Tracker slug (`key` in `data/operators.json`, e.g. "jager") for an operator ID.
pub fn operator_key(id: u64) -> Option<String> {
    static T: OnceLock<HashMap<u64, String>> = OnceLock::new();
    T.get_or_init(|| keys(OPERATORS_JSON, true)).get(&id).cloned()
}

/// Map `key` from `data/maps.json` for a map ID.
pub fn map_key(id: u64) -> Option<String> {
    static T: OnceLock<HashMap<u64, String>> = OnceLock::new();
    T.get_or_init(|| keys(MAPS_JSON, false)).get(&id).cloned()
}

fn names_by_key(raw: &str, pick_ops: bool) -> HashMap<String, String> {
    let entries: Vec<Entry> = if pick_ops {
        serde_json::from_str::<OperatorTable>(raw).map(|t| t.operators).unwrap_or_default()
    } else {
        serde_json::from_str::<MapTable>(raw).map(|t| t.maps).unwrap_or_default()
    };
    entries.into_iter().map(|e| (e.key, e.name)).collect()
}

/// Display name for an operator `key`, if known.
///
/// A key that is a raw numeric id (callers use the id as the key of an unknown operator) gets the
/// readable "Unknown operator" fallback instead of `None`.
pub fn operator_name_by_key(key: &str) -> Option<String> {
    static T: OnceLock<HashMap<String, String>> = OnceLock::new();
    if let Some(n) = T.get_or_init(|| names_by_key(OPERATORS_JSON, true)).get(key) {
        return Some(n.clone());
    }
    key.parse::<u64>().ok().map(unknown_operator_name)
}

/// Display name for a map `key`, if known.
///
/// A numeric key (an unknown map id) gets the readable "Unknown map" fallback instead of `None`.
pub fn map_name_by_key(key: &str) -> Option<String> {
    static T: OnceLock<HashMap<String, String>> = OnceLock::new();
    if let Some(n) = T.get_or_init(|| names_by_key(MAPS_JSON, false)).get(key) {
        return Some(n.clone());
    }
    key.parse::<u64>().ok().map(unknown_map_name)
}

/// Last four digits of an id: enough to tell unknown ids apart without a raw `#123456789012`.
fn id_hint(id: u64) -> String {
    let s = id.to_string();
    if s.len() > 4 {
        format!("\u{2026}{}", &s[s.len() - 4..])
    } else {
        s
    }
}

/// Readable fallback for a map id that is in no table.
pub fn unknown_map_name(id: u64) -> String {
    format!("Unknown map ({})", id_hint(id))
}

/// Readable fallback for an operator id that is in no table.
pub fn unknown_operator_name(id: u64) -> String {
    format!("Unknown operator ({})", id_hint(id))
}

pub fn operator_name(id: u64) -> String {
    operators().get(&id).cloned().unwrap_or_else(|| {
        log::warn!("unknown operator id {id}");
        unknown_operator_name(id)
    })
}

pub fn map_name(id: u64) -> String {
    maps().get(&id).cloned().unwrap_or_else(|| {
        log::warn!("unknown map id {id}");
        unknown_map_name(id)
    })
}

pub fn is_known_operator(id: u64) -> bool {
    operators().contains_key(&id)
}

pub fn is_known_map(id: u64) -> bool {
    maps().contains_key(&id)
}

/// The newest (largest) map id in `maps.json` with this `key`. Ids of one map differ per season
/// and playlist; the newest one is the one to show.
pub fn map_id_for_key(key: &str) -> Option<u64> {
    static T: OnceLock<HashMap<String, u64>> = OnceLock::new();
    T.get_or_init(|| {
        let mut m: HashMap<String, u64> = HashMap::new();
        for (id, k) in keys(MAPS_JSON, false) {
            let e = m.entry(k).or_insert(id);
            *e = (*e).max(id);
        }
        m
    })
    .get(key)
    .copied()
}

#[derive(Deserialize)]
struct SiteTable {
    maps: HashMap<String, Vec<String>>,
}

/// Site name ("2F Master Bedroom, 2F Office") -> map key, and single room name -> map keys.
struct SiteIndex {
    by_site: HashMap<String, String>,
    by_room: HashMap<String, Vec<String>>,
}

fn site_index() -> &'static SiteIndex {
    static T: OnceLock<SiteIndex> = OnceLock::new();
    T.get_or_init(|| {
        let table = serde_json::from_str::<SiteTable>(MAP_SITES_JSON)
            .map(|t| t.maps)
            .unwrap_or_else(|e| {
                log::error!("embedded map-sites table is invalid: {e}");
                HashMap::new()
            });
        let mut by_site: HashMap<String, String> = HashMap::new();
        let mut by_room: HashMap<String, Vec<String>> = HashMap::new();
        let mut clash: Vec<String> = Vec::new();
        for (key, sites) in &table {
            for site in sites {
                if let Some(prev) = by_site.insert(site.clone(), key.clone()) {
                    if prev != *key {
                        clash.push(site.clone());
                    }
                }
                for room in site.split(", ") {
                    let keys = by_room.entry(room.to_string()).or_default();
                    if !keys.contains(key) {
                        keys.push(key.clone());
                    }
                }
            }
        }
        for site in clash {
            by_site.remove(&site); // a site name shared by two maps identifies neither
        }
        SiteIndex { by_site, by_room }
    })
}

/// Map key whose known bomb sites match the given round site names; `None` when nothing matches
/// or when the sites point to two different maps. A site votes for a map when its full name
/// belongs to one map only or, failing that, when each of its room names belongs to one and the
/// same map.
pub fn map_key_from_sites<'a>(sites: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let idx = site_index();
    let mut votes: HashMap<&str, u32> = HashMap::new();
    for site in sites {
        if site.is_empty() {
            continue;
        }
        if let Some(k) = idx.by_site.get(site) {
            *votes.entry(k.as_str()).or_default() += 1;
            continue;
        }
        let mut common: Option<&str> = None;
        let mut ok = true;
        for room in site.split(", ") {
            match idx.by_room.get(room) {
                Some(keys) if keys.len() == 1 && common.map_or(true, |c| c == keys[0]) => {
                    common = Some(keys[0].as_str());
                }
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if let (true, Some(k)) = (ok, common) {
            *votes.entry(k).or_default() += 1;
        }
    }
    if votes.len() == 1 {
        votes.keys().next().map(|k| k.to_string())
    } else {
        None
    }
}

/// An unknown map id resolved through its bomb sites: the id of the known map the sites belong to.
/// `None` for a known id or when the sites do not identify a map.
pub fn resolve_map_by_sites<'a>(id: u64, sites: impl IntoIterator<Item = &'a str>) -> Option<u64> {
    if is_known_map(id) {
        return None;
    }
    let key = map_key_from_sites(sites)?;
    let target = map_id_for_key(&key)?;
    log::info!("map id {id} is not in the table; its bomb sites match '{key}', showing map {target}");
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ids_resolve() {
        assert_eq!(operator_name(378305069945), "Azami");
        assert_eq!(map_name(407558616688), "Chalet");
    }

    #[test]
    fn unknown_ids_fall_back() {
        assert_eq!(operator_name(1), "Unknown operator (1)");
        assert_eq!(map_name(2), "Unknown map (2)");
        assert_eq!(map_name(441842102525), "Unknown map (\u{2026}2525)");
        assert_eq!(
            operator_name_by_key("444310693746000").as_deref(),
            Some("Unknown operator (\u{2026}6000)")
        );
        assert_eq!(map_name_by_key("441842102525").as_deref(), Some("Unknown map (\u{2026}2525)"));
        assert_eq!(map_name_by_key("chalet").as_deref(), Some("Chalet"));
    }

    #[test]
    fn sites_identify_a_map() {
        // Chalet bomb sites from real replays; the id is made up.
        let sites = ["1F Bar, 1F Gaming Room", "2F Master Bedroom, 2F Office"];
        let id = resolve_map_by_sites(1, sites).expect("chalet");
        assert_eq!(map_name(id), "Chalet");
        assert_eq!(resolve_map_by_sites(407558616688, sites), None, "known ids are never remapped");
        assert_eq!(resolve_map_by_sites(1, ["Nowhere, Nothing"]), None);
        // sites of two different maps do not resolve
        assert_eq!(resolve_map_by_sites(1, ["1F Bar, 1F Gaming Room", "2F Gym, 2F Bedroom"]), None);
    }
}
