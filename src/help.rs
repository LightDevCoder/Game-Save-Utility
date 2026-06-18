use crate::models::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpLineKind {
    Heading,
    Paragraph,
    Bullet,
}

#[derive(Clone, Copy, Debug)]
pub struct HelpLine {
    pub kind: HelpLineKind,
    pub zh: &'static str,
    pub en: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct HelpTopic {
    pub id: &'static str,
    pub zh_title: &'static str,
    pub en_title: &'static str,
    pub lines: &'static [HelpLine],
}

impl HelpTopic {
    pub fn title(self, language: Language) -> &'static str {
        match language {
            Language::ZhCn => self.zh_title,
            Language::EnUs => self.en_title,
        }
    }

    pub fn contains(self, language: Language, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }

        let title = self.title(language).to_lowercase();
        if title.contains(&query) {
            return true;
        }

        self.lines
            .iter()
            .map(|line| match language {
                Language::ZhCn => line.zh,
                Language::EnUs => line.en,
            })
            .any(|text| text.to_lowercase().contains(&query))
    }
}

macro_rules! h {
    ($zh:expr, $en:expr) => {
        HelpLine {
            kind: HelpLineKind::Heading,
            zh: $zh,
            en: $en,
        }
    };
}

macro_rules! p {
    ($zh:expr, $en:expr) => {
        HelpLine {
            kind: HelpLineKind::Paragraph,
            zh: $zh,
            en: $en,
        }
    };
}

macro_rules! b {
    ($zh:expr, $en:expr) => {
        HelpLine {
            kind: HelpLineKind::Bullet,
            zh: $zh,
            en: $en,
        }
    };
}

const QUICK_START_LINES: &[HelpLine] = &[
    h!("快速开始", "Quick Start"),
    p!(
        "这个工具用于手动管理单机游戏的本地存档备份。推荐在 Boss 战、剧情分歧、DLC 或二周目前先创建一个备份。",
        "This tool manages local save backups for single-player games. Create a backup before boss fights, story branches, DLC, or new playthroughs."
    ),
    b!("点击左下角“添加游戏”，填写游戏名称并选择存档目录。", "Click Add Game, enter the game name, and choose the save folder."),
    b!("填写可选备份标签，然后点击“立即备份”。", "Enter an optional label, then click Backup Now."),
    b!("在备份历史中勾选节点，可以恢复单个备份或删除多个备份。", "Tick backups in the history table to restore one backup or delete multiple backups."),
];

const STEAM_LINES: &[HelpLine] = &[
    h!("Steam 库扫描", "Steam Library Scan"),
    p!(
        "点击“扫描Steam库”后，工具会读取本机 Steam 库中的已安装游戏，并尝试推断候选存档目录。",
        "Click Scan Steam to read installed Steam games and infer candidate save folders."
    ),
    b!("左侧游戏列表可以拖动右边缘调整宽度。", "Drag the right edge of the game list to resize it."),
    b!("右侧会显示安装目录、候选存档目录以及最终使用的目录。", "The details pane shows install folder, candidate save folders, and the folder that will be used."),
    b!("可以加入当前选中游戏，也可以批量加入全部有候选目录的游戏。", "You can add the selected game or batch-add all games with candidate folders."),
    b!("没有候选目录的游戏可以手动选择存档目录。", "If no candidate is found, choose a save folder manually."),
];

const BACKUP_LINES: &[HelpLine] = &[
    h!("备份与恢复", "Backup and Restore"),
    p!(
        "每个备份节点都会写入 metadata.json，并记录可恢复当前存档快照所需的信息。",
        "Each backup node writes metadata.json and records the information needed to restore that save snapshot."
    ),
    b!(
        "备份失败时会清理未完成的临时目录。",
        "Failed backups clean up incomplete temporary folders."
    ),
    b!(
        "恢复前一定会创建“恢复前自动备份”。",
        "Restore always creates a pre-restore safety backup first."
    ),
    b!(
        "“全选备份”只用于删除选择，不会触发备份，也不会执行批量恢复。",
        "Select All only affects delete selection. It does not start a backup or batch restore."
    ),
    b!(
        "删除一个或多个备份不会影响当前存档目录。",
        "Deleting one or more backups does not touch the current save folder."
    ),
];

const STORAGE_LINES: &[HelpLine] = &[
    h!("备份格式", "Backup Formats"),
    p!(
        "每个游戏可以在“添加/编辑游戏”窗口中选择备份格式。新游戏默认使用增量备份。",
        "Each game can choose its backup format in the Add/Edit Game window. New games use incremental backups by default."
    ),
    b!(
        "增量备份：每个备份节点包含 metadata.json 和 manifest.json，实际文件内容保存在游戏备份目录下的 .objects 共享对象库中。",
        "Incremental backups: each backup node contains metadata.json and manifest.json; actual file contents live in the game's .objects shared object store."
    ),
    b!(
        "首次增量备份会把当前所有文件内容写入对象库，历史列表显示为“全量”；后续备份只新增变化内容，显示为“增量”。每个 manifest 仍然代表一个完整快照。",
        "The first incremental backup stores all current file contents and is shown as Full; later backups only add changed content and are shown as Incremental. Every manifest still represents a complete snapshot."
    ),
    b!(
        "更换备份根目录不会自动创建备份；在新的空备份目录里，下一次手动备份会成为该目录下的首次完整快照。",
        "Changing the backup root does not create a backup. In a fresh empty root, the next manual backup becomes that root's first full snapshot."
    ),
    b!(
        "ZIP 压缩备份：每个备份节点包含 metadata.json 和 save_files.zip，属于压缩后的完整快照。",
        "ZIP backups: each backup node contains metadata.json and save_files.zip, which is a compressed full snapshot."
    ),
    b!(
        "v0.1.0 的 save_files/ 完整复制备份仍可扫描、恢复和删除。",
        "v0.1.0 save_files/ full-copy backups can still be scanned, restored, and deleted."
    ),
];

const SCHEDULE_LINES: &[HelpLine] = &[
    h!("定时自动备份", "Scheduled Auto Backup"),
    p!(
        "定时自动备份在应用运行或最小化到系统托盘时生效，不会创建 Windows 服务或开机启动项。",
        "Scheduled auto backup works while the app is running or minimized to tray. It does not create a Windows service or startup item."
    ),
    b!(
        "在“添加/编辑游戏”窗口中启用定时自动备份，并设置间隔数值和单位。",
        "Enable scheduled auto backup in the Add/Edit Game window and set the interval value plus unit."
    ),
    b!(
        "间隔只接受整数；可以选择“分钟”或“小时”。例如半小时可设置为 30 分钟。",
        "Intervals accept integers only; choose Minutes or Hours. For example, use 30 Minutes for half an hour."
    ),
    b!(
        "只有到达间隔且当前存档相对最近备份发生变化时，才会自动创建备份。",
        "An automatic backup is created only when the interval is due and current saves differ from the latest backup."
    ),
    b!(
        "存档变更提醒只提示存在未备份变更，不会强制创建备份。",
        "Save-change reminders only warn about unbacked changes; they do not force a backup."
    ),
];

const CLEANUP_LINES: &[HelpLine] = &[
    h!("自动清理", "Auto Cleanup"),
    p!(
        "每个游戏可以设置最大备份数量，并单独启用或关闭自动清理。",
        "Each game can have a maximum backup count and its own auto-cleanup setting."
    ),
    b!(
        "超过上限时，保留最新备份并删除更旧的备份。",
        "When the limit is exceeded, newest backups are kept and older backups are deleted."
    ),
    b!(
        "普通备份和恢复前自动备份都会参与清理。",
        "Normal backups and pre-restore backups are both included in cleanup."
    ),
];

const CLOUD_LINES: &[HelpLine] = &[
    h!("Steam Cloud 提示", "Steam Cloud Warnings"),
    p!(
        "通过 Steam 扫描加入游戏时，工具会保存 Steam AppID 和可用的 Steam Cloud 本地缓存候选路径。",
        "When a game is added through Steam scan, the app stores the Steam AppID and available Steam Cloud local cache candidate paths."
    ),
    b!(
        "工具只比较本地存档目录和 Steam Cloud 本地缓存目录的最新修改时间。",
        "The app only compares latest modified times between the local save folder and Steam Cloud local cache folder."
    ),
    b!(
        "检测到时间差异时只显示风险提示，不会阻止备份或恢复。",
        "When a time difference is detected, the app only shows a risk warning; it does not block backup or restore."
    ),
    b!(
        "工具不会访问 Steam 网络 API，也不会读取账号、密码、Cookie、验证码或登录状态。",
        "The app does not call Steam network APIs and does not read account credentials, cookies, verification codes, or login state."
    ),
];

const SHORTCUT_LINES: &[HelpLine] = &[
    h!("快捷键", "Keyboard Shortcuts"),
    p!(
        "顶部工具栏的“快捷键”按钮可以打开快捷键设置，并可启用或关闭快捷键。",
        "The Shortcuts button in the toolbar opens shortcut settings and lets you enable or disable shortcuts."
    ),
    b!("Ctrl+S：为当前选中游戏快速备份。", "Ctrl+S: back up the selected game."),
    b!("Ctrl+R：仅在恰好选中一个备份时打开恢复确认。", "Ctrl+R: open restore confirmation only when exactly one backup is selected."),
    b!("↑ / ↓：在当前活跃列表中移动选择。", "Up / Down: move selection in the active list."),
    b!("← / →：在游戏列表和备份历史之间切换活跃区域。", "Left / Right: switch the active area between games and backup history."),
    b!("光标在文本输入框内时，快捷键不会触发备份或恢复。", "Shortcuts are ignored while a text field is active."),
];

const DATA_LINES: &[HelpLine] = &[
    h!("数据目录", "Data Folder"),
    p!(
        "数据目录保存配置、日志和默认备份目录。可以通过顶部“更改并迁移”选择新位置。",
        "The data folder stores config, logs, and the default backup root. Use Move Data in the toolbar to choose a new location."
    ),
        b!("如果选择普通父目录，工具会在其中创建 GameSaveUtility 文件夹。", "If you choose a normal parent folder, the tool creates GameSaveUtility inside it."),
        b!("首次运行新版时，如果新目录不存在，旧的 GameSaveBackupTool 目录会安全迁移为 GameSaveUtility。", "On the first run of the renamed version, the legacy GameSaveBackupTool folder is safely migrated to GameSaveUtility when the new folder does not exist."),
];

const TRAY_LINES: &[HelpLine] = &[
    h!("关闭与系统托盘", "Close and System Tray"),
    p!(
        "第一次点击窗口关闭按钮时，工具会询问关闭行为。之后可以在顶部工具栏修改。",
        "The first time you click the close button, the app asks what close should do. You can later change this in the toolbar."
    ),
    b!("最小化到系统托盘：关闭按钮隐藏窗口，双击托盘图标恢复。", "Minimize to tray: the close button hides the window; double-click the tray icon to restore it."),
    b!("直接退出软件：关闭按钮直接退出。", "Exit app: the close button exits the app."),
    b!("托盘右键菜单可以显示窗口或正常退出。", "The tray right-click menu can show the window or exit normally."),
];

const LANGUAGE_LINES: &[HelpLine] = &[
    h!("双语界面", "Bilingual Interface"),
    p!(
        "顶部工具栏的 English/中文 按钮可以切换界面语言。语言设置会保存到配置文件。",
        "Use the English/中文 button in the toolbar to switch language. The setting is saved in the config file."
    ),
    b!("帮助窗口会跟随当前界面语言显示对应说明。", "The help window follows the current interface language."),
    b!("游戏名称、路径和备份标签保持用户原始输入，不会翻译。", "Game names, paths, and backup labels keep the user's original input and are not translated."),
];

const TROUBLE_LINES: &[HelpLine] = &[
    h!("常见问题", "Troubleshooting"),
    b!("打开目录失败：目标目录可能不存在、权限不足，或迁移后路径已失效。", "Open folder failed: the folder may not exist, need permission, or be stale after migration."),
    b!("Steam 扫描无候选目录：部分游戏不使用 Steam Cloud 或存档位置比较特殊，请手动选择。", "Steam scan found no candidate: some games do not use Steam Cloud or use uncommon save folders. Choose manually."),
    b!("恢复后游戏未读取存档：确认游戏已完全关闭，并确认选择的是游戏实际读取的存档目录。", "Game did not load restored save: close the game fully and verify the save folder is the one the game actually reads."),
];

static HELP_TOPICS: &[HelpTopic] = &[
    HelpTopic {
        id: "quick",
        zh_title: "快速开始",
        en_title: "Quick Start",
        lines: QUICK_START_LINES,
    },
    HelpTopic {
        id: "steam",
        zh_title: "Steam 库扫描",
        en_title: "Steam Library Scan",
        lines: STEAM_LINES,
    },
    HelpTopic {
        id: "backup",
        zh_title: "备份与恢复",
        en_title: "Backup and Restore",
        lines: BACKUP_LINES,
    },
    HelpTopic {
        id: "storage",
        zh_title: "备份格式",
        en_title: "Backup Formats",
        lines: STORAGE_LINES,
    },
    HelpTopic {
        id: "schedule",
        zh_title: "定时自动备份",
        en_title: "Scheduled Auto Backup",
        lines: SCHEDULE_LINES,
    },
    HelpTopic {
        id: "cleanup",
        zh_title: "自动清理",
        en_title: "Auto Cleanup",
        lines: CLEANUP_LINES,
    },
    HelpTopic {
        id: "cloud",
        zh_title: "Steam Cloud 提示",
        en_title: "Steam Cloud Warnings",
        lines: CLOUD_LINES,
    },
    HelpTopic {
        id: "shortcuts",
        zh_title: "快捷键",
        en_title: "Keyboard Shortcuts",
        lines: SHORTCUT_LINES,
    },
    HelpTopic {
        id: "data",
        zh_title: "数据目录",
        en_title: "Data Folder",
        lines: DATA_LINES,
    },
    HelpTopic {
        id: "tray",
        zh_title: "关闭与系统托盘",
        en_title: "Close and System Tray",
        lines: TRAY_LINES,
    },
    HelpTopic {
        id: "language",
        zh_title: "双语界面",
        en_title: "Bilingual Interface",
        lines: LANGUAGE_LINES,
    },
    HelpTopic {
        id: "trouble",
        zh_title: "常见问题",
        en_title: "Troubleshooting",
        lines: TROUBLE_LINES,
    },
];

pub fn topics() -> &'static [HelpTopic] {
    HELP_TOPICS
}

pub fn default_topic_id() -> &'static str {
    HELP_TOPICS[0].id
}

pub fn topic_by_id(id: &str) -> &'static HelpTopic {
    HELP_TOPICS
        .iter()
        .find(|topic| topic.id == id)
        .unwrap_or(&HELP_TOPICS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_topics_have_both_languages() {
        assert!(topics().len() >= 6);
        for topic in topics() {
            assert!(!topic.zh_title.is_empty());
            assert!(!topic.en_title.is_empty());
            assert!(!topic.lines.is_empty());
        }
    }

    #[test]
    fn help_search_checks_current_language() {
        let topic = topic_by_id("steam");
        assert!(topic.contains(Language::ZhCn, "候选"));
        assert!(topic.contains(Language::EnUs, "candidate"));

        assert!(topic_by_id("storage").contains(Language::ZhCn, "增量"));
        assert!(topic_by_id("storage").contains(Language::EnUs, "ZIP"));
        assert!(topic_by_id("schedule").contains(Language::ZhCn, "定时"));
        assert!(topic_by_id("shortcuts").contains(Language::EnUs, "Ctrl+S"));
    }
}
