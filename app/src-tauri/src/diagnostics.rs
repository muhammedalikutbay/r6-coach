//! "Export unknown ids" (Settings): the map, operator and weapon ids in the stored matches that the
//! embedded tables do not know, with counts, as a JSON file the user can send to the developers.
//!
//! The report is built from the stored raw parser JSON, which keeps the ids exactly as the replay had
//! them. It holds no player names, profile ids, match ids or timestamps: only ids, counts, bomb site
//! names (they identify a map), match types and game versions.

use r6k_core::{names, weapons};
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;

type Cmd<T> = Result<T, String>;

#[derive(Default)]
struct MapInfo {
    rounds: u32,
    matches: BTreeSet<usize>,
    sites: BTreeSet<String>,
    match_types: BTreeMap<String, u32>,
    versions: BTreeSet<String>,
    parser_names: BTreeSet<String>,
}

#[derive(Default)]
struct OperatorInfo {
    rounds: u32,
    matches: BTreeSet<usize>,
    kills: u32,
    match_types: BTreeMap<String, u32>,
    versions: BTreeSet<String>,
}

#[derive(Default)]
struct WeaponInfo {
    kills: u32,
    versions: BTreeSet<String>,
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// Builds the report from raw parser JSON documents (`{"rounds":[...]}`).
pub fn unknown_ids_report<'a>(raws: impl IntoIterator<Item = &'a Value>, exported_at: &str) -> Value {
    let mut maps: BTreeMap<u64, MapInfo> = BTreeMap::new();
    let mut operators: BTreeMap<u64, OperatorInfo> = BTreeMap::new();
    // (kind, id, slot, operator id) -> info; kind is "item" (loadout item id) or "asset" (kill packet id)
    let mut weapon_rows: BTreeMap<(&'static str, u64, String, u64), WeaponInfo> = BTreeMap::new();
    let (mut matches, mut kills, mut named, mut generic, mut no_weapon) = (0u32, 0u32, 0u32, 0u32, 0u32);

    for (mi, raw) in raws.into_iter().enumerate() {
        matches += 1;
        for round in raw["rounds"].as_array().into_iter().flatten() {
            let version = s(&round["gameVersion"]).to_string();
            let match_type = s(&round["matchType"]["name"]).to_string();
            let map_id = round["map"]["id"].as_u64().unwrap_or(0);
            if map_id != 0 && !names::is_known_map(map_id) {
                let m = maps.entry(map_id).or_default();
                m.rounds += 1;
                m.matches.insert(mi);
                if !s(&round["site"]).is_empty() {
                    m.sites.insert(s(&round["site"]).to_string());
                }
                *m.match_types.entry(match_type.clone()).or_default() += 1;
                if !version.is_empty() {
                    m.versions.insert(version.clone());
                }
                if !s(&round["map"]["name"]).is_empty() {
                    m.parser_names.insert(s(&round["map"]["name"]).to_string());
                }
            }

            let mut operator_of: HashMap<&str, u64> = HashMap::new();
            for p in round["players"].as_array().into_iter().flatten() {
                let op = p["operator"]["id"].as_u64().unwrap_or(0);
                operator_of.insert(s(&p["username"]), op);
                if op != 0 && !names::is_known_operator(op) {
                    let o = operators.entry(op).or_default();
                    o.rounds += 1;
                    o.matches.insert(mi);
                    *o.match_types.entry(match_type.clone()).or_default() += 1;
                    if !version.is_empty() {
                        o.versions.insert(version.clone());
                    }
                }
            }

            for e in round["matchFeedback"].as_array().into_iter().flatten() {
                if s(&e["type"]["name"]) != "Kill" {
                    continue;
                }
                kills += 1;
                let op = operator_of.get(s(&e["username"])).copied().unwrap_or(0);
                if let Some(o) = operators.get_mut(&op) {
                    o.kills += 1;
                }
                let asset = e["weaponId"].as_u64().filter(|&a| a != 0);
                let item = e["weaponItemId"].as_u64().filter(|&i| i != 0);
                let slot = s(&e["weaponSlot"]).to_string();
                if item.and_then(weapons::weapon_name).is_some() {
                    named += 1;
                    continue;
                }
                let key = match (item, asset) {
                    (Some(i), _) => ("item", i, slot, op),
                    (None, Some(a)) if slot.is_empty() && weapons::generic_kill_source(a).is_some() => {
                        generic += 1;
                        continue;
                    }
                    (None, Some(a)) => ("asset", a, slot, op),
                    (None, None) => {
                        no_weapon += 1;
                        continue;
                    }
                };
                let w = weapon_rows.entry(key).or_default();
                w.kills += 1;
                if !version.is_empty() {
                    w.versions.insert(version.clone());
                }
            }
        }
    }

    let maps: Vec<Value> = maps
        .into_iter()
        .map(|(id, m)| {
            let sites: Vec<&str> = m.sites.iter().map(String::as_str).collect();
            let resolved = names::resolve_map_by_sites(id, sites.iter().copied()).map(names::map_name);
            json!({
                "id": id,
                "rounds": m.rounds,
                "matches": m.matches.len(),
                "matchTypes": m.match_types,
                "gameVersions": m.versions,
                "sites": m.sites,
                "parserName": m.parser_names.iter().next(),
                "shownAs": resolved.unwrap_or_else(|| names::unknown_map_name(id)),
                "resolvedBySites": names::resolve_map_by_sites(id, sites.iter().copied()).is_some(),
            })
        })
        .collect();
    let operators: Vec<Value> = operators
        .into_iter()
        .map(|(id, o)| {
            json!({
                "id": id,
                "rounds": o.rounds,
                "matches": o.matches.len(),
                "kills": o.kills,
                "matchTypes": o.match_types,
                "gameVersions": o.versions,
            })
        })
        .collect();
    let weapon_rows: Vec<Value> = weapon_rows
        .into_iter()
        .map(|((kind, id, slot, op), w)| {
            let mut row = Map::new();
            row.insert(if kind == "item" { "itemId" } else { "assetId" }.into(), json!(id));
            row.insert("slot".into(), json!(if slot.is_empty() { Value::Null } else { json!(slot) }));
            row.insert("operatorId".into(), json!(op));
            row.insert("operator".into(), json!(if op == 0 { "unknown".to_string() } else { names::operator_name(op) }));
            row.insert("kills".into(), json!(w.kills));
            row.insert("gameVersions".into(), json!(w.versions));
            Value::Object(row)
        })
        .collect();

    json!({
        "note": "Unknown Siege ids found in the stored matches of R6 Coach. No player names or match ids.",
        "exportedAt": exported_at,
        "matches": matches,
        "kills": { "total": kills, "named": named, "meleeOrExplosion": generic, "withoutWeapon": no_weapon },
        "unknownMaps": maps,
        "unknownOperators": operators,
        "unknownWeapons": weapon_rows,
    })
}

fn decompress(b: &[u8]) -> Result<String, String> {
    let mut out = String::new();
    flate2::read::ZlibDecoder::new(b)
        .read_to_string(&mut out)
        .map_err(|e| format!("cannot decompress raw JSON: {e}"))?;
    Ok(out)
}

/// Report text for the stored matches of a database.
pub fn report_text(conn: &Connection, exported_at: &str) -> Cmd<String> {
    let blobs: Vec<Vec<u8>> = conn
        .prepare("SELECT raw_json FROM matches WHERE status != 'in_progress' ORDER BY timestamp, match_id")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())?;
    let docs: Vec<Value> = blobs
        .iter()
        .filter_map(|b| match decompress(b).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("unknown-ids report: a stored match was skipped: {e}");
                None
            }
        })
        .collect();
    serde_json::to_string_pretty(&unknown_ids_report(&docs, exported_at)).map_err(|e| e.to_string())
}

/// Settings > Export unknown ids: writes the report to a file the user picks.
/// Returns the saved path, or `None` when the dialog was cancelled.
#[tauri::command]
pub async fn export_unknown_ids(
    app: tauri::AppHandle,
    db: tauri::State<'_, crate::store::Db>,
) -> Cmd<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let text = {
        let conn = db.0.lock().map_err(|_| "database lock poisoned".to_string())?;
        report_text(&conn, &now_iso())?
    };
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file().set_file_name("r6koc-unknown-ids.json").add_filter("JSON", &["json"]);
        let Some(picked) = dialog.blocking_save_file() else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(|e| format!("cannot use that location: {e}"))?;
        std::fs::write(&path, text.as_bytes()).map_err(|e| format!("cannot write the file: {e}"))?;
        log::info!("unknown ids exported");
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| format!("export task failed: {e}"))?
}

fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> Value {
        json!({"rounds": [{
            "gameVersion": "Y11S4_Alpha01",
            "matchType": {"name": "Unranked", "id": 1},
            "map": {"name": "Map(999999999999)", "id": 999999999999u64},
            "site": "2F Master Bedroom, 2F Office",
            "players": [
                {"username": "Secret", "operator": {"id": 378305069945u64}},
                {"username": "Other", "operator": {"id": 777777777777u64}}],
            "matchFeedback": [
                {"type": {"name": "Kill"}, "username": "Secret", "target": "Other", "weaponId": 5, "weaponSlot": "primary", "weaponItemId": 238373640062u64},
                {"type": {"name": "Kill"}, "username": "Other", "target": "Secret", "weaponId": 6, "weaponSlot": "primary", "weaponItemId": 123456789012u64},
                {"type": {"name": "Kill"}, "username": "Other", "target": "Secret", "weaponId": 70077334926u64},
                {"type": {"name": "Kill"}, "username": "Secret", "target": "Other", "weaponId": 42},
                {"type": {"name": "Kill"}, "username": "Secret", "target": "Other"}]
        }]})
    }

    /// `R6K_SAMPLE_DB=<copy of a real database> cargo test -p r6koc-app sample_db -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a copy of a real database"]
    fn sample_db() {
        let path = std::env::var("R6K_SAMPLE_DB").expect("R6K_SAMPLE_DB");
        let tmp = tempfile::tempdir().unwrap();
        let copy = tmp.path().join("r6koc.db");
        std::fs::copy(path, &copy).unwrap();
        let conn = crate::store::open(&copy).unwrap(); // rebuilds the full JSON from the stored raw JSON
        println!("{}", report_text(&conn, "now").unwrap());
        let (mut total, mut named, mut labelled) = (0, 0, 0);
        for m in crate::store::load_full_matches(&conn).unwrap() {
            for r in &m.rounds {
                for k in &r.kills {
                    total += 1;
                    labelled += k.weapon.is_some() as u32;
                    named += k.weapon.as_deref().is_some_and(|w| !w.starts_with("Unknown")) as u32;
                }
            }
        }
        println!("kills {total}, named or labelled {named}, any label {labelled}");
    }

    #[test]
    fn report_lists_unknown_ids_without_names() {
        let r = unknown_ids_report([&raw()], "now");
        assert_eq!(r["kills"], json!({"total": 5, "named": 1, "meleeOrExplosion": 1, "withoutWeapon": 1}));
        let m = &r["unknownMaps"][0];
        assert_eq!(m["id"], 999999999999u64);
        assert_eq!(m["sites"][0], "2F Master Bedroom, 2F Office");
        assert_eq!(m["matchTypes"]["Unranked"], 1);
        assert_eq!(m["gameVersions"][0], "Y11S4_Alpha01");
        assert_eq!(m["resolvedBySites"], true, "Chalet site names identify the map");
        assert_eq!(m["shownAs"], "Chalet");
        assert_eq!(r["unknownOperators"][0]["id"], 777777777777u64);
        assert_eq!(r["unknownOperators"][0]["kills"], 2);
        let w = r["unknownWeapons"].as_array().unwrap();
        assert_eq!(w.len(), 2);
        assert!(w.iter().any(|x| x["itemId"] == 123456789012u64 && x["slot"] == "primary"));
        assert!(w.iter().any(|x| x["assetId"] == 42 && x["slot"].is_null()));
        let text = r.to_string();
        assert!(!text.contains("Secret") && !text.contains("Other"), "no player names");
    }
}
