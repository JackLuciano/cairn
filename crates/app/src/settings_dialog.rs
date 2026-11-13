use std::collections::HashMap;

use cairn_core::ops::{ForceMode, PullMode};
use cairn_core::settings::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use crate::app::CairnApp;
use crate::widgets::{hstack, icon, spacer, tr};

type GetB = fn(&Settings) -> bool;
type SetB = fn(&mut Settings, bool);
type GetI = fn(&Settings) -> usize;
type SetI = fn(&mut Settings, usize);
type GetN = fn(&Settings) -> f64;
type SetN = fn(&mut Settings, f64);
type GetS = fn(&Settings) -> String;
type SetS = fn(&mut Settings, String);

#[derive(Clone, Copy)]
enum Control {
    Toggle(GetB, SetB),
    Choice(&'static [&'static str], GetI, SetI),
    Number(GetN, SetN, f64, f64, f64, &'static str),
    Text(GetS, SetS),
    LongText(GetS, SetS),
    Secret(&'static str),
    TestAi,
    CheckUpdates,
}

#[derive(Clone, Copy)]
struct RowDef {
    id: &'static str,
    label: &'static str,
    desc: Option<&'static str>,
    control: Control,
    visible: Option<fn(&Settings) -> bool>,
}

struct Group {
    label: &'static str,
    rows: Vec<RowDef>,
}

struct Category {
    id: &'static str,
    label: &'static str,
    desc: &'static str,
    groups: Vec<Group>,
}

const fn row(id: &'static str, label: &'static str, desc: Option<&'static str>, control: Control) -> RowDef {
    RowDef { id, label, desc, control, visible: None }
}

fn ai_on(s: &Settings) -> bool {
    s.ai.enabled
}

fn schema() -> Vec<Category> {
    vec![
        Category {
            id: "general",
            label: "settings.cat.general",
            desc: "settings.cat.general_desc",
            groups: vec![
                Group {
                    label: "settings.group.startup",
                    rows: vec![
                        row("startup", "settings.startup", None, Control::Choice(&["opt.restore_tabs", "opt.home"], |s| (s.general.startup == StartupMode::Home) as usize, |s, i| s.general.startup = if i == 1 { StartupMode::Home } else { StartupMode::RestoreTabs })),
                        row("language", "settings.language", Some("settings.language_desc"), Control::Choice(&["opt.english", "opt.hungarian"], |s| (s.general.language == Language::Hungarian) as usize, |s, i| s.general.language = if i == 1 { Language::Hungarian } else { Language::English })),
                        row("confirm_quit", "settings.confirm_quit", Some("settings.confirm_quit_desc"), Control::Toggle(|s| s.general.confirm_quit_during_operation, |s, v| s.general.confirm_quit_during_operation = v)),
                    ],
                },
                Group {
                    label: "settings.group.files",
                    rows: vec![
                        row("watch", "settings.watch_files", Some("settings.watch_files_desc"), Control::Toggle(|s| s.general.watch_files, |s, v| s.general.watch_files = v)),
                        row("clone_dir", "settings.clone_dir", None, Control::Text(|s| s.general.clone_dir.clone(), |s, v| s.general.clone_dir = v)),
                    ],
                },
            ],
        },
        Category {
            id: "appearance",
            label: "settings.cat.appearance",
            desc: "settings.cat.appearance_desc",
            groups: vec![
                Group {
                    label: "settings.group.theme",
                    rows: vec![
                        row("theme", "settings.theme", None, Control::Choice(&["opt.dark", "opt.light", "opt.system"], |s| match s.appearance.theme { ThemeMode::Dark => 0, ThemeMode::Light => 1, ThemeMode::System => 2 }, |s, i| s.appearance.theme = [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System][i])),
                        row("palette", "settings.palette", Some("settings.palette_desc"), Control::Choice(&["opt.palette_graphite", "opt.palette_midnight", "opt.palette_nord", "opt.palette_solarized", "opt.palette_rose", "opt.palette_contrast"], |s| Palette::ALL.iter().position(|p| *p == s.appearance.palette).unwrap_or(0), |s, i| s.appearance.palette = Palette::ALL[i])),
                        row("accent", "settings.accent", None, Control::Choice(&["opt.blue", "opt.purple", "opt.teal", "opt.orange"], |s| s.appearance.accent as usize, |s, i| s.appearance.accent = [Accent::Blue, Accent::Purple, Accent::Teal, Accent::Orange][i])),
                        RowDef { visible: Some(|_| crate::theme::glass_supported()), ..row("glass", "settings.glass", Some("settings.glass_desc"), Control::Toggle(|s| s.appearance.glass, |s, v| s.appearance.glass = v)) },
                        row("compact_toolbar", "settings.compact_toolbar", Some("settings.compact_toolbar_desc"), Control::Toggle(|s| s.appearance.compact_toolbar, |s, v| s.appearance.compact_toolbar = v)),
                        row("reduce_motion", "settings.reduce_motion", Some("settings.reduce_motion_desc"), Control::Toggle(|s| s.appearance.reduce_motion, |s, v| s.appearance.reduce_motion = v)),
                    ],
                },
                Group {
                    label: "settings.group.text",
                    rows: vec![
                        row("ui_size", "settings.ui_size", None, Control::Number(|s| f64::from(s.appearance.ui_font_size), |s, v| s.appearance.ui_font_size = v as f32, 11., 18., 1., "unit.px")),
                        row("code_font", "settings.code_font", None, Control::Text(|s| s.appearance.code_font.clone(), |s, v| s.appearance.code_font = v)),
                        row("code_size", "settings.code_size", None, Control::Number(|s| f64::from(s.appearance.code_font_size), |s, v| s.appearance.code_font_size = v as f32, 10., 18., 0.5, "unit.px")),
                        row("avatars", "settings.avatars", None, Control::Toggle(|s| s.appearance.show_avatars, |s, v| s.appearance.show_avatars = v)),
                    ],
                },
            ],
        },
        Category {
            id: "graph",
            label: "settings.cat.graph",
            desc: "settings.cat.graph_desc",
            groups: vec![
                Group {
                    label: "settings.group.graph",
                    rows: vec![
                        row("all_branches", "settings.all_branches", Some("settings.all_branches_desc"), Control::Toggle(|s| s.graph.all_branches, |s, v| s.graph.all_branches = v)),
                        row("graph_style", "settings.graph_style", None, Control::Choice(&["opt.curved", "opt.angular"], |s| (s.graph.style == GraphStyle::Angular) as usize, |s, i| s.graph.style = if i == 1 { GraphStyle::Angular } else { GraphStyle::Curved })),
                        row("show_remotes", "settings.show_remotes", None, Control::Toggle(|s| s.graph.show_remotes, |s, v| s.graph.show_remotes = v)),
                        row("show_tags", "settings.show_tags", None, Control::Toggle(|s| s.graph.show_tags, |s, v| s.graph.show_tags = v)),
                        row("show_stashes", "settings.show_stashes", None, Control::Toggle(|s| s.graph.show_stashes, |s, v| s.graph.show_stashes = v)),
                        row("first_parent", "settings.first_parent", Some("settings.first_parent_desc"), Control::Toggle(|s| s.graph.first_parent, |s, v| s.graph.first_parent = v)),
                    ],
                },
                Group {
                    label: "settings.group.list",
                    rows: vec![
                        row("date_format", "settings.date_format", None, Control::Choice(&["opt.relative", "opt.local", "opt.iso"], |s| s.graph.date_format as usize, |s, i| s.graph.date_format = [DateFormat::Relative, DateFormat::Local, DateFormat::Iso][i])),
                        row("hash_length", "settings.hash_length", None, Control::Number(|s| s.graph.hash_length as f64, |s, v| s.graph.hash_length = v as usize, 4., 40., 1., "unit.chars")),
                        row("page_size", "settings.page_size", Some("settings.page_size_desc"), Control::Number(|s| s.graph.page_size as f64, |s, v| s.graph.page_size = v as usize, 100., 20000., 100., "unit.commits")),
                    ],
                },
            ],
        },
        Category {
            id: "diffs",
            label: "settings.cat.diffs",
            desc: "settings.cat.diffs_desc",
            groups: vec![Group {
                label: "settings.group.diffs",
                rows: vec![
                    row("highlight", "settings.highlight", Some("settings.highlight_desc"), Control::Choice(&["opt.words", "opt.lines"], |s| (s.diffs.highlight == DiffHighlight::Lines) as usize, |s, i| s.diffs.highlight = if i == 1 { DiffHighlight::Lines } else { DiffHighlight::Words })),
                    row("context", "settings.context_lines", None, Control::Number(|s| f64::from(s.diffs.context_lines), |s, v| s.diffs.context_lines = v as u32, 0., 20., 1., "unit.lines")),
                    row("diff_layout", "settings.diff_layout", Some("settings.diff_layout_desc"), Control::Choice(&["opt.unified", "opt.split"], |s| (s.diffs.layout == DiffLayout::Split) as usize, |s, i| s.diffs.layout = if i == 1 { DiffLayout::Split } else { DiffLayout::Unified })),
                    row("syntax", "settings.syntax_highlighting", None, Control::Toggle(|s| s.diffs.syntax_highlighting, |s, v| s.diffs.syntax_highlighting = v)),
                    row("wrap_lines", "settings.wrap_lines", Some("settings.wrap_lines_desc"), Control::Toggle(|s| s.diffs.wrap_lines, |s, v| s.diffs.wrap_lines = v)),
                    row("ignore_ws", "settings.ignore_whitespace", None, Control::Toggle(|s| s.diffs.ignore_whitespace, |s, v| s.diffs.ignore_whitespace = v)),
                    row("line_numbers", "settings.line_numbers", None, Control::Toggle(|s| s.diffs.line_numbers, |s, v| s.diffs.line_numbers = v)),
                    row("tab_width", "settings.tab_width", None, Control::Number(|s| f64::from(s.diffs.tab_width), |s, v| s.diffs.tab_width = v as u32, 1., 8., 1., "unit.spaces")),
                    row("collapse", "settings.collapse_large", None, Control::Number(|s| f64::from(s.diffs.collapse_larger_than_kb), |s, v| s.diffs.collapse_larger_than_kb = v as u32, 64., 8192., 64., "unit.kb")),
                ],
            }],
        },
        Category {
            id: "commits",
            label: "settings.cat.commits",
            desc: "settings.cat.commits_desc",
            groups: vec![Group {
                label: "settings.group.messages",
                rows: vec![
                    row("conventional", "settings.conventional", Some("settings.conventional_desc"), Control::Toggle(|s| s.commits.conventional, |s, v| s.commits.conventional = v)),
                    row("summary_limit", "settings.summary_limit", None, Control::Number(|s| s.commits.summary_limit as f64, |s, v| s.commits.summary_limit = v as usize, 30., 200., 2., "unit.chars")),
                    row("warn_long", "settings.warn_long", None, Control::Toggle(|s| s.commits.warn_long_summary, |s, v| s.commits.warn_long_summary = v)),
                    row("sign", "settings.sign", Some("settings.sign_desc"), Control::Toggle(|s| s.commits.sign, |s, v| s.commits.sign = v)),
                    row("checks_commit", "settings.checks_on_commit", None, Control::Toggle(|s| s.commits.run_checks, |s, v| s.commits.run_checks = v)),
                    row("ai_messages", "settings.ai_messages", None, Control::Choice(&["opt.on_request", "opt.always", "opt.never"], |s| s.commits.ai_messages as usize, |s, i| s.commits.ai_messages = [AiSuggest::OnRequest, AiSuggest::Always, AiSuggest::Never][i])),
                    row("ai_instructions", "settings.ai_instructions", Some("settings.ai_instructions_desc"), Control::LongText(|s| s.commits.ai_instructions.clone(), |s, v| s.commits.ai_instructions = v)),
                ],
            }],
        },
        Category {
            id: "git",
            label: "settings.cat.git",
            desc: "settings.cat.git_desc",
            groups: vec![
                Group {
                    label: "settings.group.pull",
                    rows: vec![
                        row("pull_mode", "settings.pull_mode", None, Control::Choice(&["opt.rebase", "opt.merge", "opt.ff_only"], |s| match s.git.pull_mode { PullMode::Rebase => 0, PullMode::Merge => 1, PullMode::FastForwardOnly => 2 }, |s, i| s.git.pull_mode = [PullMode::Rebase, PullMode::Merge, PullMode::FastForwardOnly][i])),
                        row("auto_fetch", "settings.auto_fetch", Some("settings.auto_fetch_desc"), Control::Number(|s| f64::from(s.git.auto_fetch_minutes), |s, v| s.git.auto_fetch_minutes = v as u32, 0., 240., 5., "unit.minutes")),
                        row("prune", "settings.prune", None, Control::Toggle(|s| s.git.prune_on_fetch, |s, v| s.git.prune_on_fetch = v)),
                        row("autostash", "settings.autostash", None, Control::Toggle(|s| s.git.autostash, |s, v| s.git.autostash = v)),
                    ],
                },
                Group {
                    label: "settings.group.rebase",
                    rows: vec![
                        row("autosquash", "settings.autosquash", None, Control::Toggle(|s| s.git.autosquash, |s, v| s.git.autosquash = v)),
                        row("update_refs", "settings.update_refs", Some("settings.update_refs_desc"), Control::Toggle(|s| s.git.update_refs, |s, v| s.git.update_refs = v)),
                        row("rerere", "settings.rerere", Some("settings.rerere_desc"), Control::Toggle(|s| s.git.rerere, |s, v| s.git.rerere = v)),
                    ],
                },
                Group {
                    label: "settings.group.push",
                    rows: vec![
                        row("force", "settings.force_mode", None, Control::Choice(&["opt.with_lease", "opt.never"], |s| (s.git.force_mode == ForceMode::None) as usize, |s, i| s.git.force_mode = if i == 1 { ForceMode::None } else { ForceMode::WithLease })),
                        row("push_tags", "settings.push_tags", None, Control::Toggle(|s| s.git.push_tags, |s, v| s.git.push_tags = v)),
                        row("default_branch", "settings.default_branch", None, Control::Text(|s| s.git.default_branch.clone(), |s, v| s.git.default_branch = v)),
                    ],
                },
            ],
        },
        Category {
            id: "remotes",
            label: "settings.cat.remotes",
            desc: "settings.cat.remotes_desc",
            groups: vec![Group {
                label: "settings.group.prs",
                rows: vec![
                    row("prs_graph", "settings.prs_in_graph", None, Control::Toggle(|s| s.remotes.show_prs_in_graph, |s, v| s.remotes.show_prs_in_graph = v)),
                    row("ci_refresh", "settings.ci_refresh", None, Control::Number(|s| f64::from(s.remotes.ci_refresh_seconds), |s, v| s.remotes.ci_refresh_seconds = v as u32, 30., 900., 30., "unit.seconds")),
                    row("draft_prs", "settings.draft_prs", None, Control::Toggle(|s| s.remotes.draft_prs, |s, v| s.remotes.draft_prs = v)),
                    row("link_stack", "settings.link_stack", None, Control::Toggle(|s| s.remotes.link_stacked_prs, |s, v| s.remotes.link_stacked_prs = v)),
                ],
            }],
        },
        Category {
            id: "workspace",
            label: "settings.cat.workspace",
            desc: "settings.cat.workspace_desc",
            groups: vec![Group {
                label: "settings.group.linked",
                rows: vec![
                    row("link_branches", "settings.link_branches", Some("settings.link_branches_desc"), Control::Toggle(|s| s.workspace.link_same_named_branches, |s, v| s.workspace.link_same_named_branches = v)),
                    row("push_order", "settings.push_order", None, Control::Toggle(|s| s.workspace.push_dependencies_first, |s, v| s.workspace.push_dependencies_first = v)),
                    row("all_or_nothing", "settings.all_or_nothing", None, Control::Toggle(|s| s.workspace.all_or_nothing_push, |s, v| s.workspace.all_or_nothing_push = v)),
                    row("worktree_dir", "settings.worktree_dir", Some("settings.worktree_dir_desc"), Control::Text(|s| s.workspace.worktree_dir.clone(), |s, v| s.workspace.worktree_dir = v)),
                    row("worktree_cleanup", "settings.worktree_cleanup", Some("settings.worktree_cleanup_desc"), Control::Number(|s| f64::from(s.workspace.cleanup_merged_worktrees_days), |s, v| s.workspace.cleanup_merged_worktrees_days = v as u32, 0., 90., 1., "unit.days")),
                ],
            }],
        },
        Category {
            id: "ai",
            label: "settings.cat.ai",
            desc: "settings.cat.ai_desc",
            groups: vec![
                Group {
                    label: "settings.group.connection",
                    rows: vec![
                        row("ai_enabled", "settings.ai_enabled", None, Control::Toggle(|s| s.ai.enabled, |s, v| s.ai.enabled = v)),
                        RowDef { visible: Some(ai_on), ..row("ai_provider", "settings.ai_provider", Some("settings.ai_provider_desc"), Control::Choice(&["opt.claude_code", "opt.api_key", "opt.custom"], |s| s.ai.provider as usize, |s, i| s.ai.provider = [AiProvider::ClaudeCode, AiProvider::ApiKey, AiProvider::CustomEndpoint][i])) },
                        RowDef { visible: Some(|s| s.ai.enabled && s.ai.provider == AiProvider::ApiKey), ..row("api_key", "settings.api_key", Some("settings.api_key_desc"), Control::Secret("anthropic")) },
                        RowDef { visible: Some(|s| s.ai.enabled && s.ai.provider == AiProvider::CustomEndpoint), ..row("endpoint", "settings.endpoint", Some("settings.endpoint_desc"), Control::Text(|s| s.ai.endpoint_url.clone(), |s, v| s.ai.endpoint_url = v)) },
                        RowDef { visible: Some(|s| s.ai.enabled && s.ai.provider == AiProvider::CustomEndpoint), ..row("endpoint_key", "settings.endpoint_key", None, Control::Secret("custom")) },
                        RowDef { visible: Some(ai_on), ..row("ai_test", "ai.test", Some("settings.ai_test_desc"), Control::TestAi) },
                        RowDef { visible: Some(|s| s.ai.enabled && s.ai.provider != AiProvider::ClaudeCode), ..row("limit", "settings.monthly_limit", Some("settings.monthly_limit_desc"), Control::Number(|s| f64::from(s.ai.monthly_limit_usd), |s, v| s.ai.monthly_limit_usd = v as u32, 0., 1000., 5., "unit.usd")) },
                    ],
                },
                Group {
                    label: "settings.group.models",
                    rows: vec![
                        RowDef { visible: Some(ai_on), ..row("model_deep", "settings.model_deep", None, Control::Choice(&["model.opus", "model.sonnet"], |s| (s.ai.model_deep != "claude-opus-5-5") as usize, |s, i| s.ai.model_deep = ["claude-opus-5-5", "claude-sonnet-5-5"][i].into())) },
                        RowDef { visible: Some(ai_on), ..row("model_fast", "settings.model_fast", None, Control::Choice(&["model.haiku", "model.sonnet", "model.opus"], |s| ["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"].iter().position(|m| *m == s.ai.model_fast).unwrap_or(0), |s, i| s.ai.model_fast = ["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"][i].into())) },
                        RowDef { visible: Some(ai_on), ..row("show_usage", "settings.show_usage", None, Control::Toggle(|s| s.ai.show_usage, |s, v| s.ai.show_usage = v)) },
                    ],
                },
                Group {
                    label: "settings.group.ai_features",
                    rows: vec![
                        RowDef { visible: Some(ai_on), ..row("ai_review", "settings.ai_review", None, Control::Choice(&["opt.ask", "opt.always", "opt.never"], |s| s.ai.review as usize, |s, i| s.ai.review = [AiReview::Ask, AiReview::Always, AiReview::Never][i])) },
                        RowDef { visible: Some(ai_on), ..row("ai_conflicts", "settings.ai_conflicts", None, Control::Toggle(|s| s.ai.conflict_suggestions, |s, v| s.ai.conflict_suggestions = v)) },
                        RowDef { visible: Some(ai_on), ..row("ai_pr", "settings.ai_pr", None, Control::Toggle(|s| s.ai.pr_descriptions, |s, v| s.ai.pr_descriptions = v)) },
                    ],
                },
                Group {
                    label: "settings.group.privacy",
                    rows: vec![
                        RowDef { visible: Some(ai_on), ..row("ai_context", "settings.ai_context", None, Control::Choice(&["opt.diff_only", "opt.whole_files"], |s| s.ai.send_whole_files as usize, |s, i| s.ai.send_whole_files = i == 1)) },
                        RowDef { visible: Some(ai_on), ..row("ai_exclude", "settings.ai_exclude", None, Control::Text(|s| s.ai.excluded_paths.clone(), |s, v| s.ai.excluded_paths = v)) },
                        RowDef { visible: Some(ai_on), ..row("ai_redact", "settings.ai_redact", None, Control::Toggle(|s| s.ai.redact_secrets, |s, v| s.ai.redact_secrets = v)) },
                    ],
                },
            ],
        },
        Category {
            id: "checks",
            label: "settings.cat.checks",
            desc: "settings.cat.checks_desc",
            groups: vec![
                Group {
                    label: "settings.group.checks",
                    rows: vec![
                        row("checks_when", "settings.checks_when", None, Control::Choice(&["opt.commit_and_push", "opt.push_only", "opt.never"], |s| s.checks.when as usize, |s, i| s.checks.when = [ChecksWhen::CommitAndPush, ChecksWhen::PushOnly, ChecksWhen::Never][i])),
                        row("checks_file", "settings.checks_file", None, Control::Text(|s| s.checks.config_file.clone(), |s, v| s.checks.config_file = v)),
                        row("checks_timeout", "settings.checks_timeout", None, Control::Number(|s| f64::from(s.checks.timeout_seconds), |s, v| s.checks.timeout_seconds = v as u32, 10., 1800., 10., "unit.seconds")),
                        row("secret_scan", "settings.secret_scan", None, Control::Toggle(|s| s.checks.secret_scan, |s, v| s.checks.secret_scan = v)),
                        row("block_secret", "settings.block_secret", None, Control::Toggle(|s| s.checks.block_on_secret, |s, v| s.checks.block_on_secret = v)),
                    ],
                },
                Group {
                    label: "settings.group.undo",
                    rows: vec![
                        row("snapshots", "settings.snapshots", Some("settings.snapshots_desc"), Control::Toggle(|s| s.checks.snapshots, |s, v| s.checks.snapshots = v)),
                        row("undo_days", "settings.undo_days", None, Control::Number(|s| f64::from(s.checks.undo_days), |s, v| s.checks.undo_days = v as u32, 1., 365., 1., "unit.days")),
                        row("protect_main", "settings.protect_main", None, Control::Toggle(|s| s.checks.protect_main, |s, v| s.checks.protect_main = v)),
                    ],
                },
            ],
        },
        Category {
            id: "keyboard",
            label: "settings.cat.keyboard",
            desc: "settings.cat.keyboard_desc",
            groups: vec![Group {
                label: "settings.group.keys",
                rows: vec![
                    row("vim", "settings.vim", Some("settings.vim_desc"), Control::Toggle(|s| s.keyboard.vim_keys, |s, v| s.keyboard.vim_keys = v)),
                    row("hints", "settings.key_hints", None, Control::Toggle(|s| s.keyboard.show_hints, |s, v| s.keyboard.show_hints = v)),
                ],
            }],
        },
        Category {
            id: "notifications",
            label: "settings.cat.notifications",
            desc: "settings.cat.notifications_desc",
            groups: vec![Group {
                label: "settings.group.notify_when",
                rows: vec![
                    row("n_ci", "settings.n_ci", None, Control::Toggle(|s| s.notifications.ci_finished, |s, v| s.notifications.ci_finished = v)),
                    row("n_review", "settings.n_review", None, Control::Toggle(|s| s.notifications.reviews, |s, v| s.notifications.reviews = v)),
                    row("n_behind", "settings.n_behind", None, Control::Toggle(|s| s.notifications.branch_behind, |s, v| s.notifications.branch_behind = v)),
                    row("n_fetch", "settings.n_fetch", None, Control::Toggle(|s| s.notifications.fetch_found_commits, |s, v| s.notifications.fetch_found_commits = v)),
                    row("n_sound", "settings.n_sound", None, Control::Toggle(|s| s.notifications.sound, |s, v| s.notifications.sound = v)),
                ],
            }],
        },
        Category {
            id: "performance",
            label: "settings.cat.performance",
            desc: "settings.cat.performance_desc",
            groups: vec![Group {
                label: "settings.group.indexing",
                rows: vec![
                    row("commit_graph", "settings.commit_graph", Some("settings.commit_graph_desc"), Control::Toggle(|s| s.performance.write_commit_graph, |s, v| s.performance.write_commit_graph = v)),
                    row("fsmonitor", "settings.fsmonitor", Some("settings.fsmonitor_desc"), Control::Toggle(|s| s.performance.fsmonitor, |s, v| s.performance.fsmonitor = v)),
                ],
            }],
        },
        Category {
            id: "updates",
            label: "settings.cat.updates",
            desc: "settings.cat.updates_desc",
            groups: vec![Group {
                label: "settings.group.updates",
                rows: vec![
                    row("check_updates", "settings.check_updates", Some("settings.check_updates_desc"), Control::CheckUpdates),
                    row("auto_update", "settings.auto_update", Some("settings.auto_update_desc"), Control::Toggle(|s| s.updates.auto_update, |s, v| s.updates.auto_update = v)),
                    row("release_notes", "settings.release_notes", None, Control::Toggle(|s| s.updates.show_release_notes, |s, v| s.updates.show_release_notes = v)),
                ],
            }],
        },
        Category {
            id: "tools",
            label: "settings.cat.tools",
            desc: "settings.cat.tools_desc",
            groups: vec![Group {
                label: "settings.group.programs",
                rows: vec![
                    row("editor", "settings.editor", Some("settings.editor_desc"), Control::Text(|s| s.tools.editor.clone(), |s, v| s.tools.editor = v)),
                    row("diff_tool", "settings.diff_tool", Some("settings.diff_tool_desc"), Control::Text(|s| s.tools.diff_tool.clone(), |s, v| s.tools.diff_tool = v)),
                    row("merge_tool", "settings.merge_tool", Some("settings.merge_tool_desc"), Control::Text(|s| s.tools.merge_tool.clone(), |s, v| s.tools.merge_tool = v)),
                ],
            }],
        },
        Category {
            id: "advanced",
            label: "settings.cat.advanced",
            desc: "settings.cat.advanced_desc",
            groups: vec![Group {
                label: "settings.group.diagnostics",
                rows: vec![row("log_git", "settings.log_git", Some("settings.log_git_desc"), Control::Toggle(|s| s.advanced.log_git_commands, |s, v| s.advanced.log_git_commands = v))],
            }],
        },
    ]
}

pub(crate) struct SettingsPanel {
    app: Entity<CairnApp>,
    original: Settings,
    draft: Settings,
    category: &'static str,
    search: Entity<InputState>,
    texts: HashMap<&'static str, Entity<InputState>>,
    areas: HashMap<&'static str, Entity<TextareaState>>,
    secrets: HashMap<&'static str, Entity<InputState>>,
    secret_status: HashMap<&'static str, SharedString>,
    saved: bool,
    _subs: Vec<Subscription>,
}

fn category_icon(id: &str) -> IconName {
    match id {
        "general" => IconName::Settings2,
        "appearance" => IconName::Palette,
        "graph" => IconName::GitCommitHorizontal,
        "diffs" => IconName::FileDiff,
        "commits" => IconName::GitCommitVertical,
        "git" => IconName::GitBranch,
        "remotes" => IconName::Cloud,
        "workspace" => IconName::Layers,
        "ai" => IconName::Sparkles,
        "checks" => IconName::ShieldCheck,
        "keyboard" => IconName::Keyboard,
        "notifications" => IconName::Bell,
        "performance" => IconName::Gauge,
        "updates" => IconName::Download,
        "tools" => IconName::SquareTerminal,
        _ => IconName::Wrench,
    }
}

thread_local! {
    static REQUESTED: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// Makes the next Settings window open at a given section instead of General.
///
/// # Arguments
/// * `category` - the section id, such as `ai`
pub(crate) fn request_category(category: &'static str) {
    REQUESTED.with(|r| r.set(Some(category)));
}

impl SettingsPanel {
    fn new(app: Entity<CairnApp>, settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(tr("settings.search")));
        let mut subs = vec![cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify())];
        let mut texts = HashMap::new();
        let mut areas = HashMap::new();
        let mut secrets = HashMap::new();
        for cat in schema() {
            for group in cat.groups {
                for r in group.rows {
                    match r.control {
                        Control::Text(get, set) => {
                            let input = cx.new(|cx| InputState::new(window, cx).default_value(get(&settings)));
                            subs.push(cx.subscribe(&input, move |this: &mut Self, input, ev: &InputEvent, cx| {
                                if matches!(ev, InputEvent::Change) {
                                    let v = input.read(cx).value().to_string();
                                    set(&mut this.draft, v);
                                    this.saved = false;
                                    cx.notify();
                                }
                            }));
                            texts.insert(r.id, input);
                        }
                        Control::LongText(get, set) => {
                            let current = get(&settings);
                            let area = cx.new(|cx| {
                                let mut s = TextareaState::new(window, cx).auto_grow(4, 10).placeholder(tr("settings.ai_instructions_placeholder"));
                                s.set_value(current, window, cx);
                                s
                            });
                            subs.push(cx.subscribe(&area, move |this: &mut Self, area, ev: &InputEvent, cx| {
                                if matches!(ev, InputEvent::Change) {
                                    let v = area.read(cx).value().to_string();
                                    set(&mut this.draft, v);
                                    this.saved = false;
                                    cx.notify();
                                }
                            }));
                            areas.insert(r.id, area);
                        }
                        Control::Secret(name) => {
                            let existing = crate::secrets::get(name).is_some();
                            let input = cx.new(|cx| {
                                InputState::new(window, cx)
                                    .masked(true)
                                    .placeholder(if existing { tr("settings.secret_saved") } else { tr("settings.secret_placeholder") })
                            });
                            secrets.insert(name, input);
                        }
                        _ => {}
                    }
                }
            }
        }
        Self {
            app,
            original: settings.clone(),
            draft: settings,
            category: REQUESTED.with(|r| r.take()).unwrap_or("general"),
            search,
            texts,
            areas,
            secrets,
            secret_status: HashMap::new(),
            saved: false,
            _subs: subs,
        }
    }

    fn changed_count(&self) -> usize {
        let defaults = Settings::default();
        let mut n = 0;
        for cat in schema() {
            for g in cat.groups {
                for r in g.rows {
                    if is_modified(&r, &self.draft, &defaults) {
                        n += 1;
                    }
                }
            }
        }
        n
    }

    fn render_control(&mut self, r: RowDef, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        match r.control {
            Control::Toggle(get, set) => {
                let v = get(&self.draft);
                Switch::new(r.id)
                    .checked(v)
                    .on_click(cx.listener(move |this, checked: &bool, window, cx| {
                        set(&mut this.draft, *checked);
                        this.after_change(window, cx);
                    }))
                    .into_any_element()
            }
            Control::Choice(options, get, set) if r.id == "theme" => {
                let current = get(&self.draft);
                let accent = theme.primary;
                let (border, fg, muted) = (theme.border, theme.foreground, theme.muted_foreground);
                let mut cards = h_flex().gap_3();
                for (i, key) in options.iter().enumerate() {
                    let on = i == current;
                    let (bg, panel, line): (Hsla, Hsla, Hsla) = match i {
                        0 => (rgb(0x0d0f12).into(), rgb(0x14171c).into(), rgb(0x262b33).into()),
                        1 => (rgb(0xf3f5f7).into(), rgb(0xffffff).into(), rgb(0xd8dde3).into()),
                        _ => (rgb(0x0d0f12).into(), rgb(0xffffff).into(), rgb(0x8b949e).into()),
                    };
                    let preview = h_flex()
                        .w(px(112.))
                        .h(px(64.))
                        .rounded(px(6.))
                        .overflow_hidden()
                        .bg(bg)
                        .child(v_flex().w(px(30.)).h_full().bg(panel).border_r_1().border_color(line).p_1().gap_1().children((0..3).map(move |_| div().h(px(4.)).rounded(px(2.)).bg(line))))
                        .child(v_flex().flex_1().p_1p5().gap_1().child(div().h(px(5.)).w(px(42.)).rounded(px(2.)).bg(accent)).children((0..3).map(move |k| div().h(px(4.)).w(px(64. - k as f32 * 12.)).rounded(px(2.)).bg(line))));
                    cards = cards.child(
                        v_flex()
                            .id((r.id, i))
                            .gap_1p5()
                            .p_1p5()
                            .rounded(px(10.))
                            .border_2()
                            .border_color(if on { accent } else { border })
                            .cursor_pointer()
                            .hover(move |s| s.border_color(accent.opacity(0.6)))
                            .child(preview)
                            .child(div().text_xs().text_center().font_weight(if on { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }).text_color(if on { fg } else { muted }).child(tr(key)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                set(&mut this.draft, i);
                                this.after_change(window, cx);
                            })),
                    );
                }
                cards.into_any_element()
            }
            Control::Choice(options, get, set) if r.id == "palette" => {
                let current = get(&self.draft);
                let dark = cx.theme().is_dark();
                let accent = crate::theme::accent_color(self.draft.appearance.accent, dark);
                let (fg, muted) = (theme.foreground, theme.muted_foreground);
                let mut grid = div().grid().grid_cols(3).gap_2();
                for (i, key) in options.iter().enumerate() {
                    let on = i == current;
                    let s = crate::theme::surface(Palette::ALL[i], dark);
                    let c = |v: u32| -> Hsla { rgb(v).into() };
                    let lanes: Vec<Hsla> = (0..3).map(|k| crate::widgets::lane_color(k, cx)).collect();
                    let preview = h_flex()
                        .h(px(58.))
                        .rounded(px(8.))
                        .overflow_hidden()
                        .bg(c(s.background))
                        .child(v_flex().w(px(26.)).h_full().bg(c(s.sidebar)).border_r_1().border_color(c(s.border)).p_1().gap_1().children((0..3).map(move |_| div().h(px(4.)).rounded(px(2.)).bg(c(s.border)))))
                        .child(
                            v_flex()
                                .flex_1()
                                .p_1p5()
                                .gap(px(5.))
                                .child(div().h(px(5.)).w(px(38.)).rounded(px(2.)).bg(accent))
                                .children(lanes.into_iter().enumerate().map(move |(k, lane)| h_flex().gap_1().items_center().child(div().size(px(5.)).rounded_full().bg(lane)).child(div().h(px(4.)).w(px(54. - k as f32 * 10.)).rounded(px(2.)).bg(c(s.muted_foreground).opacity(0.5))))),
                        );
                    grid = grid.child(
                        v_flex()
                            .id((r.id, i))
                            .gap_1p5()
                            .p_1p5()
                            .rounded(px(12.))
                            .border_2()
                            .border_color(if on { accent } else { theme.border })
                            .when(on, |d| d.shadow(crate::widgets::glow(accent, 12.)))
                            .cursor_pointer()
                            .hover(move |s| s.border_color(accent.opacity(0.6)))
                            .child(preview)
                            .child(div().text_xs().text_center().font_weight(if on { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }).text_color(if on { fg } else { muted }).child(tr(key)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                set(&mut this.draft, i);
                                this.after_change(window, cx);
                            })),
                    );
                }
                grid.into_any_element()
            }
            Control::Choice(options, get, set) if r.id == "accent" => {
                let current = get(&self.draft);
                let dark = cx.theme().is_dark();
                let swatches: [(u32, u32); 4] = [(0x4c9eff, 0x0a64c2), (0xb48efa, 0x7a46d6), (0x39c5cf, 0x0a7480), (0xe3a03f, 0x9a6200)];
                let fg = theme.foreground;
                let mut row = h_flex().gap_2();
                for (i, key) in options.iter().enumerate() {
                    let on = i == current;
                    let color: Hsla = rgb(if dark { swatches[i].0 } else { swatches[i].1 }).into();
                    row = row.child(
                        div()
                            .id((r.id, i))
                            .size(px(30.))
                            .rounded_full()
                            .p(px(3.))
                            .border_2()
                            .border_color(if on { fg } else { theme.transparent })
                            .cursor_pointer()
                            .tooltip({
                                let label = tr(key);
                                move |window, cx| gpui_kit::component::tooltip::Tooltip::new(label.clone()).build(window, cx)
                            })
                            .child(div().size_full().rounded_full().bg(color))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                set(&mut this.draft, i);
                                this.after_change(window, cx);
                            })),
                    );
                }
                row.into_any_element()
            }
            Control::Choice(options, get, set) => {
                let current = get(&self.draft);
                let mut group = h_flex().p(px(2.)).gap(px(2.)).rounded(px(8.)).border_1().border_color(theme.border).bg(theme.background);
                for (i, key) in options.iter().enumerate() {
                    let on = i == current;
                    let (hover, active, fg, muted) = (theme.list_hover, theme.secondary, theme.foreground, theme.muted_foreground);
                    group = group.child(
                        div()
                            .id((r.id, i))
                            .h(px(28.))
                            .px_3()
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .text_xs()
                            .cursor_pointer()
                            .when(on, |d| d.bg(active).text_color(fg).font_weight(FontWeight::SEMIBOLD).border_1().border_color(theme.border))
                            .when(!on, |d| d.text_color(muted).hover(move |s| s.bg(hover)))
                            .child(tr(key))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                set(&mut this.draft, i);
                                this.after_change(window, cx);
                            })),
                    );
                }
                group.into_any_element()
            }
            Control::Number(get, set, min, max, step, unit) => {
                let v = get(&self.draft);
                let fmt = if step < 1.0 { format!("{v:.1}") } else { format!("{v:.0}") };
                hstack()
                    .gap_1()
                    .child(
                        Button::new((r.id, 0usize))
                            .small()
                            .ghost()
                            .icon(icon(IconName::Minus))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let v = (get(&this.draft) - step).max(min);
                                set(&mut this.draft, v);
                                this.after_change(window, cx);
                            })),
                    )
                    .child(div().min_w(px(56.)).text_center().text_sm().font_family(theme.mono_font_family.clone()).child(fmt))
                    .child(
                        Button::new((r.id, 1usize))
                            .small()
                            .ghost()
                            .icon(icon(IconName::Plus))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let v = (get(&this.draft) + step).min(max);
                                set(&mut this.draft, v);
                                this.after_change(window, cx);
                            })),
                    )
                    .child(div().text_xs().text_color(theme.muted_foreground).child(tr(unit)))
                    .into_any_element()
            }
            Control::Text(_, _) => match self.texts.get(r.id) {
                Some(input) => div().w(px(300.)).child(Input::new(input).small()).into_any_element(),
                None => div().into_any_element(),
            },
            Control::LongText(_, _) => match self.areas.get(r.id) {
                Some(area) => div().w_full().child(Textarea::new(area)).into_any_element(),
                None => div().into_any_element(),
            },
            Control::TestAi => {
                let status = self.secret_status.get("ai_test").cloned();
                v_flex()
                    .items_end()
                    .gap_1()
                    .child(Button::new("ai-test").small().outline().icon(icon(IconName::Plug)).label(tr("ai.test")).on_click(cx.listener(|this, _, _, cx| {
                        let settings = this.draft.clone();
                        this.secret_status.insert("ai_test", tr("ai.testing"));
                        cx.notify();
                        cx.spawn(async move |this, cx| {
                            let result = cx.background_executor().spawn(async move { crate::ai::test_connection(&settings) }).await;
                            let _ = this.update(cx, |this, cx| {
                                let msg = match result {
                                    Ok(_) => tr("ai.test_ok"),
                                    Err(e) => SharedString::from(e),
                                };
                                this.secret_status.insert("ai_test", msg);
                                cx.notify();
                            });
                        })
                        .detach();
                    })))
                    .when_some(status, |d, s| d.child(div().max_w(px(320.)).text_xs().text_color(theme.muted_foreground).child(s)))
                    .into_any_element()
            }
            Control::CheckUpdates => {
                let status = self.secret_status.get("updates").cloned();
                let checking = status.as_ref().is_some_and(|s| *s == tr("update.checking"));
                v_flex()
                    .items_end()
                    .gap_1()
                    .child(
                        crate::widgets::framed_if(Button::new("check-updates"), crate::widgets::Tone::Plain, !checking, cx)
                            .icon(icon(IconName::RefreshCw))
                            .label(tr("settings.check_updates_button"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.secret_status.insert("updates", tr("update.checking"));
                                cx.notify();
                                let app = this.app.clone();
                                cx.spawn(async move |this, cx| {
                                    let result = cx.background_executor().spawn(async move { crate::updater::check_now() }).await;
                                    let _ = this.update(cx, |this, cx| {
                                        let msg: SharedString = match result {
                                            Ok(Some(info)) => {
                                                let version = info.TargetFullRelease.Version.clone();
                                                app.update(cx, |a, cx| {
                                                    a.update_ready = Some((version.clone(), info));
                                                    cx.notify();
                                                });
                                                t!("update.found", version = version).to_string().into()
                                            }
                                            Ok(None) => t!("update.up_to_date", version = crate::updater::current_version()).to_string().into(),
                                            Err(e) => e.into(),
                                        };
                                        this.secret_status.insert("updates", msg);
                                        cx.notify();
                                    });
                                })
                                .detach();
                            })),
                    )
                    .when_some(status, |d, s| d.child(div().max_w(px(320.)).text_xs().text_color(theme.muted_foreground).child(s)))
                    .into_any_element()
            }
            Control::Secret(name) => {
                let Some(input) = self.secrets.get(name).cloned() else { return div().into_any_element() };
                let status = self.secret_status.get(name).cloned();
                v_flex()
                    .gap_1()
                    .child(
                        hstack()
                            .gap_1()
                            .child(div().w(px(240.)).child(Input::new(&input).small()))
                            .child(Button::new((r.id, 10usize)).small().outline().label(tr("settings.secret_save")).on_click(cx.listener(move |this, _, window, cx| {
                                let value = input.read(cx).value().trim().to_string();
                                if value.is_empty() {
                                    return;
                                }
                                let msg = match crate::secrets::set(name, &value) {
                                    Ok(()) => tr("settings.secret_stored"),
                                    Err(e) => SharedString::from(e),
                                };
                                input.update(cx, |s, cx| s.set_value("", window, cx));
                                this.secret_status.insert(name, msg);
                                cx.notify();
                            })))
                            .child(Button::new((r.id, 11usize)).small().ghost().label(tr("settings.secret_remove")).on_click(cx.listener(move |this, _, _, cx| {
                                let _ = crate::secrets::delete(name);
                                this.secret_status.insert(name, tr("settings.secret_removed"));
                                cx.notify();
                            }))),
                    )
                    .when_some(status, |d, s| d.child(div().text_xs().text_color(cx.theme().muted_foreground).child(s)))
                    .into_any_element()
            }
        }
    }

    fn after_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.saved = false;
        let preview = self.draft.clone();
        crate::theme::apply(&preview, Some(window), cx);
        cx.notify();
    }

    fn render_row(&mut self, r: RowDef, last: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let defaults = Settings::default();
        let modified = is_modified(&r, &self.draft, &defaults);
        let control = self.render_control(r, cx);
        h_flex()
            .flex_wrap()
            .items_center()
            .gap_4()
            .py_3()
            .when(!last, |d| d.border_b_1())
            .border_color(theme.border.opacity(0.6))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(240.))
                    .gap_0p5()
                    .child(
                        hstack()
                            .gap_2()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(tr(r.label)))
                            .when(modified, |d| d.child(div().size(px(7.)).rounded_full().bg(theme.primary))),
                    )
                    .when_some(r.desc, |d, desc| d.child(div().text_xs().text_color(theme.muted_foreground).child(tr(desc)))),
            )
            .child(control)
            .when(modified, |d| {
                d.child(
                    Button::new((r.id, 99usize))
                        .xsmall()
                        .ghost()
                        .label(tr("settings.reset"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            reset_row(&r, &mut this.draft);
                            if let (Control::Text(get, _), Some(input)) = (r.control, this.texts.get(r.id)) {
                                let v = get(&this.draft);
                                input.update(cx, |s, cx| s.set_value(v, window, cx));
                            }
                            if let (Control::LongText(get, _), Some(area)) = (r.control, this.areas.get(r.id)) {
                                let v = get(&this.draft);
                                area.update(cx, |s, cx| s.set_value(v, window, cx));
                            }
                            this.after_change(window, cx);
                        })),
                )
            })
            .into_any_element()
    }
}

fn is_modified(r: &RowDef, a: &Settings, defaults: &Settings) -> bool {
    match r.control {
        Control::Toggle(get, _) => get(a) != get(defaults),
        Control::Choice(_, get, _) => get(a) != get(defaults),
        Control::Number(get, _, _, _, _, _) => (get(a) - get(defaults)).abs() > f64::EPSILON,
        Control::Text(get, _) | Control::LongText(get, _) => get(a) != get(defaults),
        Control::Secret(_) | Control::TestAi | Control::CheckUpdates => false,
    }
}

fn reset_row(r: &RowDef, s: &mut Settings) {
    let d = Settings::default();
    match r.control {
        Control::Toggle(get, set) => set(s, get(&d)),
        Control::Choice(_, get, set) => set(s, get(&d)),
        Control::Number(get, set, ..) => set(s, get(&d)),
        Control::Text(get, set) | Control::LongText(get, set) => set(s, get(&d)),
        Control::Secret(_) | Control::TestAi | Control::CheckUpdates => {}
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let query = self.search.read(cx).value().trim().to_lowercase();
        let cats = schema();
        let draft = self.draft.clone();
        let visible = |r: &RowDef| r.visible.is_none_or(|f| f(&draft));
        let defaults = Settings::default();

        let mut nav = v_flex().gap_0p5().p_2();
        for c in &cats {
            let on = query.is_empty() && c.id == self.category;
            let changes: usize = c.groups.iter().flat_map(|g| g.rows.iter()).filter(|r| is_modified(r, &self.draft, &defaults)).count();
            let id = c.id;
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("set-cat-{id}")))
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .h(px(34.))
                    .px_2p5()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_sm()
                    .when(on, |d| d.bg(theme.list_active).text_color(theme.foreground).font_weight(FontWeight::SEMIBOLD))
                    .when(!on, |d| d.text_color(theme.muted_foreground).hover(|s| s.bg(theme.list_hover)))
                    .child(icon(category_icon(id)).small().text_color(if on { theme.primary } else { theme.muted_foreground }))
                    .child(div().flex_1().child(tr(c.label)))
                    .when(changes > 0, |d| d.child(crate::widgets::count_chip(changes.to_string(), theme.primary.opacity(0.15), theme.primary)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.category = id;
                        this.search.update(cx, |s, cx| s.set_value("", window, cx));
                        cx.notify();
                    })),
            );
        }

        let mut content = v_flex().gap_1();
        if query.is_empty() {
            let cat = cats.iter().find(|c| c.id == self.category).unwrap_or(&cats[0]);
            content = content.child(
                hstack()
                    .gap_3()
                    .pb_1()
                    .child(
                        div()
                            .flex_none()
                            .size(px(40.))
                            .rounded(px(10.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(theme.primary.opacity(0.14))
                            .child(icon(category_icon(cat.id)).text_color(theme.primary)),
                    )
                    .child(
                        v_flex()
                            .gap(px(2.))
                            .child(div().text_lg().font_weight(FontWeight::BOLD).child(tr(cat.label)))
                            .child(div().text_sm().text_color(theme.muted_foreground).child(tr(cat.desc))),
                    ),
            );
            if matches!(cat.id, "appearance" | "graph" | "diffs") {
                content = content.child(render_preview(&self.draft, cx));
            }
            for g in &cat.groups {
                let rows: Vec<RowDef> = g.rows.iter().copied().filter(|r| visible(r)).collect();
                if rows.is_empty() {
                    continue;
                }
                let mut card = v_flex().px_4().rounded(px(10.)).border_1().border_color(theme.border).bg(theme.sidebar);
                let n = rows.len();
                for (i, r) in rows.into_iter().enumerate() {
                    card = card.child(self.render_row(r, i + 1 == n, cx));
                }
                content = content.child(crate::widgets::section_label(tr(g.label), cx).px_0().pt_5()).child(card);
            }
        } else {
            let mut found = 0;
            for c in &cats {
                let rows: Vec<RowDef> = c
                    .groups
                    .iter()
                    .flat_map(|g| g.rows.iter().copied())
                    .filter(|r| visible(r))
                    .filter(|r| {
                        let text = format!("{} {} {}", tr(r.label), r.desc.map(tr).unwrap_or_default(), tr(c.label)).to_lowercase();
                        text.contains(&query)
                    })
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                found += rows.len();
                content = content.child(crate::widgets::section_label(tr(c.label), cx).px_0());
                for r in rows {
                    content = content.child(self.render_row(r, false, cx));
                }
            }
            if found == 0 {
                content = content.child(div().py_6().text_sm().text_color(theme.muted_foreground).child(t!("settings.no_results", q = query.clone()).to_string()));
            }
        }

        let changed = self.changed_count();
        let status: SharedString = if self.saved {
            t!("settings.saved_to", path = Settings::file().display().to_string()).to_string().into()
        } else if changed > 0 {
            t!("settings.changed_count", n = changed).to_string().into()
        } else {
            tr("settings.all_default")
        };

        let has_repo = self.app.read(cx).active_tab().is_some();
        let app = self.app.clone();
        let nav = nav.when(has_repo, |n| {
            n.child(div().h(px(1.)).mx_1p5().my_2().bg(theme.border)).child(
                div()
                    .id("set-repo-settings")
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .h(px(34.))
                    .px_2p5()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.bg(theme.list_hover))
                    .child(icon(IconName::FolderGit2).small())
                    .child(tr("palette.repo_settings"))
                    .on_click(move |_, window, cx| {
                        if let Some(tab) = app.read(cx).active_tab() {
                            window.close_dialog(cx);
                            crate::repo_settings::open(tab, window, cx);
                        }
                    }),
            )
        });
        let total: usize = cats.iter().map(|c| c.groups.iter().map(|g| g.rows.len()).sum::<usize>()).sum();
        v_flex()
            .w_full()
            .h(px(660.))
            .child(
                hstack()
                    .gap_3()
                    .pb_3()
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child(tr("settings.title")))
                    .child(div().w(px(360.)).child(Input::new(&self.search).prefix(icon(IconName::Search).small())))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(t!("settings.count", n = total).to_string())),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme.border)
                    .overflow_hidden()
                    .child(div().id("settings-nav").flex_none().w(px(230.)).h_full().overflow_y_scroll().bg(theme.sidebar).border_r_1().border_color(theme.border).child(nav))
                    .child(div().id("settings-content").flex_1().min_w_0().h_full().overflow_y_scroll().px_6().py_5().bg(theme.background).child(content)),
            )
            .child(
                hstack()
                    .pt_3()
                    .gap_2()
                    .child(crate::updater::about_line(self.draft.appearance.reduce_motion, cx))
                    .child(div().text_xs().text_color(if self.saved { theme.green } else { theme.muted_foreground }).child(status))
                    .child(spacer())
                    .child(
                        crate::widgets::framed(Button::new("settings-reset-all"), crate::widgets::Tone::Plain, cx)
                            .text_color(theme.muted_foreground)
                            .label(tr("settings.reset_all"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.draft = Settings::default();
                                for area in this.areas.values().cloned().collect::<Vec<_>>() {
                                    area.update(cx, |s, cx| s.set_value("", window, cx));
                                }
                                for (id, input) in this.texts.clone() {
                                    for cat in schema() {
                                        for g in cat.groups {
                                            for r in g.rows {
                                                if r.id == id
                                                    && let Control::Text(get, _) = r.control
                                                {
                                                    let v = get(&this.draft);
                                                    input.update(cx, |s, cx| s.set_value(v, window, cx));
                                                }
                                            }
                                        }
                                    }
                                }
                                this.after_change(window, cx);
                            })),
                    )
                    .child(
                        crate::widgets::framed(Button::new("settings-cancel"), crate::widgets::Tone::Plain, cx)
                            .label(tr("dialog.cancel"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let original = this.original.clone();
                                crate::theme::apply(&original, Some(window), cx);
                                window.close_dialog(cx);
                            })),
                    )
                    .child(
                        crate::widgets::framed(Button::new("settings-save"), crate::widgets::Tone::Accent, cx)
                            .px(px(18.))
                            .label(tr("dialog.save"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let draft = this.draft.clone();
                                this.original = draft.clone();
                                this.saved = true;
                                this.app.update(cx, |app, cx| app.apply_settings(draft, window, cx));
                                cx.notify();
                            })),
                    ),
            )
    }
}

/// A small live sample of History and a diff, drawn with the settings being edited, so every
/// change to colors, fonts, avatars or diff options shows at once.
///
/// # Arguments
/// * `s` - the settings being edited
///
/// # Returns
/// The preview card.
fn render_preview(s: &Settings, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let lanes = [crate::widgets::lane_color(0, cx), crate::widgets::lane_color(1, cx), crate::widgets::lane_color(2, cx)];
    let rows: [(usize, &str, &str, &str, Option<&str>); 4] = [
        (1, "feat(ui): city search box", "Alex Morgan", "2h", Some("feat/search")),
        (0, "fix(api): drop stale cache entries", "Kai Tanaka", "5h", Some("main")),
        (2, "docs: explain the forecast cache", "Noor Haddad", "1d", None),
        (0, "chore: scaffold project", "Sam Rivera", "3d", None),
    ];
    let mut history = v_flex().rounded(px(10.)).border_1().border_color(theme.border).overflow_hidden().bg(theme.background);
    for (i, (lane, msg, who, when, chip)) in rows.into_iter().enumerate() {
        let color = lanes[lane];
        let mut graph = h_flex().relative().w(px(56.)).h_full().pl_2();
        for (l, c) in lanes.iter().enumerate() {
            graph = graph.child(
                div().relative().w(px(14.)).h_full().child(div().absolute().left(px(6.)).top_0().bottom_0().w(px(2.)).bg(c.opacity(if l == lane { 0.9 } else { 0.35 }))).when(l == lane, |d| d.child(div().absolute().left(px(2.)).top(px(11.)).size(px(10.)).rounded_full().border_2().border_color(*c).bg(theme.background))),
            );
        }
        history = history.child(
            h_flex()
                .h(px(32.))
                .gap_2()
                .pr_3()
                .when(i > 0, |d| d.border_t_1().border_color(theme.border.opacity(0.5)))
                .when(i == 0, |d| d.bg(theme.primary.opacity(0.12)))
                .child(graph)
                .when_some(chip, |d, c| d.child(div().px(px(6.)).h(px(18.)).flex().items_center().rounded(px(4.)).text_xs().font_weight(FontWeight::SEMIBOLD).text_color(color).bg(color.opacity(0.14)).border_1().border_color(color.opacity(0.45)).child(c.to_string())))
                .child(div().flex_1().min_w_0().truncate().text_sm().child(msg.to_string()))
                .when(s.appearance.show_avatars, |d| d.child(crate::widgets::avatar(who, px(20.), cx)))
                .child(div().w(px(84.)).truncate().text_xs().text_color(theme.muted_foreground).child(who.to_string()))
                .child(div().w(px(24.)).text_xs().text_color(theme.muted_foreground).child(when.to_string())),
        );
    }
    let mono = theme.mono_font_family.clone();
    let code = |sign: &'static str, text: &'static str, bg: Hsla, fg: Hsla, n: &'static str| {
        h_flex().h(px(22.)).px_2().gap_3().bg(bg).font_family(mono.clone()).text_size(theme.mono_font_size).child(div().w(px(22.)).text_color(theme.muted_foreground).when(s.diffs.line_numbers, |d| d.child(n))).child(div().w(px(10.)).text_color(fg).child(sign)).child(div().text_color(theme.foreground).child(text))
    };
    let diff = v_flex()
        .rounded(px(10.))
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .bg(theme.background)
        .child(code(" ", "fn forecast(city: &str) -> Report {", theme.transparent, theme.muted_foreground, "12"))
        .child(code("-", "    let cache = Cache::new();", theme.red.opacity(0.12), theme.red, "13"))
        .child(code("+", "    let cache = Cache::with_ttl(10 * 60);", theme.green.opacity(0.12), theme.green, "13"))
        .child(code(" ", "    cache.get_or_fetch(city)", theme.transparent, theme.muted_foreground, "14"));
    v_flex()
        .gap_2()
        .mt_3()
        .p_3()
        .rounded(px(12.))
        .border_1()
        .border_color(theme.primary.opacity(0.35))
        .bg(linear_gradient(180., linear_color_stop(theme.primary.opacity(0.08), 0.), linear_color_stop(theme.sidebar, 0.6)))
        .child(hstack().gap_1p5().child(icon(IconName::Eye).small().text_color(theme.primary)).child(div().text_xs().font_weight(FontWeight::BOLD).text_color(theme.muted_foreground).child(tr("settings.preview").to_uppercase())).child(spacer()).child(div().text_xs().text_color(theme.muted_foreground).child(tr("settings.preview_hint"))))
        .child(history)
        .child(diff)
        .into_any_element()
}

pub(crate) fn open(app: Entity<CairnApp>, settings: Settings, window: &mut Window, cx: &mut App) {
    let panel = cx.new(|cx| SettingsPanel::new(app, settings, window, cx));
    window.open_dialog(cx, move |dialog, _, cx| dialog.w(px(1040.)).bg(cx.theme().popover.opacity(1.)).close_button(true).child(panel.clone()));
}
