import { invoke, Channel } from "@tauri-apps/api/core";
import type {
  AIIdentity,
  AIConfig,
  DetectResult,
  ProjectCommits,
} from "./types";

// ===== 契约：Tauri command 名与参数（camelCase）=====
// Rust 侧用 serde(rename_all = "camelCase") 对齐。三个 agent 都以此为准，不要改签名。

/** 检测系统是否安装了可用的 git */
export async function checkGit(): Promise<boolean> {
  return invoke<boolean>("check_git");
}

/** 递归扫描目录下的 git 仓库（限深 2 层） */
export async function detectGitRepos(rootDir: string): Promise<DetectResult[]> {
  return invoke<DetectResult[]>("detect_git_repos", { rootDir });
}

/** 读取指定仓库 git config 中的 user.name / user.email */
export async function getGitIdentity(path: string): Promise<AIIdentity> {
  return invoke<AIIdentity>("get_git_identity", { path });
}

/** 列出仓库所有本地分支（当前 HEAD 置顶） */
export async function getGitBranches(path: string): Promise<string[]> {
  return invoke<string[]>("get_git_branches", { path });
}

/** 读取仓库当前 HEAD 分支名（用于前台时同步外部切换的分支） */
export async function getCurrentBranch(path: string): Promise<string> {
  return invoke<string>("get_git_current_branch", { path });
}

export interface RepoRef {
  path: string;
  branch: string; // 空串 = 当前 HEAD
}

/**
 * 并发拉取多个仓库指定分支的提交记录，按作者身份过滤。
 * authors: 多组 {name, email}，name 或 email 匹配任一即算自己的提交；空数组不过滤。
 * from/to: ISO 日期字符串（yyyy-MM-dd）。
 */
export async function fetchCommits(
  repos: RepoRef[],
  authors: AIIdentity[],
  from: string,
  to: string,
): Promise<ProjectCommits[]> {
  return invoke<ProjectCommits[]>("fetch_commits", { repos, authors, from, to });
}

export interface GeneratePayload {
  config: AIConfig;
  system: string; // 渲染后的提示词模板
  user: string; // 提交数据文本
}

/** 调 AI 生成日报，返回 Markdown 全文 */
export async function generateReport(payload: GeneratePayload): Promise<string> {
  return invoke<string>("generate_report", { payload });
}

export interface ReportUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface StreamResult {
  content: string;
  usage?: ReportUsage | null;
}

/**
 * 流式调 AI 生成日报。
 * onDelta 每收到一段增量文本即调用；返回 { content, usage }（usage 含 token 用量，可能为空）。
 */
export async function generateReportStream(
  payload: GeneratePayload,
  onDelta: (delta: string) => void,
): Promise<StreamResult> {
  const channel = new Channel<string>();
  channel.onmessage = (delta) => onDelta(delta);
  return invoke<StreamResult>("generate_report_stream", { payload, channel });
}

/** 最小请求验证 AI 配置，返回耗时描述（如 "0.8s"），失败抛错 */
export async function testAiConnection(config: AIConfig): Promise<string> {
  return invoke<string>("test_ai_connection", { config });
}

/** 拉取接口的模型列表（model id 数组），用于设置页下拉填充 */
export async function listModels(config: AIConfig): Promise<string[]> {
  return invoke<string[]>("fetch_models", { config });
}

// ===== API key 安全存储（系统钥匙串）=====

/** 保存 API key 到系统钥匙串 */
export async function saveApiKey(apiKey: string): Promise<void> {
  return invoke<void>("save_api_key", { apiKey });
}

/** 从系统钥匙串读取 API key（未保存返回 null） */
export async function loadApiKey(): Promise<string | null> {
  return invoke<string | null>("load_api_key");
}

/** 从系统钥匙串删除 API key */
export async function deleteApiKey(): Promise<void> {
  return invoke<void>("delete_api_key");
}
