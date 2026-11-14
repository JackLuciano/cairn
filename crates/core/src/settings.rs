use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ops::{ForceMode, PullMode};
use crate::paths::{data_dir, write_atomic};
use crate::Result;

/// Light, dark or follow Windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

/// The accent color used for highlights and primary buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    #[default]
    Blue,
    Purple,
    Teal,
    Orange,
}

/// How tightly the UI is packed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    #[default]
    Compact,
    Comfortable,
}

/// The UI language.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    System,
    English,
    Hungarian,
    EnglishUk,
}

impl Language {
    // Alphabetical by English language name, with the English variants together.
    pub const ALL: [Self; 3] = [
        Self::EnglishUk,
        Self::English,
        Self::Hungarian,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::English => "English (US)",
            Self::EnglishUk => "English (UK)",
            Self::Hungarian => "Hungarian · Magyar",
        }
    }

    /// The locale code used by the translation files.
    pub fn code(self) -> &'static str {
        match self {
            Self::System => "en",
            Self::English => "en",
            Self::Hungarian => "hu",
            Self::EnglishUk => "en-GB",
        }
    }

    pub fn from_locale(locale: &str) -> Self {
        let locale = locale.to_ascii_lowercase().replace('_', "-");
        let mut parts = locale.split(['-', '.', '@']);
        match parts.next() {
            Some("en") if parts.any(|part| part == "gb") => Self::EnglishUk,
            Some("hu") => Self::Hungarian,
            _ => Self::English,
        }
    }
}

/// What opens when Cairn starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupMode {
    #[default]
    RestoreTabs,
    Home,
}

/// How dates are shown in lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    #[default]
    Relative,
    Local,
    Iso,
}

/// How graph lines bend between lanes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphStyle {
    #[default]
    Curved,
    Angular,
}

/// Unified or side-by-side diffs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLayout {
    #[default]
    Unified,
    Split,
}

/// Whether changed words or whole lines are highlighted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffHighlight {
    #[default]
    Words,
    Lines,
}

/// When AI suggests commit messages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiSuggest {
    #[default]
    OnRequest,
    Always,
    Never,
}

/// How Cairn talks to an AI model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    ClaudeCode,
    ApiKey,
    CustomEndpoint,
    CodexCli,
    #[default]
    Automatic,
}

impl AiProvider {
    pub fn uses_api(self) -> bool {
        matches!(self, Self::ApiKey | Self::CustomEndpoint)
    }
}

/// When the AI review runs before a push.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiReview {
    #[default]
    Ask,
    Always,
    Never,
}

/// When configured checks run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChecksWhen {
    #[default]
    CommitAndPush,
    PushOnly,
    Never,
}

/// What happens right after a successful commit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AfterCommit {
    #[default]
    Nothing,
    Push,
    Sync,
}

/// What the commit button does when nothing is staged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartCommit {
    #[default]
    Ask,
    Always,
    Never,
}

/// General behavior.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub startup: StartupMode,
    pub language: Language,
    pub clone_dir: String,
    pub confirm_quit_during_operation: bool,
    pub watch_files: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            startup: StartupMode::RestoreTabs,
            language: Language::default(),
            clone_dir: dirs::home_dir().map(|h| h.join("repos").to_string_lossy().into_owned()).unwrap_or_default(),
            confirm_quit_during_operation: true,
            watch_files: true,
        }
    }
}

/// Look and feel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemeMode,
    pub accent: Accent,
    pub density: Density,
    pub ui_font_size: f32,
    pub code_font: String,
    pub code_font_size: f32,
    pub show_avatars: bool,
    pub reduce_motion: bool,
    pub palette: Palette,
    pub glass: bool,
    pub compact_toolbar: bool,
    pub details_width: f32,
    pub list_width: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            accent: Accent::Blue,
            density: Density::Compact,
            ui_font_size: 13.0,
            code_font: "JetBrains Mono".into(),
            code_font_size: 12.5,
            show_avatars: true,
            reduce_motion: false,
            palette: Palette::Graphite,
            glass: true,
            compact_toolbar: false,
            details_width: 380.,
            list_width: 300.,
        }
    }
}

/// The set of surface colors the whole app is painted with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    #[default]
    Graphite,
    Midnight,
    Nord,
    Solarized,
    Rose,
    HighContrast,
}

impl Palette {
    /// Every palette, in the order the gallery shows them.
    pub const ALL: [Palette; 6] = [Palette::Graphite, Palette::Midnight, Palette::Nord, Palette::Solarized, Palette::Rose, Palette::HighContrast];
}

/// Commit list and graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Graph {
    pub all_branches: bool,
    pub style: GraphStyle,
    pub show_remotes: bool,
    pub show_tags: bool,
    pub show_stashes: bool,
    pub first_parent: bool,
    pub date_format: DateFormat,
    pub hash_length: usize,
    pub page_size: usize,
}

impl Default for Graph {
    fn default() -> Self {
        Self {
            all_branches: false,
            style: GraphStyle::Curved,
            show_remotes: true,
            show_tags: true,
            show_stashes: false,
            first_parent: false,
            date_format: DateFormat::Relative,
            hash_length: 7,
            page_size: 500,
        }
    }
}

/// Diff display.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Diffs {
    pub layout: DiffLayout,
    pub highlight: DiffHighlight,
    pub context_lines: u32,
    pub ignore_whitespace: bool,
    pub syntax_highlighting: bool,
    pub line_numbers: bool,
    pub wrap_lines: bool,
    pub tab_width: u32,
    pub collapse_larger_than_kb: u32,
}

impl Default for Diffs {
    fn default() -> Self {
        Self {
            layout: DiffLayout::Unified,
            highlight: DiffHighlight::Words,
            context_lines: 3,
            ignore_whitespace: false,
            syntax_highlighting: true,
            line_numbers: true,
            wrap_lines: false,
            tab_width: 4,
            collapse_larger_than_kb: 512,
        }
    }
}

/// Commit creation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Commits {
    pub conventional: bool,
    pub summary_limit: usize,
    pub warn_long_summary: bool,
    pub ai_messages: AiSuggest,
    pub ai_instructions: String,
    pub sign: bool,
    pub run_checks: bool,
    pub confirm_amend_pushed: bool,
    pub after_commit: AfterCommit,
    pub smart_commit: SmartCommit,
    pub ask_on_main: bool,
}

impl Default for Commits {
    fn default() -> Self {
        Self {
            conventional: true,
            summary_limit: 72,
            warn_long_summary: true,
            ai_messages: AiSuggest::OnRequest,
            ai_instructions: String::new(),
            sign: false,
            run_checks: true,
            confirm_amend_pushed: true,
            after_commit: AfterCommit::Nothing,
            smart_commit: SmartCommit::Ask,
            ask_on_main: true,
        }
    }
}

/// Pull, rebase and push defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GitBehavior {
    pub pull_mode: PullMode,
    pub auto_fetch_minutes: u32,
    pub prune_on_fetch: bool,
    pub autostash: bool,
    pub autosquash: bool,
    pub update_refs: bool,
    pub rerere: bool,
    pub force_mode: ForceMode,
    pub push_tags: bool,
    pub default_branch: String,
    pub confirm_sync: bool,
    pub warn_rewritten_upstream: bool,
    pub branch_prefix: String,
    pub update_on_switch: bool,
    pub worktree_copy: String,
}

impl Default for GitBehavior {
    fn default() -> Self {
        Self {
            pull_mode: PullMode::Rebase,
            auto_fetch_minutes: 15,
            prune_on_fetch: true,
            autostash: true,
            autosquash: true,
            update_refs: true,
            rerere: true,
            force_mode: ForceMode::WithLease,
            push_tags: false,
            default_branch: "main".into(),
            confirm_sync: false,
            warn_rewritten_upstream: true,
            branch_prefix: String::new(),
            update_on_switch: true,
            worktree_copy: ".env, .env.local".into(),
        }
    }
}

/// Hosting accounts and pull requests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Remotes {
    pub show_prs_in_graph: bool,
    pub ci_refresh_seconds: u32,
    pub draft_prs: bool,
    pub link_stacked_prs: bool,
}

impl Default for Remotes {
    fn default() -> Self {
        Self { show_prs_in_graph: true, ci_refresh_seconds: 120, draft_prs: true, link_stacked_prs: true }
    }
}

/// Multi-repo behavior inside a category.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub link_same_named_branches: bool,
    pub push_dependencies_first: bool,
    pub all_or_nothing_push: bool,
    pub worktree_dir: String,
    pub cleanup_merged_worktrees_days: u32,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            link_same_named_branches: true,
            push_dependencies_first: true,
            all_or_nothing_push: true,
            worktree_dir: String::new(),
            cleanup_merged_worktrees_days: 7,
        }
    }
}

/// AI features and connection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ai {
    pub enabled: bool,
    pub provider: AiProvider,
    pub endpoint_url: String,
    pub monthly_limit_usd: u32,
    pub model_deep: String,
    pub model_fast: String,
    pub codex_model_deep: String,
    pub codex_model_fast: String,
    pub show_usage: bool,
    pub review: AiReview,
    pub conflict_suggestions: bool,
    pub pr_descriptions: bool,
    pub send_whole_files: bool,
    pub excluded_paths: String,
    pub redact_secrets: bool,
}

impl Default for Ai {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: AiProvider::default(),
            endpoint_url: "http://localhost:4000".into(),
            monthly_limit_usd: 20,
            model_deep: "claude-opus-5-5".into(),
            model_fast: "claude-haiku-5-5".into(),
            codex_model_deep: String::new(),
            codex_model_fast: String::new(),
            show_usage: false,
            review: AiReview::Ask,
            conflict_suggestions: true,
            pr_descriptions: true,
            send_whole_files: false,
            excluded_paths: "**/.env*, **/secrets/**".into(),
            redact_secrets: true,
        }
    }
}

/// Pre-commit and pre-push checks plus undo history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Checks {
    pub when: ChecksWhen,
    pub config_file: String,
    pub timeout_seconds: u32,
    pub secret_scan: bool,
    pub block_on_secret: bool,
    pub snapshots: bool,
    pub undo_days: u32,
    pub protect_main: bool,
}

impl Default for Checks {
    fn default() -> Self {
        Self {
            when: ChecksWhen::CommitAndPush,
            config_file: ".cairn/checks.toml".into(),
            timeout_seconds: 120,
            secret_scan: true,
            block_on_secret: true,
            snapshots: true,
            undo_days: 30,
            protect_main: true,
        }
    }
}

/// Keyboard behavior.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keyboard {
    pub vim_keys: bool,
    pub show_hints: bool,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self { vim_keys: true, show_hints: true }
    }
}

/// Desktop notifications.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    pub ci_finished: bool,
    pub reviews: bool,
    pub branch_behind: bool,
    pub fetch_found_commits: bool,
    pub sound: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self { ci_finished: true, reviews: true, branch_behind: false, fetch_found_commits: false, sound: false }
    }
}

/// Performance tuning.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Performance {
    pub write_commit_graph: bool,
    pub fsmonitor: bool,
}

impl Default for Performance {
    fn default() -> Self {
        Self { write_commit_graph: true, fsmonitor: false }
    }
}

/// Updates and privacy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Updates {
    pub auto_update: bool,
    pub show_release_notes: bool,
    pub last_seen_version: String,
}

impl Default for Updates {
    fn default() -> Self {
        Self { auto_update: true, show_release_notes: true, last_seen_version: String::new() }
    }
}

/// External programs Cairn hands files to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tools {
    pub editor: String,
    pub diff_tool: String,
    pub merge_tool: String,
}

/// Troubleshooting options.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Advanced {
    pub log_git_commands: bool,
}

/// A named git identity applied to repositories whose path matches its folder.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IdentityProfile {
    pub name: String,
    pub user_name: String,
    pub email: String,
    pub signing_key: String,
    pub folder: String,
}

impl IdentityProfile {
    /// Whether a repository path is inside this profile's folder.
    ///
    /// # Arguments
    /// * `path` - repository root
    ///
    /// # Returns
    /// `true` when the folder is set and contains the path.
    pub fn matches(&self, path: &std::path::Path) -> bool {
        if self.folder.trim().is_empty() {
            return false;
        }
        let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
        let p = norm(&path.to_string_lossy());
        let f = norm(&self.folder);
        p == f || p.starts_with(&format!("{f}/"))
    }
}

/// Every user setting, stored in `settings.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub profiles: Vec<IdentityProfile>,
    pub general: General,
    pub appearance: Appearance,
    pub graph: Graph,
    pub diffs: Diffs,
    pub commits: Commits,
    pub git: GitBehavior,
    pub remotes: Remotes,
    pub workspace: Workspace,
    pub ai: Ai,
    pub checks: Checks,
    pub keyboard: Keyboard,
    pub notifications: Notifications,
    pub performance: Performance,
    pub updates: Updates,
    pub tools: Tools,
    pub advanced: Advanced,
}

impl Settings {
    /// The settings file path.
    ///
    /// # Returns
    /// `%APPDATA%\Cairn\settings.toml` on Windows.
    pub fn file() -> PathBuf {
        data_dir().join("settings.toml")
    }

    /// Loads settings, falling back to defaults for anything missing.
    ///
    /// # Returns
    /// The user's settings.
    pub fn load() -> Self {
        std::fs::read_to_string(Self::file())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Writes settings to disk.
    ///
    /// # Returns
    /// `Ok(())` when saved.
    pub fn save(&self) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| crate::Error::Other(e.to_string()))?;
        write_atomic(&Self::file(), text.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml_and_fills_missing_keys() {
        let mut s = Settings::default();
        s.appearance.theme = ThemeMode::Light;
        s.general.language = Language::Hungarian;
        let text = toml::to_string_pretty(&s).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back, s);
        let partial: Settings = toml::from_str("[appearance]\ntheme = \"light\"\n").unwrap();
        assert_eq!(partial.appearance.theme, ThemeMode::Light);
        assert_eq!(partial.general.language, Language::System);
        assert_eq!(partial.git.pull_mode, PullMode::Rebase);
        assert_eq!(partial.ai.provider, AiProvider::Automatic);
    }

    #[test]
    fn preserves_saved_claude_provider() {
        let settings: Settings = toml::from_str("[ai]\nprovider = \"claude_code\"\n").unwrap();
        assert_eq!(settings.ai.provider, AiProvider::ClaudeCode);
        assert!(settings.ai.codex_model_fast.is_empty());
    }

    #[test]
    fn language_selection_round_trips_and_preserves_legacy_english() {
        let legacy: Settings = toml::from_str("[general]\nlanguage = \"english\"\n").unwrap();
        assert_eq!(legacy.general.language, Language::English);

        for language in std::iter::once(Language::System).chain(Language::ALL) {
            let mut settings = Settings::default();
            settings.general.language = language;
            let saved = toml::to_string(&settings).unwrap();
            let restored: Settings = toml::from_str(&saved).unwrap();
            assert_eq!(restored.general.language, language);
        }
    }

    #[test]
    fn system_locale_maps_to_supported_languages() {
        for (locale, language) in [
            ("en_GB.UTF-8", Language::EnglishUk),
            ("en-US", Language::English),
            ("hu-HU", Language::Hungarian),
            ("ru-RU", Language::English),
            ("pt-PT", Language::English),
            ("zh-Hant", Language::English),
            ("fi-FI", Language::English),
            ("", Language::English),
        ] {
            assert_eq!(Language::from_locale(locale), language, "{locale}");
        }
    }
}
