//! Operator and map names from the embedded `data/*.json` tables.
//! Unknown IDs never fail: they get a readable fallback.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

const OPERATORS_JSON: &str = include_str!("../../../data/operators.json");
const MAPS_JSON: &str = include_str!("../../../data/maps.json");

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

pub fn operator_name(id: u64) -> String {
    operators().get(&id).cloned().unwrap_or_else(|| {
        log::warn!("unknown operator id {id}");
        format!("Operator #{id}")
    })
}

pub fn map_name(id: u64) -> String {
    maps().get(&id).cloned().unwrap_or_else(|| {
        log::warn!("unknown map id {id}");
        format!("Map #{id}")
    })
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
        assert_eq!(operator_name(1), "Operator #1");
        assert_eq!(map_name(2), "Map #2");
    }
}
