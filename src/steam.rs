use crate::fs_utils::expand_env_vars;
use crate::models::{AppError, AppResult};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const STEAM_CLOUD_LOCAL_CACHE_SOURCE: &str = "Steam Cloud 本地缓存";

#[derive(Clone, Debug)]
pub struct SteamSavePathCandidate {
    pub path: PathBuf,
    pub source: String,
    pub exists: bool,
}

impl SteamSavePathCandidate {
    pub fn is_steam_cloud_cache(&self) -> bool {
        self.source == STEAM_CLOUD_LOCAL_CACHE_SOURCE
    }
}

#[derive(Clone, Debug)]
pub struct SteamGameCandidate {
    pub app_id: String,
    pub name: String,
    pub install_dir: PathBuf,
    pub save_paths: Vec<SteamSavePathCandidate>,
}

impl SteamGameCandidate {
    pub fn recommended_save_path(&self) -> Option<PathBuf> {
        self.save_paths
            .iter()
            .find(|candidate| candidate.exists)
            .or_else(|| self.save_paths.first())
            .map(|candidate| candidate.path.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum VdfToken {
    Text(String),
    Open,
    Close,
}

pub fn scan_installed_games() -> AppResult<Vec<SteamGameCandidate>> {
    let steam_roots = find_steam_roots();
    if steam_roots.is_empty() {
        return Err(AppError::message(
            "未找到 Steam 安装目录。请确认已安装 Steam，或手动添加游戏。",
        ));
    }

    let mut games = scan_with_steam_roots(&steam_roots)?;
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(games)
}

fn scan_with_steam_roots(steam_roots: &[PathBuf]) -> AppResult<Vec<SteamGameCandidate>> {
    let libraries = discover_library_roots(steam_roots);
    if libraries.is_empty() {
        return Err(AppError::message("未找到 Steam 库目录。"));
    }

    let mut seen_app_ids = HashSet::new();
    let mut games = Vec::new();

    for library in libraries {
        let steamapps = library.join("steamapps");
        let Ok(entries) = fs::read_dir(&steamapps) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !is_app_manifest(&path) {
                continue;
            }

            let raw = fs::read_to_string(&path)
                .map_err(|err| AppError::io("Steam 清单读取失败", &path, err))?;
            let Some((app_id, name, install_dir_name)) = parse_app_manifest(&raw) else {
                continue;
            };
            if !seen_app_ids.insert(app_id.clone()) {
                continue;
            }

            let install_dir = steamapps.join("common").join(install_dir_name);
            let save_paths = save_path_candidates(steam_roots, &app_id, &name, &install_dir);
            games.push(SteamGameCandidate {
                app_id,
                name,
                install_dir,
                save_paths,
            });
        }
    }

    Ok(games)
}

fn find_steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(path) = steam_path_from_registry() {
        push_existing_steam_root(&mut roots, path);
    }

    for key in ["PROGRAMFILES(X86)", "PROGRAMFILES"] {
        if let Some(base) = env::var_os(key) {
            push_existing_steam_root(&mut roots, PathBuf::from(base).join("Steam"));
        }
    }
    push_existing_steam_root(&mut roots, PathBuf::from(r"C:\Program Files (x86)\Steam"));
    push_existing_steam_root(&mut roots, PathBuf::from(r"C:\Program Files\Steam"));

    roots
}

#[cfg(windows)]
fn steam_path_from_registry() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam = hkcu.open_subkey("Software\\Valve\\Steam").ok()?;
    steam
        .get_value::<String, _>("SteamPath")
        .ok()
        .map(PathBuf::from)
}

#[cfg(not(windows))]
fn steam_path_from_registry() -> Option<PathBuf> {
    None
}

fn push_existing_steam_root(roots: &mut Vec<PathBuf>, path: PathBuf) {
    if !(path.join("steamapps").is_dir() || path.join("userdata").is_dir()) {
        return;
    }
    push_unique_path(roots, path);
}

fn discover_library_roots(steam_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut libraries = Vec::new();

    for root in steam_roots {
        push_unique_path(&mut libraries, root.clone());

        let library_file = root.join("steamapps").join("libraryfolders.vdf");
        let Ok(raw) = fs::read_to_string(&library_file) else {
            continue;
        };

        for path in parse_library_paths(&raw) {
            if path.join("steamapps").is_dir() {
                push_unique_path(&mut libraries, path);
            }
        }
    }

    libraries
}

fn save_path_candidates(
    steam_roots: &[PathBuf],
    app_id: &str,
    name: &str,
    install_dir: &Path,
) -> Vec<SteamSavePathCandidate> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for (path, source) in known_save_paths(app_id, name, install_dir) {
        push_save_candidate(&mut candidates, &mut seen, path, source);
    }

    for root in steam_roots {
        let userdata = root.join("userdata");
        let Ok(users) = fs::read_dir(&userdata) else {
            continue;
        };

        for user in users.flatten() {
            let remote = user.path().join(app_id).join("remote");
            if remote.is_dir() {
                push_save_candidate(
                    &mut candidates,
                    &mut seen,
                    remote,
                    STEAM_CLOUD_LOCAL_CACHE_SOURCE.to_owned(),
                );
            }
        }
    }

    candidates
}

fn known_save_paths(app_id: &str, name: &str, install_dir: &Path) -> Vec<(PathBuf, String)> {
    let mut paths = Vec::new();
    let lower_name = name.to_lowercase();

    if app_id == "1091500" || lower_name.contains("cyberpunk 2077") {
        paths.push(expanded(
            "%USERPROFILE%/Saved Games/CD Projekt Red/Cyberpunk 2077",
        ));
    }
    if app_id == "1245620" || lower_name.contains("elden ring") {
        paths.push(expanded("%APPDATA%/EldenRing"));
    }
    if app_id == "2358720" || lower_name.contains("black myth") {
        paths.push(install_dir.join("b1").join("Saved").join("SaveGames"));
    }
    if app_id == "292030" || lower_name.contains("witcher 3") {
        paths.push(expanded("%USERPROFILE%/Documents/The Witcher 3/gamesaves"));
    }
    if app_id == "413150" || lower_name.contains("stardew valley") {
        paths.push(expanded("%APPDATA%/StardewValley/Saves"));
    }
    if app_id == "814380" || lower_name.contains("sekiro") {
        paths.push(expanded("%APPDATA%/Sekiro"));
    }
    if app_id == "1086940" || lower_name.contains("baldur's gate 3") {
        paths.push(expanded(
            "%LOCALAPPDATA%/Larian Studios/Baldur's Gate 3/PlayerProfiles",
        ));
    }
    if app_id == "1623730" || lower_name.contains("palworld") {
        paths.push(expanded("%LOCALAPPDATA%/Pal/Saved/SaveGames"));
    }

    paths
        .into_iter()
        .map(|path| (path, "内置常见路径".to_owned()))
        .collect()
}

fn expanded(path: &str) -> PathBuf {
    PathBuf::from(expand_env_vars(path))
}

fn push_save_candidate(
    candidates: &mut Vec<SteamSavePathCandidate>,
    seen: &mut HashSet<String>,
    path: PathBuf,
    source: String,
) {
    let key = path_key(&path);
    if !seen.insert(key) {
        return;
    }

    let exists = path.is_dir();
    candidates.push(SteamSavePathCandidate {
        path,
        source,
        exists,
    });
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    let key = path_key(&path);
    if !paths.iter().any(|existing| path_key(existing) == key) {
        paths.push(path);
    }
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

fn is_app_manifest(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with("appmanifest_") && name.ends_with(".acf")
}

fn parse_app_manifest(raw: &str) -> Option<(String, String, String)> {
    let tokens = tokenize_vdf(raw);
    let app_id = value_after_key(&tokens, "appid")?;
    let name = value_after_key(&tokens, "name")?;
    let install_dir = value_after_key(&tokens, "installdir")?;
    Some((app_id, name, install_dir))
}

fn parse_library_paths(raw: &str) -> Vec<PathBuf> {
    let tokens = tokenize_vdf(raw);
    let mut paths = Vec::new();

    for pair in tokens.windows(2) {
        match (&pair[0], &pair[1]) {
            (VdfToken::Text(key), VdfToken::Text(value)) if key == "path" => {
                paths.push(PathBuf::from(value));
            }
            (VdfToken::Text(key), VdfToken::Text(value))
                if key.chars().all(|ch| ch.is_ascii_digit())
                    && (value.contains('\\') || value.contains('/')) =>
            {
                paths.push(PathBuf::from(value));
            }
            _ => {}
        }
    }

    paths
}

fn value_after_key(tokens: &[VdfToken], key: &str) -> Option<String> {
    tokens
        .windows(2)
        .find_map(|pair| match (&pair[0], &pair[1]) {
            (VdfToken::Text(candidate), VdfToken::Text(value)) if candidate == key => {
                Some(value.clone())
            }
            _ => None,
        })
}

fn tokenize_vdf(raw: &str) -> Vec<VdfToken> {
    let mut tokens = Vec::new();
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                let mut text = String::new();
                while let Some(next) = chars.next() {
                    match next {
                        '"' => break,
                        '\\' => {
                            if let Some(escaped) = chars.next() {
                                text.push(escaped);
                            }
                        }
                        _ => text.push(next),
                    }
                }
                tokens.push(VdfToken::Text(text));
            }
            '{' => tokens.push(VdfToken::Open),
            '}' => tokens.push(VdfToken::Close),
            '/' if chars.peek() == Some(&'/') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn scans_installed_steam_game_and_cloud_save_candidate() {
        let root = tempdir().unwrap();
        let steam = root.path().join("Steam");
        let library = root.path().join("SteamLibrary");
        fs::create_dir_all(steam.join("steamapps")).unwrap();
        fs::create_dir_all(
            library
                .join("steamapps")
                .join("common")
                .join("Cyberpunk 2077"),
        )
        .unwrap();
        fs::create_dir_all(
            steam
                .join("userdata")
                .join("123")
                .join("1091500")
                .join("remote"),
        )
        .unwrap();

        fs::write(
            steam.join("steamapps").join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{ \"1\" {{ \"path\" \"{}\" }} }}",
                escape_vdf_path(&library)
            ),
        )
        .unwrap();
        fs::write(
            library.join("steamapps").join("appmanifest_1091500.acf"),
            "\"AppState\" { \"appid\" \"1091500\" \"name\" \"Cyberpunk 2077\" \"installdir\" \"Cyberpunk 2077\" }",
        )
        .unwrap();

        let games = scan_with_steam_roots(&[steam]).unwrap();

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].app_id, "1091500");
        assert_eq!(games[0].name, "Cyberpunk 2077");
        assert!(games[0].save_paths.iter().any(|candidate| candidate.source
            == STEAM_CLOUD_LOCAL_CACHE_SOURCE
            && candidate.exists));
    }

    fn escape_vdf_path(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "\\\\")
    }
}
