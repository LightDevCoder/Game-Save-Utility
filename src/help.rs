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
    b!("在备份历史中选择节点，可以恢复或删除。", "Select a backup in the history table to restore or delete it."),
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
        "备份会递归复制存档目录，并为每个备份节点写入 metadata.json。",
        "Backup recursively copies the save folder and writes metadata.json for each backup node."
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
        "删除单个备份不会影响当前存档目录。",
        "Deleting one backup does not touch the current save folder."
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

const DATA_LINES: &[HelpLine] = &[
    h!("数据目录", "Data Folder"),
    p!(
        "数据目录保存配置、日志和默认备份目录。可以通过顶部“更改并迁移”选择新位置。",
        "The data folder stores config, logs, and the default backup root. Use Move Data in the toolbar to choose a new location."
    ),
    b!("如果选择普通父目录，工具会在其中创建 GameSaveBackupTool 文件夹。", "If you choose a normal parent folder, the tool creates GameSaveBackupTool inside it."),
    b!("迁移成功后，旧的 GameSaveBackupTool 文件夹会整体删除。", "After migration succeeds, the old GameSaveBackupTool folder is removed."),
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
        id: "cleanup",
        zh_title: "自动清理",
        en_title: "Auto Cleanup",
        lines: CLEANUP_LINES,
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
    }
}
