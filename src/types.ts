// 前后端共享类型契约。Rust command 参数与返回值与这里对齐（camelCase，serde rename）。

export type ReportType = "daily" | "weekly" | "monthly";

export type AIProtocol = "openai" | "anthropic";

/** 设置页二级导航区块 */
export type SettingsSection = "ai" | "identities" | "templates";

export interface AIIdentity {
  name: string;
  email: string;
}

export interface AIConfig {
  protocol: AIProtocol;
  baseUrl: string;
  model: string;
  apiKey: string;
}

export interface Project {
  id: string;
  name: string;
  path: string;
  branch: string;
  importedAt: number;
  lastCommitAt: number | null;
  myCommitCount: number;
  missing?: boolean; // 路径不存在
  alias?: string; // 项目别称（AI 生成时展示用；未设置回退 name）
}

export interface CommitItem {
  hash: string; // 短 hash
  authorName: string;
  authorEmail: string;
  time: string; // ISO 时间
  message: string; // 完整 message（含 body）
  projectId: string;
  projectName: string;
}

export interface ProjectCommits {
  projectId: string;
  projectName: string;
  commits: CommitItem[];
}

export interface ReportRecord {
  id: string;
  type: ReportType;
  dateRange: string; // 展示用日期范围描述
  from: string; // ISO 日期
  to: string; // ISO 日期
  content: string; // Markdown 全文
  generatedAt: number;
  projectIds: string[];
}

export interface DetectResult {
  name: string;
  path: string;
  branch: string;
  lastCommitAt: number | null;
}

export interface DirtyCount {
  path: string;
  dirty: number; // 未提交文件数（含 untracked）
}

// 模板变量
export const TEMPLATE_VARS = [
  { key: "{{commits}}", label: "分组提交记录" },
  { key: "{{date_range}}", label: "日期范围" },
  { key: "{{project_names}}", label: "项目名列表" },
  { key: "{{report_type}}", label: "报告类型" },
] as const;
