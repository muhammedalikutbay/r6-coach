//! Locates the Rainbow Six Siege `MatchReplay` folder on this machine.

use std::fs;
use std::path::{Path, PathBuf};

const GAME_DIR: &str = "Tom Clancy's Rainbow Six Siege";
const UBISOFT_DEFAULT: &str = r"C:\Program Files (x86)\Ubisoft\Ubisoft Game Launcher\games\Tom Clancy's Rainbow Six Siege\MatchReplay";

/// State of a candidate replay folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayDirStatus {
    Missing,
    Empty,
    Ready { match_count: usize },
}

/// Counts `Match-*` sub-folders of `dir`.
pub fn inspect(dir: &Path) -> ReplayDirStatus {
    let Ok(rd) = fs::read_dir(dir) else {
        return ReplayDirStatus::Missing;
    };
    let match_count = rd
        .flatten()
        .filter(|e| {
            e.path().is_dir() && e.file_name().to_string_lossy().starts_with("Match-")
        })
        .count();
    if match_count == 0 {
        ReplayDirStatus::Empty
    } else {
        ReplayDirStatus::Ready { match_count }
    }
}

/// Extracts library paths (`"path"` values) from a Steam `libraryfolders.vdf`.
pub fn parse_steam_library_folders(vdf: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in vdf.lines() {
        let toks = quoted_tokens(line);
        if toks.len() == 2 && toks[0].eq_ignore_ascii_case("path") {
            out.push(PathBuf::from(&toks[1]));
        }
    }
    out
}

/// Splits a line into its quoted tokens, resolving `\\` and `\"` escapes.
fn quoted_tokens(line: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut s = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some(o) => s.push(o),
                    None => break,
                },
                o => s.push(o),
            }
        }
        toks.push(s);
    }
    toks
}

/// All places the replay folder might be, in priority order (not checked for existence).
pub fn candidate_dirs() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![PathBuf::from(UBISOFT_DEFAULT)];
    #[cfg(windows)]
    {
        out.extend(registry::ubisoft_dirs());
        out.extend(registry::steam_dirs());
    }
    let mut seen = Vec::new();
    out.retain(|p| {
        let key = p.to_string_lossy().replace('/', "\\").to_lowercase();
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    log::debug!("replay dir candidates: {out:?}");
    out
}

/// First candidate that is `Ready`.
pub fn find_replay_dir() -> Option<PathBuf> {
    candidate_dirs()
        .into_iter()
        .find(|d| matches!(inspect(d), ReplayDirStatus::Ready { .. }))
}

#[cfg(windows)]
mod registry {
    use super::*;
    use winreg::enums::*;
    use winreg::RegKey;

    fn replay_in(install: &str) -> PathBuf {
        // Launcher installs point at the launcher root; the game sits under games\.
        let base = PathBuf::from(install);
        base.join("games").join(GAME_DIR).join("MatchReplay")
    }

    pub fn ubisoft_dirs() -> Vec<PathBuf> {
        let mut out = Vec::new();
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let Ok(launcher) = hklm.open_subkey(r"SOFTWARE\WOW6432Node\Ubisoft\Launcher") else {
            return out;
        };
        if let Ok(p) = launcher.get_value::<String, _>("InstallDir") {
            out.push(replay_in(&p));
        }
        if let Ok(installs) = launcher.open_subkey("Installs") {
            for name in installs.enum_keys().flatten() {
                if let Ok(k) = installs.open_subkey(&name) {
                    if let Ok(dir) = k.get_value::<String, _>("InstallDir") {
                        out.push(PathBuf::from(dir).join("MatchReplay"));
                    }
                }
            }
        }
        out
    }

    pub fn steam_dirs() -> Vec<PathBuf> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let Ok(steam) = hkcu.open_subkey(r"Software\Valve\Steam") else {
            return Vec::new();
        };
        let Ok(root) = steam.get_value::<String, _>("SteamPath") else {
            return Vec::new();
        };
        let root = PathBuf::from(root);
        let mut libs = vec![root.clone()];
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        match fs::read_to_string(&vdf) {
            Ok(text) => libs.extend(parse_steam_library_folders(&text)),
            Err(e) => log::debug!("cannot read {}: {e}", vdf.display()),
        }
        libs.into_iter()
            .map(|l| l.join("steamapps").join("common").join(GAME_DIR).join("MatchReplay"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VDF: &str = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"359550"		"1234"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}
"#;

    #[test]
    fn parses_vdf() {
        assert_eq!(
            parse_steam_library_folders(VDF),
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\SteamLibrary")
            ]
        );
        assert!(parse_steam_library_folders("garbage").is_empty());
    }

    #[test]
    fn inspect_states() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(inspect(&t.path().join("nope")), ReplayDirStatus::Missing);
        assert_eq!(inspect(t.path()), ReplayDirStatus::Empty);
        fs::create_dir(t.path().join("Other")).unwrap();
        fs::write(t.path().join("Match-file"), b"x").unwrap();
        assert_eq!(inspect(t.path()), ReplayDirStatus::Empty);
        fs::create_dir(t.path().join("Match-2026-10-24_12-07-17-1")).unwrap();
        fs::create_dir(t.path().join("Match-2026-10-24_12-37-24-1")).unwrap();
        assert_eq!(inspect(t.path()), ReplayDirStatus::Ready { match_count: 2 });
    }

    #[test]
    fn candidates_start_with_default() {
        assert_eq!(candidate_dirs()[0], PathBuf::from(UBISOFT_DEFAULT));
    }
}
