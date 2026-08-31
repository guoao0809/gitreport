//! Git 仓库检测与提交记录拉取（Tauri command）。
//! 前后端共享类型也定义在此模块，供 ai.rs 复用。

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

/// 创建 git 子进程命令。Windows 下加 CREATE_NO_WINDOW，避免每次执行 git 弹一个 cmd 窗口。
#[cfg(windows)]
pub(crate) fn git_cmd() -> Command {
    use std::os::windows::process::CommandExt;
    let mut cmd = Command::new("git");
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    cmd
}

#[cfg(not(windows))]
pub(crate) fn git_cmd() -> Command {
    Command::new("git")
}

// ===== 与前端 src/types.ts 对齐的共享类型（serde camelCase）=====

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AIConfig {
    pub protocol: String, // "openai" | "anthropic"
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AIIdentity {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectResult {
    pub name: String,
    pub path: String,
    pub branch: String,
    pub last_commit_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitItem {
    pub hash: String,
    pub author_name: String,
    pub author_email: String,
    pub time: String,
    pub message: String,
    pub project_id: String,
    pub project_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCommits {
    pub project_id: String,
    pub project_name: String,
    pub commits: Vec<CommitItem>,
}

/// fetch_commits 入参：仓库路径 + 要统计的分支（空串 = 当前 HEAD）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRef {
    pub path: String,
    pub branch: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratePayload {
    pub config: AIConfig,
    pub system: String,
    pub user: String,
}

// ===== 工具函数 =====

/// 检测系统是否安装了可用的 git
#[tauri::command]
pub fn check_git() -> bool {
    git_cmd()
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 运行 git 命令，返回 stdout 字符串（失败返回中文错误）
fn git_output(args: &[&str]) -> Result<String, String> {
    let out = git_cmd()
        .args(args)
        .output()
        .map_err(|e| format!("无法启动 git：{e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git 命令失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 判断目录是否为 git 仓库（存在 .git 目录或文件）
fn is_git_repo(p: &Path) -> bool {
    p.join(".git").exists()
}

/// 取路径最后一段作为项目名
fn dir_name(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string())
}

/// 判断提交作者是否匹配身份列表（name 或 email 任一命中即保留；空列表不过滤）
fn author_matches(name: &str, email: &str, authors: &[AIIdentity]) -> bool {
    if authors.is_empty() {
        return true;
    }
    authors
        .iter()
        .any(|a| (!a.name.is_empty() && a.name == name) || (!a.email.is_empty() && a.email == email))
}

// ===== command：读取 git config 中的用户身份 =====

/// 读取指定仓库的 `git config user.name` / `user.email`。
/// 优先仓库 local 配置（`-C path`），为空则回退到全局（`--global`），再回退系统级。
#[tauri::command]
pub fn get_git_identity(path: String) -> Result<AIIdentity, String> {
    let name = read_git_config(&path, "user.name");
    let email = read_git_config(&path, "user.email");
    if name.is_empty() && email.is_empty() {
        return Err("未在 git config 中找到 user.name / user.email，请先执行 git config 设置".to_string());
    }
    Ok(AIIdentity { name, email })
}

fn read_git_config(path: &str, key: &str) -> String {
    // 仓库级（继承 global，取该仓库实际生效值）
    if !path.is_empty() {
        if let Ok(s) = git_output(&["-C", path, "config", "--get", key]) {
            let s = s.trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    // 全局
    if let Ok(s) = git_output(&["config", "--global", "--get", key]) {
        let s = s.trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    // 系统级
    git_output(&["config", "--system", "--get", key])
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

// ===== command：扫描 git 仓库（限深 2 层）=====

#[tauri::command]
pub fn detect_git_repos(root_dir: String) -> Result<Vec<DetectResult>, String> {
    let root = Path::new(&root_dir);
    if !root.is_dir() {
        return Err(format!("目录不存在：{root_dir}"));
    }

    let mut results = Vec::new();
    for entry in walkdir::WalkDir::new(&root_dir)
        .max_depth(2)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_dir() || !is_git_repo(entry.path()) {
            continue;
        }
        let path_str = entry.path().to_string_lossy().to_string();
        // 默认分支（失败给空串，不中断扫描）
        let branch = git_output(&["-C", &path_str, "rev-parse", "--abbrev-ref", "HEAD"])
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        // 最近提交时间戳（%ct 为秒，×1000 转毫秒以对齐前端 Date.now()；失败给 None）
        let last_commit_at = git_output(&["-C", &path_str, "log", "-1", "--format=%ct"])
            .ok()
            .and_then(|s| s.trim().parse::<i64>().ok())
            .map(|secs| secs * 1000);
        results.push(DetectResult {
            name: dir_name(&path_str),
            path: path_str,
            branch,
            last_commit_at,
        });
    }
    Ok(results)
}

// ===== command：列出仓库分支 =====

/// 列出仓库所有本地分支（`git branch --format=%(refname:short)`），
/// 当前 HEAD 分支排最前（若属于本地分支）。
#[tauri::command]
pub fn get_git_branches(path: String) -> Result<Vec<String>, String> {
    let out = git_output(&["-C", &path, "branch", "--format=%(refname:short)"])?;
    let mut branches: Vec<String> = out
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    // 当前 HEAD 分支置顶
    if let Ok(head) = git_output(&["-C", &path, "rev-parse", "--abbrev-ref", "HEAD"]) {
        let head = head.trim().to_string();
        if let Some(pos) = branches.iter().position(|b| *b == head) {
            let h = branches.remove(pos);
            branches.insert(0, h);
        }
    }
    Ok(branches)
}

// ===== command：读取当前 HEAD 分支 =====

/// 读取仓库当前 HEAD 分支名（用于前台时同步外部切换的分支）
#[tauri::command]
pub fn get_git_current_branch(path: String) -> Result<String, String> {
    let b = git_output(&["-C", &path, "rev-parse", "--abbrev-ref", "HEAD"])?;
    Ok(b.trim().to_string())
}

// ===== command：拉取提交记录 =====

#[tauri::command]
pub fn fetch_commits(
    repos: Vec<RepoRef>,
    authors: Vec<AIIdentity>,
    from: String,
    to: String,
) -> Result<Vec<ProjectCommits>, String> {
    // 校验日期格式（yyyy-MM-dd），until 用 to 的次日以包含 to 全天
    chrono::NaiveDate::parse_from_str(&from, "%Y-%m-%d")
        .map_err(|e| format!("开始日期格式错误（应为 yyyy-MM-dd）：{e}"))?;
    let to_date = chrono::NaiveDate::parse_from_str(&to, "%Y-%m-%d")
        .map_err(|e| format!("结束日期格式错误（应为 yyyy-MM-dd）：{e}"))?;
    let until_next = (to_date + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    let mut results = Vec::new();
    for repo in &repos {
        let project_id = repo.path.clone();
        let project_name = dir_name(&repo.path);
        // 单个仓库失败不中断整体：返回空 commits 并记日志
        let commits = match log_commits(&repo.path, &repo.branch, &from, &until_next) {
            Ok(list) => list,
            Err(e) => {
                log::warn!("仓库 {project_name} 拉取提交失败：{e}");
                Vec::new()
            }
        };
        // 作者过滤 + 填充项目字段
        let commits: Vec<CommitItem> = commits
            .into_iter()
            .filter(|c| author_matches(&c.author_name, &c.author_email, &authors))
            .map(|c| CommitItem {
                hash: short_hash(&c.hash),
                author_name: c.author_name,
                author_email: c.author_email,
                time: c.time,
                message: c.message,
                project_id: project_id.clone(),
                project_name: project_name.clone(),
            })
            .collect();
        results.push(ProjectCommits {
            project_id,
            project_name,
            commits,
        });
    }
    Ok(results)
}

/// 短 hash 取前 7 位
fn short_hash(h: &str) -> String {
    h.chars().take(7).collect()
}

/// 解析 git log 后的单条提交
struct RawCommit {
    hash: String,
    author_name: String,
    author_email: String,
    time: String,
    message: String,
}

/// 执行 git log 并解析。branch 为空串时查当前 HEAD，否则查指定分支。
/// 用 %x1e（record separator）作提交边界：即使 message 多行或恰好像提交首行，也不会误判边界。
fn log_commits(path: &str, branch: &str, from: &str, until: &str) -> Result<Vec<RawCommit>, String> {
    // git 对纯日期 "YYYY-MM-DD" 解析不可靠（返回空），必须补 " 00:00:00"
    let since_arg = format!("--since={from} 00:00:00");
    let until_arg = format!("--until={until} 00:00:00");
    let mut args: Vec<String> = vec![
        "-C".into(),
        path.into(),
        "log".into(),
        since_arg,
        until_arg,
        "--pretty=format:%H%x1f%an%x1f%ae%x1f%aI%x1f%B%x1e".into(),
        "--name-only".into(),
    ];
    if !branch.is_empty() {
        // 插到 "log" 之后（index 3），生成 git log <branch> --since=...
        args.insert(3, branch.into());
    }
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let out = git_output(&refs)?;
    Ok(parse_log_output(&out))
}

/// 判断某行是否为提交首行：␟ 前是完整 hash（sha1 仓库 40 位 hex，sha256 仓库 64 位）
fn is_header_line(line: &str) -> bool {
    let b = line.as_bytes();
    match b.iter().position(|&c| c == 0x1f) {
        Some(pos) if pos == 40 || pos == 64 => b[..pos].iter().all(|c| c.is_ascii_hexdigit()),
        _ => false,
    }
}

/// 解析 git log 输出。按 %x1e 切分后，每段结构（字节级实测）：
/// 段 0：`hash␟an␟ae␟aI␟B首行` + B 的续行（含末尾换行）
/// 段 N：`\n` + 上一条提交的文件行 + 空行 + 第 N 条提交的首行与 message
/// 即：文件行总在头部之前、同段之内，且文件名不含 ␟，故段内第一个
/// 形如 hash␟ 的行必为本条提交首行；message 取其后所有行。
fn parse_log_output(out: &str) -> Vec<RawCommit> {
    let mut commits = Vec::new();
    for piece in out.split('\u{1e}') {
        let lines: Vec<&str> = piece.lines().collect();
        let Some(hi) = lines.iter().position(|l| is_header_line(l)) else {
            continue;
        };
        let parts: Vec<&str> = lines[hi].splitn(5, '\u{1f}').collect();
        if parts.len() < 5 {
            continue;
        }
        let mut message = parts[4].to_string();
        for line in &lines[hi + 1..] {
            message.push('\n');
            message.push_str(line);
        }
        commits.push(RawCommit {
            hash: parts[0].to_string(),
            author_name: parts[1].to_string(),
            author_email: parts[2].to_string(),
            time: parts[3].to_string(),
            message,
        });
    }
    commits
}

// ponytail: 提交边界靠 %x1e（record separator）+ 段内 hash␟ 首行定位；
// 已知边界：message 含 ␟ 或文件名恰好是 40/64 位 hex 串时可能混淆，
// 需要时改用 -z / 自定义脚本输出。
#[cfg(test)]
mod tests {
    use super::*;

    /// 按真实 git log --pretty --name-only 的字节级输出结构构造样例
    fn sample() -> String {
        let s = "\u{1f}";
        let e = "\u{1e}";
        let hash1 = "01d7d7ba4ce9e7c79e2ab86ca8663b9d00faf0b4";
        let hash2 = "ace6048cf417e3a1d5c9be9bb823f57bc28e5e80";
        // 段 0：hash2 的首行+message；段 1：\n b.txt \n 空行 hash1 首行+message；段 2：\n a.txt
        format!(
            "{h2}{s}tester{s}t@t.com{s}2026-08-28T13:36:22+08:00{s}single line msg{e}\nb.txt\n\n{h1}{s}tester{s}t@t.com{s}2026-08-28T13:36:22+08:00{s}subject line\n\nbody line 1\nbody line 2{e}\na.txt\n",
            h1 = hash1, h2 = hash2, s = s, e = e
        )
    }

    #[test]
    fn parse_multi_line_message_and_files() {
        let cs = parse_log_output(&sample());
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].hash, "ace6048cf417e3a1d5c9be9bb823f57bc28e5e80");
        assert_eq!(cs[0].message, "single line msg");
        assert_eq!(cs[0].author_name, "tester");
        assert_eq!(cs[0].author_email, "t@t.com");
        assert_eq!(cs[0].time, "2026-08-28T13:36:22+08:00");
        assert_eq!(
            cs[1].hash, "01d7d7ba4ce9e7c79e2ab86ca8663b9d00faf0b4"
        );
        // message 保留 subject 与 body 之间的空行（%B 语义）
        assert_eq!(cs[1].message, "subject line\n\nbody line 1\nbody line 2");
    }

    #[test]
    fn parse_empty() {
        assert!(parse_log_output("").is_empty());
        assert!(parse_log_output("\u{1e}\u{1e}").is_empty());
    }

    #[test]
    fn single_record_no_trailing_newline() {
        // 单条提交、无文件行也不 panic
        let s = "\u{1f}";
        let out = format!(
            "{h}{s}n{s}e{s}2026-01-01T00:00:00+08:00{s}only subject",
            h = "b".repeat(40),
            s = s
        );
        let cs = parse_log_output(&out);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].message, "only subject");
    }

    #[test]
    fn sha256_repo_header_recognized() {
        // sha256 仓库 hash 为 64 位 hex
        let s = "\u{1f}";
        let h = "a".repeat(64);
        let out = format!("{h}{s}n{s}e{s}t{s}msg", h = h, s = s);
        let cs = parse_log_output(&out);
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].hash, h);
    }

    #[test]
    fn short_hash_7() {
        assert_eq!(
            short_hash("01d7d7ba4ce9e7c79e2ab86ca8663b9d00faf0b4"),
            "01d7d7b"
        );
    }
}
