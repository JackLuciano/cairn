use std::io::Write;
use std::process::{Command, Stdio};

use cairn_core::conflicts::ConflictRegion;
use cairn_core::settings::{AiProvider, Language, Settings};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const API_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tier {
    Deep,
    Fast,
}

fn model_for(settings: &Settings, tier: Tier) -> String {
    match tier {
        Tier::Deep => settings.ai.model_deep.clone(),
        Tier::Fast => settings.ai.model_fast.clone(),
    }
}

fn price_per_mtok(model: &str) -> (f64, f64) {
    if model.contains("opus") {
        (4.0, 20.0)
    } else if model.contains("sonnet") {
        (2.0, 10.0)
    } else {
        (0.10, 0.50)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct UsageFile {
    month: String,
    usd: f64,
    input_tokens: u64,
    output_tokens: u64,
}

fn usage_path() -> std::path::PathBuf {
    cairn_core::paths::data_dir().join("ai-usage.json")
}

fn load_usage() -> UsageFile {
    let month = chrono::Local::now().format("%Y-%m").to_string();
    let u: UsageFile = std::fs::read(usage_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if u.month == month { u } else { UsageFile { month, ..Default::default() } }
}

fn record_usage(model: &str, input: u64, output: u64) {
    let mut u = load_usage();
    let (pi, po) = price_per_mtok(model);
    u.usd += input as f64 / 1e6 * pi + output as f64 / 1e6 * po;
    u.input_tokens += input;
    u.output_tokens += output;
    if let Ok(bytes) = serde_json::to_vec_pretty(&u) {
        let _ = cairn_core::paths::write_atomic(&usage_path(), &bytes);
    }
}

pub(crate) fn month_spend() -> f64 {
    load_usage().usd
}

fn language_line(settings: &Settings) -> &'static str {
    match settings.general.language {
        Language::Hungarian => "Answer in Hungarian, using the informal \"te\" form. Keep code, file names and commit message prefixes as they are.",
        Language::English => "Answer in English.",
    }
}

fn glob_to_regex(glob: &str) -> Option<regex::Regex> {
    let mut re = String::from("(?i)^");
    let g = glob.trim();
    let mut chars = g.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                }
                re.push_str("(.*/)?");
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\' => {
                re.push('\\');
                re.push(c);
            }
            _ => re.push(c),
        }
    }
    re.push('$');
    regex::Regex::new(&re).ok()
}

pub(crate) fn sanitize_diff(settings: &Settings, diff: &str) -> String {
    let excluded: Vec<regex::Regex> = settings.ai.excluded_paths.split(',').filter(|s| !s.trim().is_empty()).filter_map(glob_to_regex).collect();
    let mut out = String::new();
    let mut skipping = false;
    for chunk in diff.split_inclusive('\n') {
        if let Some(rest) = chunk.strip_prefix("diff --git ") {
            let path = rest.trim().rsplit_once(" b/").map(|(_, b)| b.to_string()).unwrap_or_default();
            skipping = excluded.iter().any(|r| r.is_match(&path));
            if skipping {
                out.push_str(&format!("[diff of {path} omitted by the user's privacy settings]\n"));
            }
        }
        if !skipping {
            out.push_str(chunk);
        }
    }
    if settings.ai.redact_secrets {
        out = cairn_core::checks::redact(&out);
    }
    const LIMIT: usize = 400_000;
    if out.len() > LIMIT {
        let mut cut = LIMIT;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push_str("\n[diff truncated: it is longer than Cairn sends to the model]\n");
    }
    out
}

fn call_api(settings: &Settings, tier: Tier, system: &str, user: &str, max_tokens: u32) -> Result<String, String> {
    let (base, key, official) = match settings.ai.provider {
        AiProvider::ApiKey => (API_URL.to_string(), crate::secrets::get("anthropic").ok_or("Add your Anthropic API key in Settings → AI.")?, true),
        AiProvider::CustomEndpoint => (settings.ai.endpoint_url.trim_end_matches('/').to_string(), crate::secrets::get("custom").unwrap_or_default(), false),
        AiProvider::ClaudeCode => unreachable!(),
    };
    let limit = f64::from(settings.ai.monthly_limit_usd);
    if limit > 0.0 && month_spend() >= limit {
        return Err(format!("The monthly AI limit of ${limit:.0} is reached. Raise it in Settings → AI."));
    }
    let model = model_for(settings, tier);
    let mut body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system,
        "messages": [{ "role": "user", "content": user }],
    });
    let with_fallback = official && (model.contains("opus-5") || model.contains("sonnet-5-5") || model.contains("fable"));
    if with_fallback {
        body["fallbacks"] = json!("default");
    }
    if tier == Tier::Fast {
        body["output_config"] = json!({ "effort": "low" });
    } else {
        body["output_config"] = json!({ "effort": "medium" });
    }
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(600)))
        .build()
        .new_agent();
    let mut req = agent
        .post(format!("{base}/v1/messages"))
        .header("content-type", "application/json")
        .header("anthropic-version", API_VERSION);
    if !key.is_empty() {
        req = req.header("x-api-key", &key);
    }
    if with_fallback {
        req = req.header("anthropic-beta", FALLBACK_BETA);
    }
    let mut resp = req.send_json(&body).map_err(|e| format!("Could not reach the AI service: {e}"))?;
    let status = resp.status().as_u16();
    let value: Value = resp.body_mut().read_json().map_err(|e| format!("Unexpected AI response: {e}"))?;
    if status >= 400 {
        let msg = value.pointer("/error/message").and_then(|v| v.as_str()).unwrap_or("request failed");
        return Err(match status {
            401 => "The API key was rejected. Check it in Settings → AI.".into(),
            429 => "The AI service is busy or your rate limit is reached. Try again in a minute.".into(),
            _ => format!("AI request failed ({status}): {msg}"),
        });
    }
    let used_model = value.get("model").and_then(|v| v.as_str()).unwrap_or(&model).to_string();
    let input = value.pointer("/usage/input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output = value.pointer("/usage/output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    record_usage(&used_model, input, output);
    if value.get("stop_reason").and_then(|v| v.as_str()) == Some("refusal") {
        return Err("The model declined this request.".into());
    }
    let text: String = value
        .get("content")
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    if text.trim().is_empty() {
        return Err("The AI returned an empty answer.".into());
    }
    if settings.ai.show_usage {
        crate::repo_view::log_error(&format!("AI usage: {used_model} in={input} out={output}"));
    }
    Ok(text)
}

fn call_claude_code(settings: &Settings, tier: Tier, system: &str, user: &str) -> Result<String, String> {
    let mut cmd = Command::new("claude");
    cmd.args(["-p", "--output-format", "json", "--model", &model_for(settings, tier), "--system-prompt", system])
        .args(["--disallowed-tools", "Bash", "Edit", "Write", "NotebookEdit", "WebFetch", "WebSearch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|_| "Claude Code is not installed. Install it, sign in with `claude`, or switch to an API key in Settings → AI.".to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(user.as_bytes()).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: Value = serde_json::from_str(stdout.trim()).map_err(|_| {
        let err = String::from_utf8_lossy(&out.stderr);
        if err.to_lowercase().contains("login") || err.to_lowercase().contains("auth") {
            "Claude Code is not signed in. Run `claude` once in a terminal and sign in.".to_string()
        } else {
            format!("Claude Code returned an unexpected answer: {}", err.lines().next().unwrap_or_default())
        }
    })?;
    if value.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err(value.get("result").and_then(|v| v.as_str()).unwrap_or("Claude Code reported an error.").to_string());
    }
    value.get("result").and_then(|v| v.as_str()).map(str::to_string).ok_or_else(|| "Claude Code returned no answer.".to_string())
}

pub(crate) fn complete(settings: &Settings, tier: Tier, system: &str, user: &str) -> Result<String, String> {
    if !settings.ai.enabled {
        return Err("AI features are turned off in Settings → AI.".into());
    }
    let system = format!("{system}\n\n{}", language_line(settings));
    match settings.ai.provider {
        AiProvider::ClaudeCode => call_claude_code(settings, tier, &system, user),
        _ => call_api(settings, tier, &system, user, if tier == Tier::Deep { 16000 } else { 4000 }),
    }
}

pub(crate) fn test_connection(settings: &Settings) -> Result<String, String> {
    complete(settings, Tier::Fast, "You are a connection test.", "Reply with the single word: ok")
}

fn strip_fences(text: &str) -> String {
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim().to_string()
}

fn extract_json(text: &str, open: char, close: char) -> Option<String> {
    let t = strip_fences(text);
    let start = t.find(open)?;
    let end = t.rfind(close)?;
    (end > start).then(|| t[start..=end].to_string())
}

pub(crate) fn suggest_conflict(settings: &Settings, path: &str, before: &[String], region: &ConflictRegion, after: &[String]) -> Result<Vec<String>, String> {
    let system = "You resolve git merge conflicts. Combine both sides so neither side's intent is lost. Reply with only the resolved lines that replace the conflict block, no explanation, no code fences.";
    let user = format!(
        "File: {path}\n\nLines before the conflict:\n{}\n\nOurs ({}):\n{}\n\nCommon ancestor:\n{}\n\nTheirs ({}):\n{}\n\nLines after the conflict:\n{}\n",
        before.join("\n"),
        region.ours_label,
        region.ours.join("\n"),
        if region.base.is_empty() { "(not available)".to_string() } else { region.base.join("\n") },
        region.theirs_label,
        region.theirs.join("\n"),
        after.join("\n")
    );
    let text = complete(settings, Tier::Deep, system, &user)?;
    Ok(strip_fences(&text).lines().map(str::to_string).collect())
}

pub(crate) fn commit_message(settings: &Settings, staged_diff: &str) -> Result<(String, String), String> {
    let style = if settings.commits.conventional {
        "Use Conventional Commits: type(scope): summary, where scope is the main folder or module touched."
    } else {
        "Write a plain, imperative summary."
    };
    let system = format!(
        "You write git commit messages. {style} The summary must be at most {} characters. After a blank line, add a short body only when the change needs explaining. Reply with only the commit message.",
        settings.commits.summary_limit
    );
    let text = complete(settings, Tier::Fast, &system, &sanitize_diff(settings, staged_diff))?;
    let text = strip_fences(&text);
    let (summary, body) = text.split_once("\n\n").map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).unwrap_or((text.trim().to_string(), String::new()));
    Ok((summary, body))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Question {
    ExplainCommit,
    WhatCouldBreak,
    SummarizeToday,
}

pub(crate) fn explain(settings: &Settings, question: Question, context: &str) -> Result<String, String> {
    let system = match question {
        Question::ExplainCommit => "You explain git commits to a developer in plain language. Start with one sentence on what changed and why it matters, then up to five short bullet points. No headings.",
        Question::WhatCouldBreak => "You review a branch for risk. Say in one sentence what is most likely to break, then up to five short bullet points with concrete scenarios. No headings.",
        Question::SummarizeToday => "You summarize a developer's recent commits for a standup. One sentence overview, then up to six bullet points grouped by topic. No headings.",
    };
    complete(settings, Tier::Fast, system, &sanitize_diff(settings, context))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Finding {
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub problem: String,
    #[serde(default)]
    pub old: Option<String>,
    #[serde(default)]
    pub fix: Option<String>,
}

pub(crate) fn review(settings: &Settings, diff: &str) -> Result<Vec<Finding>, String> {
    let system = "You review code changes before they are pushed. Report only real problems: bugs, crashes, security issues, data loss, and clear inconsistencies with the surrounding code. Do not report style nits. Reply with only a JSON array; each item has: severity (\"bug\", \"security\", \"consistency\" or \"minor\"), file, line (number in the new file or null), problem (one or two sentences), old (the exact current line, or null) and fix (the replacement line, or null). Reply with [] when there is nothing worth reporting.";
    let text = complete(settings, Tier::Deep, system, &sanitize_diff(settings, diff))?;
    let json = extract_json(&text, '[', ']').ok_or("The review did not come back in the expected format.")?;
    serde_json::from_str::<Vec<Finding>>(&json).map_err(|e| format!("Could not read the review: {e}"))
}

pub(crate) fn pr_description(settings: &Settings, commits: &str, diff: &str) -> Result<(String, String), String> {
    let system = "You write pull request descriptions in Markdown. First line: a short title. Then a blank line, then: a one-paragraph summary, a \"## Changes\" bullet list, and a \"## Testing\" section containing only the placeholder [Describe how you tested this]. Do not invent test results.";
    let user = format!("Commits:\n{commits}\n\nDiff:\n{}", sanitize_diff(settings, diff));
    let text = strip_fences(&complete(settings, Tier::Deep, system, &user)?);
    let (title, body) = text.split_once('\n').map(|(a, b)| (a.trim_start_matches('#').trim().to_string(), b.trim().to_string())).unwrap_or((text.clone(), String::new()));
    Ok((title, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_paths_are_removed() {
        let mut s = Settings::default();
        s.ai.excluded_paths = "**/.env*, **/secrets/**".into();
        let diff = "diff --git a/app/.env.local b/app/.env.local\n+KEY=1\ndiff --git a/src/a.rs b/src/a.rs\n+fn a() {}\n";
        let out = sanitize_diff(&s, diff);
        assert!(!out.contains("KEY=1"));
        assert!(out.contains("fn a()"));
    }

    #[test]
    fn extracts_json_array() {
        assert_eq!(extract_json("Here:\n```json\n[{\"a\":1}]\n```", '[', ']').as_deref(), Some("[{\"a\":1}]"));
    }
}
