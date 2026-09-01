# GitReport — AI 工作日报生成器

一款基于 **Tauri 2 + Vue 3** 的桌面端 AI 报告工具：扫描并导入本地的多个 Git 仓库，按「你的身份」自动过滤提交记录，交由 AI 生成结构化的**日报 / 周报 / 月报**，支持流式输出、预览编辑、一键复制，并沉淀为本地历史。

> 告别「打开 git log 一条条抄 / 复制到 AI 再总结」的手工流程，让提交记录自己变成业务化的汇报。

---

## 功能特性

### 🚀 多项目导入
- 递归扫描指定目录下的全部 Git 仓库（默认限深 2 层），批量勾选导入。
- 每个项目可独立切换统计分支（本地分支列表，当前 HEAD 置顶）。
- 自动记录每个项目最近一次提交时间；路径失效时提示「仓库路径不存在」。

### 👤 按身份过滤「自己的提交」
- 支持多组身份 `name + email`，可手动添加，或一键「**从 git config 读取**」（优先仓库级，回退全局、系统级）。
- 组合数据库命中匹配：任一 `name` 或 `email` 匹配即保留；身份为空则不过滤。
- 顶部下拉可随时切换统计身份，或选择「全部提交（不过滤）」。

### 📰 三种报告类型 + 智能日期范围
- 日报 / 周报 / 月报，切换类型自动计算默认日期范围（今日本月/本周/整月）。
- **月报支持两种生成模式**：
  - **按提交记录** —— 直接汇总本月 Git 提交；
  - **按日报 / 周报** —— 从历史报告归档中自动拼装成月报（无需重复选项目拉取）。

### 🔍 提交预览
- 勾选项目或改日期后**自动**拉取并预览提交，按项目分组、时间倒序展示，无需等待生成。

### 🤖 AI 生成（OpenAI / Anthropic 双协议）
- 同时支持 **OpenAI 兼容**（任何 OpenAI 格式的网关/中转）与 **Anthropic** 协议，Base URL 智能拼接 `/v1`。
- **模型列表**：Base URL / API Key 就绪后一键「**获取模型**」，从接口拉取可用模型下拉选择（OpenAI `/models` 与 Anthropic `/v1/models` 自适应）。
- **流式输出**：边生成边显示，全程实时可见；内容以 Markdown 渲染，可切换「预览 / 编辑」离线改稿，一键复制全文。
- **token 用量**：生成结束后展示本次消耗 token 数（输入 / 输出），OpenAI 与 Anthropic 双端兼容。

### 🧩 提示词模板可定制
- 日报 / 周报 / 月报各有一套可编辑的提示词模板，内置变量插入：
  `{{commits}}` `{{date_range}}` `{{project_names}}` `{{report_type}}`。

### 🗂 历史归档
- 生成的报告自动落盘至本地存储，按类型浏览、随时回看、复制全文。

### 🔐 安全
- **API Key 不落配置文件**，存入操作系统钥匙串（Windows 凭据管理器 / macOS 钥匙串 / Linux Secret Service）。
- 所有 Git 数据均在本地读取，提交内容仅在生成时提交给所配置的 AI 接口。

### 🖥 桌面体验
- 无边框窗口 + 自定义标题栏，最小化/关闭按钮。
- **关闭按钮 → 隐藏到托盘**，常驻后台；托盘左键单击或菜单显示窗口；**单实例**，二次启动自动唤起已有窗口。

---

## 技术栈

| 层次 | 技术 |
|------|------|
| 桌面壳 | Tauri 2（Rust）、tauri-plugin-dialog / opener / single-instance / log |
| 前端框架 | Vue 3（`<script setup>` + TypeScript） |
| 构建 & 样式 | Vite 6、Tailwind CSS v4、`@tailwindcss/vite` |
| 状态管理 | Pinia（settings / projects / reports 三个 store，localStorage 持久化） |
| UI 组件 | lucide-vue-next 图标、自研 Tabs（滑块动画）、reka-ui、class-variance-authority / tailwind-merge |
| 内容渲染 | markdown-it + DOMPurify（Markdown 渲染 + XSS 安全） |
| 实用工具 | @vueuse/core（useClipboard 等） |
| 后端 git 操作 | Rust `std::process::Command` 调用 git、walkdir 扫描、chrono 日期处理 |
| AI 请求 | reqwest（blocking + stream，SSE 流式解析） |
| API Key 存储 | keyring 系统钥匙串 |

---

## 目录结构

```
gitreport/
├─ src/                     # 前端（Vue）
│  ├─ views/                # GenerateView / HistoryView / SettingsView
│  ├─ components/           # TitleBar / SideNav / Tabs / Toast
│  ├─ stores/               # Pinia store（settings / projects / reports）
│  ├─ api.ts                # 与 Rust 的 Tauri invoke 契约（camelCase 对齐）
│  ├─ types.ts              # 前后端共享类型契约
│  └─ main.ts / App.vue
└─ src-tauri/               # 后端（Rust）
   ├─ src/
   │  ├─ lib.rs             # 应用入口：command 注册、托盘、窗口关闭隐藏、单实例
   │  ├─ git.rs             # git 命令封装：扫描/身份/分支/提交拉取 + 单元测试
   │  ├─ ai.rs              # AI 接口：OpenAI/Anthropic 非流式与 SSE 流式 + 单元测试
   │  └─ secrets.rs         # API key 系统钥匙串存取
   └─ tauri.conf.json       # 窗口/构建/打包配置
```

> 前后端通过 Tauri `invoke` 通信，共享 `src/types.ts` 与 `src-tauri/src/git.rs` 中的类型（serde `rename_all = "camelCase"` 对齐）。

---

## 开发与打包

### 环境要求
- Node.js（pnpm 包管理器）
- Rust 工具链（cargo）
- 系统已安装 **git**（仓库扫描与提交拉取依赖）

### 开发模式（热更新）

```bash
pnpm install
pnpm tauri dev
```

### 类型检查 & 前端构建

```bash
pnpm build        # = vue-tsc --noEmit && vite build
```

### 打包 Windows 安装包（NSIS）

```bash
pnpm tauri build
```

> 产物位于 `src-tauri/target/release/bundle/`，图标为 240×240 自绘 Logo。

### CI 自动构建与发布（GitHub Actions）

项目内置 `build-windows.yml` 与 `build-macos.yml`：每次 push 到 `main` 或手动 `workflow_dispatch` 触发构建（并可缓存 cargo），当 push **以 `v` 开头的 tag** 时，会额外把对应平台的安装包自动上传为一个 **GitHub Release**（Windows NSIS `.exe` + macOS `.dmg`）。

发布一个版本：

```bash
git tag v1.0.0
git push origin v1.0.0
```

> 触发后会在 Actions 中看到一个 `GitReport v1.0.0` 的 Release，win / mac 两个平台的安装包自动合并挂载；Release 由 `tauri-apps/tauri-action` 创建，需在仓库设置中授权 `contents: write`（workflow 内已声明）。
>
> macOS 提供两种架构的 `.dmg`：**aarch64**（Apple Silicon / M 系列）与 **x86_64**（Intel），请按机型选择下载。

---

## 快速使用

1. **设置 → AI 模型**：填写接口协议、Base URL、模型名与 API Key，点「测试连接」确认可用；或点「**获取模型**」从接口拉取列表直接选择。
   - OpenAI 兼容示例：`https://api.openai.com/v1`、`gpt-4o-mini`
   - Anthropic 示例：`https://api.anthropic.com/v1`、`claude-sonnet-4-5`
2. **生成页**：点击「导入项目」选择工作目录，勾选仓库（可切换分支）。
3. （推荐）**设置 → Git 身份**：点击「从 git config 读取」自动填充身份，再点该身份卡片设为当前统计对象。
4. 选择**报告类型**（日报/周报/月报）与日期范围，底部点击「**生成**」，实时观看流式输出，可切「编辑」改稿、「复制全文」。
5. 生成结果自动进入**历史**，随时回看。

---

## 常见问题（FAQ）

**Q：为什么月报切换类型后项目列表没了？**
月报「按日报/周报」模式无需选项目，直接从历史日报/周报归档拼装；若该时段无历史记录会给出提示。

**Q：点击关闭窗口，程序怎么退出了？**
默认关闭是**最小化到托盘**。彻底退出请右键托盘图标 →「退出」。

**Q：提交预览为什么是空的？**
确认已勾选项目、日期范围有效，且当前身份在该时间段内有提交；「全部提交（不过滤）」可排查是否身份过滤导致。

**Q：API Key 存在哪里？**
存于系统钥匙串（Windows 凭据管理器），不写入任何配置文件或 localStorage。

---

## 许可

内部工具，暂无开源许可声明。
