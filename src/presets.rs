use crate::fs_utils::expand_env_vars;
use crate::models::PresetGame;
use std::path::PathBuf;

pub fn built_in_presets() -> Vec<PresetGame> {
    vec![
        PresetGame {
            name: "赛博朋克2077".to_owned(),
            path_candidates: vec![
                "%USERPROFILE%/Saved Games/CD Projekt Red/Cyberpunk 2077".to_owned()
            ],
            note: None,
        },
        PresetGame {
            name: "艾尔登法环".to_owned(),
            path_candidates: vec!["%APPDATA%/EldenRing".to_owned()],
            note: None,
        },
        PresetGame {
            name: "黑神话悟空".to_owned(),
            path_candidates: vec![
                "%PROGRAMFILES(X86)%/Steam/steamapps/common/BlackMythWukong/b1/Saved/SaveGames"
                    .to_owned(),
            ],
            note: Some("该路径与安装位置和平台有关，只作为候选路径。".to_owned()),
        },
        PresetGame {
            name: "巫师3".to_owned(),
            path_candidates: vec!["%USERPROFILE%/Documents/The Witcher 3/gamesaves".to_owned()],
            note: None,
        },
        PresetGame {
            name: "星露谷物语".to_owned(),
            path_candidates: vec!["%APPDATA%/StardewValley/Saves".to_owned()],
            note: None,
        },
        PresetGame {
            name: "只狼".to_owned(),
            path_candidates: vec!["%APPDATA%/Sekiro".to_owned()],
            note: None,
        },
        PresetGame {
            name: "博德之门3".to_owned(),
            path_candidates: vec![
                "%LOCALAPPDATA%/Larian Studios/Baldur's Gate 3/PlayerProfiles".to_owned(),
            ],
            note: None,
        },
        PresetGame {
            name: "幻兽帕鲁".to_owned(),
            path_candidates: vec!["%LOCALAPPDATA%/Pal/Saved/SaveGames".to_owned()],
            note: None,
        },
    ]
}

pub fn first_expanded_path(preset: &PresetGame) -> Option<PathBuf> {
    preset
        .path_candidates
        .first()
        .map(|path| PathBuf::from(expand_env_vars(path)))
}
