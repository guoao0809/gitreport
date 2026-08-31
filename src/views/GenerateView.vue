<script setup lang="ts">
import { computed, ref, watch, onMounted, onBeforeUnmount } from "vue";
import MarkdownIt from "markdown-it";
import DOMPurify from "dompurify";
import { useClipboard } from "@vueuse/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Sparkles, RefreshCw, Copy, ChevronRight, FolderPlus, GitBranch, ChevronDown, Trash2, RefreshCw as ScanIcon, Pencil, UserRound } from "lucide-vue-next";
import type { DetectResult, Project, ProjectCommits, ReportType } from "../types";
import { fetchCommits, generateReportStream, detectGitRepos, getGitBranches, getCurrentBranch, gitDirtyCounts } from "../api";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useSettingStore } from "../stores/settings";
import { useProjectStore } from "../stores/projects";
import { useReportStore } from "../stores/reports";
import { showToast } from "../components/toast";
import Tabs from "../components/Tabs.vue";
import { SelectRoot, SelectTrigger, SelectValue, SelectPortal, SelectContent, SelectViewport, SelectItem, SelectItemText } from "reka-ui";

const settingStore = useSettingStore();
const projectStore = useProjectStore();
const reportStore = useReportStore();

// 当前用于统计的身份（未选中时为 null）
const activeIdentity = computed(() => {
  const idx = settingStore.activeIdentityIndex;
  return idx >= 0 && idx < settingStore.identities.length ? settingStore.identities[idx] : null;
});
/** 是否已配置 git 身份（身份列表非空即视为已配置） */
const hasIdentity = computed(() => settingStore.identities.length > 0);
// 必须有 git 身份才能筛选提交：无身份 → 空数组（不展示任何提交），否则按所选身份过滤
const filterAuthors = computed(() => (activeIdentity.value ? [activeIdentity.value] : []));

/** 顶部身份下拉框：value 为身份下标字符串，切换后自动刷新提交预览 */
const identitySelectValue = computed({
  get: () => String(settingStore.activeIdentityIndex),
  set: (v: string) => {
    settingStore.setActiveIdentity(Number(v));
    autoLoadPreview();
  },
});

// ===== 报告类型与日期范围 =====
const reportType = ref<ReportType>("daily");
const from = ref("");
const to = ref("");
// 月报生成模式：git = 按提交记录；reports = 按已生成的日报/周报
const monthlyMode = ref<"git" | "reports">("git");

const typeLabels: Record<ReportType, string> = {
  daily: "日报",
  weekly: "周报",
  monthly: "月报",
};
const typeTabs = computed(() =>
  (Object.entries(typeLabels) as [ReportType, string][]).map(([value, label]) => ({ value, label })),
);
const monthlyModeTabs = [
  { value: "git" as const, label: "按提交记录" },
  { value: "reports" as const, label: "按日报/周报" },
];
const modeTabs = [
  { value: "preview" as const, label: "预览" },
  { value: "edit" as const, label: "编辑" },
];

function fmt(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

/** 按类型设置默认日期范围：日报=今天，周报=本周一~周日，月报=本月1号~末日 */
function applyDefaultRange(type: ReportType) {
  const now = new Date();
  if (type === "daily") {
    from.value = fmt(now);
    to.value = fmt(now);
  } else if (type === "weekly") {
    const day = (now.getDay() + 6) % 7; // 周一=0
    const monday = new Date(now);
    monday.setDate(now.getDate() - day);
    const sunday = new Date(monday);
    sunday.setDate(monday.getDate() + 6);
    from.value = fmt(monday);
    to.value = fmt(sunday);
  } else {
    from.value = fmt(new Date(now.getFullYear(), now.getMonth(), 1));
    to.value = fmt(new Date(now.getFullYear(), now.getMonth() + 1, 0));
  }
}
applyDefaultRange("daily");
watch(reportType, (t) => applyDefaultRange(t));

// ===== 项目选择 =====
const selected = ref(new Set<string>());
const projects = computed<Project[]>(() => projectStore.projects);
const allSelected = computed(
  () => projects.value.length > 0 && selected.value.size === projects.value.length,
);
function toggleAll() {
  if (allSelected.value) selected.value.clear();
  else projects.value.forEach((p) => selected.value.add(p.id));
}
function toggleOne(id: string) {
  if (selected.value.has(id)) selected.value.delete(id);
  else selected.value.add(id);
}

function fmtDate(ts: number | null): string {
  if (!ts) return "暂无提交";
  const d = new Date(ts);
  return fmt(d);
}

// ===== 项目别称（AI 生成时展示用，未设置回退仓库名）=====
function displayName(id: string, fallback = ""): string {
  const p = projects.value.find((x) => x.id === id);
  if (p?.alias?.trim()) return p.alias.trim();
  return p?.name || fallback;
}
const editingAliasId = ref<string | null>(null);
const aliasDraft = ref("");
const vFocus = { mounted: (el: HTMLElement) => el.focus() };

function startEditAlias(p: Project) {
  editingAliasId.value = p.id;
  aliasDraft.value = p.alias || "";
}
function saveAlias(id: string) {
  if (editingAliasId.value !== id) return; // esc 已取消
  projectStore.setProjectAlias(id, aliasDraft.value.trim());
  editingAliasId.value = null;
}
function cancelAlias() {
  editingAliasId.value = null;
}

// ===== 导入项目 =====
const scanning = ref(false);
const candidates = ref<DetectResult[]>([]);
const candidateChecked = ref<Set<string>>(new Set());
const importDialogOpen = ref(false);

async function importProjects() {
  const dir = await open({ directory: true, title: "选择要扫描的目录" });
  if (!dir || typeof dir !== "string") return;
  scanning.value = true;
  try {
    const detected = await detectGitRepos(dir);
    if (detected.length === 0) {
      showToast("该目录下没有发现 git 仓库", "error");
      return;
    }
    candidates.value = detected;
    candidateChecked.value = new Set(
      detected.filter((d) => !projectStore.hasProject(d.path)).map((d) => d.path),
    );
    importDialogOpen.value = true;
  } catch (e) {
    showToast(`扫描失败：${e}`, "error");
  } finally {
    scanning.value = false;
  }
}

function toggleCandidate(path: string) {
  if (candidateChecked.value.has(path)) candidateChecked.value.delete(path);
  else candidateChecked.value.add(path);
  candidateChecked.value = new Set(candidateChecked.value);
}

function confirmImport() {
  const paths = candidates.value
    .map((d) => d.path)
    .filter((p) => candidateChecked.value.has(p));
  if (paths.length === 0) {
    showToast("请至少勾选一个仓库", "error");
    return;
  }
  const added = projectStore.addProjects(paths, candidates.value);
  showToast(`成功导入 ${added} 个项目`, "success");
  importDialogOpen.value = false;
}

// ===== 分支切换 =====
const branchesCache = ref<Record<string, string[]>>({});
const branchLoading = ref<Record<string, boolean>>({});
const branchOpenFor = ref<string | null>(null);

async function toggleBranches(id: string, path: string) {
  if (branchOpenFor.value === id) {
    branchOpenFor.value = null;
    return;
  }
  branchOpenFor.value = id;
  if (!branchesCache.value[id]) {
    branchLoading.value[id] = true;
    try {
      branchesCache.value[id] = await getGitBranches(path);
    } catch (e) {
      showToast(`获取分支失败：${e}`, "error");
      branchesCache.value[id] = [];
    } finally {
      branchLoading.value[id] = false;
    }
  }
}

function changeBranch(id: string, branch: string) {
  projectStore.updateBranch(id, branch);
  branchOpenFor.value = null;
  // 该项目已被勾选时，切分支后刷新提交预览
  if (selected.value.has(id)) autoLoadPreview();
  showToast("已切换分支", "success");
}

// 点击下拉容器/触发按钮以外的区域时收起分支下拉
function onDocClick(e: MouseEvent) {
  const target = e.target as HTMLElement | null;
  if (target && target.closest("[data-branch-dropdown]")) return;
  branchOpenFor.value = null;
}
onMounted(() => document.addEventListener("click", onDocClick));
onBeforeUnmount(() => document.removeEventListener("click", onDocClick));

// ===== 窗口回到前台时，同步外部（如 VSCode）切换的分支 + 刷新未提交状态 =====
let unlistenFocus: (() => void) | undefined;
/** 项目路径 → 未提交文件数（含 untracked） */
const dirtyMap = ref<Record<string, number>>({});

async function refreshDirty() {
  if (projects.value.length === 0) return;
  try {
    const list = await gitDirtyCounts(projects.value.map((p) => p.path));
    dirtyMap.value = Object.fromEntries(list.map((d) => [d.path, d.dirty]));
  } catch {
    /* 刷新失败静默忽略 */
  }
}

async function syncBranchesFromExternal() {
  if (projects.value.length === 0) return;
  let changed = false;
  await Promise.all([
    Promise.all(
      projects.value.map(async (p) => {
        try {
          const branch = await getCurrentBranch(p.path);
          if (branch && branch !== p.branch) {
            projectStore.updateBranch(p.id, branch);
            changed = true;
          }
        } catch {
          /* 仓库不可访问则忽略 */
        }
      }),
    ),
    refreshDirty(),
  ]);
  // 有勾选项目且分支变化时刷新提交预览
  if (changed && selected.value.size > 0) autoLoadPreview();
}
onMounted(() => {
  refreshDirty();
  getCurrentWindow()
    .onFocusChanged(({ payload }) => {
      if (payload) syncBranchesFromExternal();
    })
    .then((un) => (unlistenFocus = un));
});
onBeforeUnmount(() => {
  document.removeEventListener("click", onDocClick);
  unlistenFocus?.();
});

function removeProject(id: string) {
  const p = projects.value.find((x) => x.id === id);
  if (window.confirm(`确定移除项目「${p?.alias?.trim() || p?.name || id}」吗？`)) {
    projectStore.removeProject(id);
    selected.value.delete(id);
    showToast("已移除项目", "success");
  }
}

// ===== 提交预览（勾选项目或日期变化时自动加载）=====
const previews = ref<ProjectCommits[]>([]);
const previewLoading = ref(false);

async function autoLoadPreview() {
  const repos = projects.value
    .filter((p) => selected.value.has(p.id))
    .map((p) => ({ path: p.path, branch: p.branch }));
  if (repos.length === 0 || !hasIdentity.value) {
    previews.value = [];
    return;
  }
  const authors = filterAuthors.value;
  previewLoading.value = true;
  try {
    previews.value = await fetchCommits(repos, authors, from.value, to.value);
  } catch (e) {
    // 自动加载失败静默处理，生成时会有明确报错
    console.error("自动拉取提交失败：", e);
  } finally {
    previewLoading.value = false;
  }
}

// 勾选集合（序列化）或日期范围变化时，自动刷新提交预览
const selectedKey = computed(() => Array.from(selected.value).sort().join(","));
watch([selectedKey, from, to], () => autoLoadPreview());

// 按项目分组展示提交（保持 fetch 返回顺序，每个项目内按时间倒序）
const projectPreviews = computed(() =>
  previews.value
    .map((pc) => ({
      ...pc,
      commits: [...pc.commits].sort((a, b) => (a.time < b.time ? 1 : -1)),
    }))
    .filter((pc) => pc.commits.length > 0),
);

// ===== 结果区 =====
const result = ref("");
const mode = ref<"preview" | "edit">("preview");
const generating = ref(false);
const stage = ref("");
const lastUsage = ref<{ prompt: number; completion: number; total: number } | null>(null);

const md = new MarkdownIt({ breaks: true, linkify: true });
const renderedHtml = computed(() =>
  result.value ? DOMPurify.sanitize(md.render(result.value)) : "",
);

const { copy } = useClipboard({ legacy: true });
async function copyAll() {
  if (!result.value) {
    showToast("没有可复制的内容", "error");
    return;
  }
  await copy(result.value);
  showToast("已复制", "success");
}

/** 组装提交数据文本：按项目分组 */
function buildUserText(groups: ProjectCommits[], dateRange: string): string {
  const lines: string[] = [`日期范围：${dateRange}`, ""];
  for (const g of groups) {
    lines.push(`## ${displayName(g.projectId, g.projectName)}`);
    if (g.commits.length === 0) {
      lines.push("（该时间段内没有提交）");
    } else {
      for (const c of g.commits) {
        lines.push(`- ${c.time.slice(0, 10)} ${c.hash} ${c.message.split("\n")[0]}`);
      }
    }
    lines.push("");
  }
  return lines.join("\n");
}

/** 用模板渲染 system 提示词：替换 {{commits}} 等变量 */
function renderTemplate(tpl: string, vars: Record<string, string>): string {
  return tpl.replace(/\{\{\w+\}\}/g, (m) => vars[m] ?? m);
}

/** 月报「按日报/周报」模式：无需勾选项目，从历史报告拼装 */
const useReportsMode = computed(
  () => reportType.value === "monthly" && monthlyMode.value === "reports",
);

/** 生成按钮可用性：key 就绪 + 普通模式需有身份、勾选项目且预览到提交；reports 模式需日期范围内有历史报告 */
const canGenerate = computed(() => {
  if (!settingStore.keyReady) return false;
  if (generating.value) return false;
  if (useReportsMode.value) {
    return reportStore.reports.some(
      (r) =>
        (r.type === "daily" || r.type === "weekly") &&
        r.from >= from.value &&
        r.from <= to.value,
    );
  }
  if (!hasIdentity.value) return false;
  const chosen = projects.value.filter((p) => selected.value.has(p.id));
  if (chosen.length === 0) return false;
  return projectPreviews.value.length > 0;
});

// ===== 生成时未提交确认弹窗 =====
/** 非空 = 弹窗展示中的「有未提交文件的项目」列表 */
const pendingDirtyList = ref<{ p: Project; dirty: number }[]>([]);

function confirmDirtyDialog(go: boolean) {
  const useReports = reportType.value === "monthly" && monthlyMode.value === "reports";
  pendingDirtyList.value = [];
  if (go) doGenerate(useReports);
}

async function generate() {
  if (generating.value) return;
  const ai = settingStore.ai;
  if (!ai) {
    showToast("请先在设置中配置 AI 模型", "error");
    return;
  }

  // 月报的「按日报/周报」模式：从历史报告拼装，无需选项目/拉 git
  const useReports = reportType.value === "monthly" && monthlyMode.value === "reports";

  if (!useReports && !hasIdentity.value) {
    showToast("请先到「设置 → Git 身份」配置提交人身份", "error");
    return;
  }

  const chosen = projects.value.filter((p) => selected.value.has(p.id));
  if (!useReports && chosen.length === 0) {
    showToast("请先选择项目", "error");
    return;
  }

  // 勾选项目有未提交文件时弹窗确认（生成前实时查一次，保证准确）
  if (!useReports) {
    await refreshDirty();
    const dirtyList = chosen
      .map((p) => ({ p, dirty: dirtyMap.value[p.path] ?? 0 }))
      .filter((d) => d.dirty > 0);
    if (dirtyList.length > 0) {
      pendingDirtyList.value = dirtyList;
      return; // 等待用户选择：忽略并继续 / 取消
    }
  }
  await doGenerate(useReports);
}

/** 弹窗中「忽略并继续」后真正执行生成 */
async function doGenerate(useReports: boolean) {
  const ai = settingStore.ai!;
  const authors = filterAuthors.value;
  const chosen = projects.value.filter((p) => selected.value.has(p.id));

  generating.value = true;
  stage.value = useReports ? "正在汇总历史报告…" : "正在统计提交…";
  try {
    let user: string;
    let projectNames: string;
    let projectIds: string[];

    if (useReports) {
      // 汇总日期范围内的日报/周报
      const records = reportStore.reports
        .filter(
          (r) =>
            (r.type === "daily" || r.type === "weekly") &&
            r.from >= from.value &&
            r.from <= to.value,
        )
        .sort((a, b) => a.from.localeCompare(b.from));
      if (records.length === 0) {
        showToast("所选时间段内没有日报/周报记录", "error");
        return;
      }
      user = buildUserTextFromReports(records);
      projectNames = [...new Set(records.flatMap((r) => r.projectIds))]
        .map((id) => displayName(id))
        .filter(Boolean)
        .join("、");
      projectIds = [...new Set(records.flatMap((r) => r.projectIds))];
    } else {
      const groups = await fetchCommits(
        chosen.map((p) => ({ path: p.path, branch: p.branch })),
        authors,
        from.value,
        to.value,
      );
      previews.value = groups;
      const total = groups.reduce((n, g) => n + g.commits.length, 0);
      if (total === 0) {
        showToast("所选时间段内没有你的提交记录", "error");
        return;
      }
      const dateRange = from.value === to.value ? from.value : `${from.value} 至 ${to.value}`;
      user = buildUserText(groups, dateRange);
      projectNames = groups.map((g) => displayName(g.projectId, g.projectName)).join("、");
      projectIds = chosen.map((p) => p.id);
    }

    const dateRange = from.value === to.value ? from.value : `${from.value} 至 ${to.value}`;
    const system = renderTemplate(settingStore.templates[reportType.value], {
      "{{commits}}": user,
      "{{date_range}}": dateRange,
      "{{project_names}}": projectNames,
      "{{report_type}}": typeLabels[reportType.value],
    });

    stage.value = "正在生成…";
    result.value = "";
    lastUsage.value = null;
    mode.value = "preview";
    // 流式：每收到增量就追加显示，返回完整文本与 token 用量
    const { content, usage } = await generateReportStream(
      { config: ai, system, user },
      (delta) => {
        result.value += delta;
      },
    );
    result.value = content;
    mode.value = "preview";
    if (usage && usage.totalTokens > 0) {
      lastUsage.value = { prompt: usage.promptTokens, completion: usage.completionTokens, total: usage.totalTokens };
    }

    reportStore.addReport({
      id: `r-${Date.now()}`,
      type: reportType.value,
      dateRange,
      from: from.value,
      to: to.value,
      content,
      generatedAt: Date.now(),
      projectIds,
    });
    showToast("生成完成", "success");
  } catch (e) {
    showToast(`生成失败：${e}`, "error");
  } finally {
    generating.value = false;
    stage.value = "";
  }
}

/** 从历史日报/周报记录拼装月报输入文本 */
function buildUserTextFromReports(records: { type: ReportType; dateRange: string; content: string }[]): string {
  const lines: string[] = [];
  for (const r of records) {
    lines.push(`## ${typeLabels[r.type]} ${r.dateRange}`);
    lines.push(r.content);
    lines.push("");
  }
  return lines.join("\n");
}
</script>

<template>
  <div class="flex h-full flex-col gap-4 p-4">
    <!-- 顶部：类型 + 日期范围 + 导入项目 -->
    <div class="flex flex-wrap items-center gap-3">
      <Tabs v-model="reportType" :tabs="typeTabs" />
      <!-- 月报模式切换 -->
      <Tabs v-if="reportType === 'monthly'" v-model="monthlyMode" :tabs="monthlyModeTabs" />
      <div class="flex items-center gap-2 text-sm text-text">
        <input
          v-model="from"
          type="date"
          class="rounded-lg border border-border bg-panel px-2.5 py-1.5 text-sm text-title outline-none focus:border-primary"
        />
        <span class="text-muted">至</span>
        <input
          v-model="to"
          type="date"
          class="rounded-lg border border-border bg-panel px-2.5 py-1.5 text-sm text-title outline-none focus:border-primary"
        />
      </div>
      <!-- git 身份切换 -->
      <SelectRoot
        v-if="settingStore.identities.length > 0"
        v-model="identitySelectValue"
      >
        <SelectTrigger
          class="group flex items-center gap-1.5 rounded-lg border border-border bg-panel py-1.5 pl-2.5 pr-2.5 text-sm text-title outline-none focus:border-primary"
        >
          <SelectValue />
          <ChevronDown
            :size="14"
            class="text-muted transition-transform duration-200 group-data-[state=open]:rotate-180"
          />
        </SelectTrigger>
        <SelectPortal>
          <SelectContent
            position="popper"
            class="z-30 max-w-72 overflow-hidden rounded-lg border border-border bg-panel shadow-lg"
          >
            <SelectViewport class="max-h-72 overflow-y-auto p-1">
              <SelectItem
                value="-1"
                class="cursor-pointer rounded-md px-3 py-2 text-left text-sm text-text outline-none data-highlighted:bg-surface data-highlighted:text-title data-state-checked:text-primary"
              >
                <SelectItemText>全部提交（不过滤）</SelectItemText>
              </SelectItem>
              <SelectItem
                v-for="(ident, i) in settingStore.identities"
                :key="i"
                :value="String(i)"
                class="cursor-pointer rounded-md px-3 py-2 text-left text-sm text-text outline-none data-highlighted:bg-surface data-highlighted:text-title data-state-checked:text-primary"
              >
                <SelectItemText>{{ ident.name }} &lt;{{ ident.email }}&gt;</SelectItemText>
              </SelectItem>
            </SelectViewport>
          </SelectContent>
        </SelectPortal>
      </SelectRoot>
      <!-- 未配置 git 身份：跳转到设置页配置 -->
      <button
        v-else
        class="flex items-center gap-1.5 rounded-lg border border-dashed border-border bg-panel px-3 py-1.5 text-sm text-muted hover:border-primary hover:text-primary"
        @click="settingStore.openSettingsSection('identities')"
      >
        <UserRound :size="14" />
        设置 git 身份
      </button>
      <button
        class="flex items-center gap-1.5 rounded-lg border border-border bg-panel px-3 py-1.5 text-sm text-text hover:border-primary hover:text-primary disabled:opacity-50"
        :disabled="scanning"
        @click="importProjects"
      >
        <ScanIcon v-if="scanning" :size="14" class="animate-spin" />
        <FolderPlus v-else :size="14" />
        {{ scanning ? "扫描中…" : "导入项目" }}
      </button>
    </div>

    <!-- 中部两栏 -->
    <div class="flex min-h-0 flex-1 gap-4">
      <!-- 左：项目选择 + 提交预览 -->
      <div class="flex w-[55%] min-w-0 flex-col rounded-xl border border-border bg-panel">
        <div class="flex items-center justify-between border-b border-border px-4 py-3">
          <span class="text-sm font-medium text-title">项目（{{ projects.length }}）</span>
          <button class="text-sm text-primary hover:underline" @click="toggleAll">
            {{ allSelected ? "取消全选" : "全选" }}
          </button>
        </div>
        <div class="flex-1 overflow-y-auto px-2 py-1">
          <div
            v-for="p in projects"
            :key="p.id"
            class="flex items-center gap-2.5 rounded-lg px-2.5 py-2 hover:bg-surface"
          >
            <input
              type="checkbox"
              class="h-4 w-4 shrink-0 accent-[#4F6BF6]"
              :checked="selected.has(p.id)"
              @change="toggleOne(p.id)"
            />
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <template v-if="editingAliasId === p.id">
                  <input
                    v-model="aliasDraft"
                    v-focus
                    type="text"
                    placeholder="项目别称"
                    class="w-40 shrink-0 rounded border border-primary px-1.5 py-0.5 text-sm text-title outline-none"
                    @keyup.enter="saveAlias(p.id)"
                    @keyup.esc="cancelAlias"
                    @blur="saveAlias(p.id)"
                  />
                </template>
                <template v-else>
                  <span
                    class="min-w-0 max-w-40 truncate text-sm text-title"
                    :title="p.alias?.trim() ? `别称：${p.alias}` : '设置项目别称'"
                  >{{ p.name }}</span>
                  <span
                    v-if="p.alias?.trim()"
                    class="shrink-0 rounded bg-primary/10 px-1 py-0.5 text-xs text-primary"
                    :title="`别称：${p.alias}`"
                  >{{ p.alias }}</span>
                  <button
                    class="shrink-0 rounded p-0.5 text-muted hover:bg-surface hover:text-primary"
                    title="编辑项目别称"
                    @click="startEditAlias(p)"
                  >
                    <Pencil :size="12" />
                  </button>
                </template>
                <!-- 分支切换 -->
                <div class="relative" data-branch-dropdown>
                  <button
                    class="flex items-center gap-1 rounded px-1.5 py-0.5 font-mono text-xs text-muted hover:bg-surface hover:text-primary"
                    :disabled="!!p.missing"
                    @click="toggleBranches(p.id, p.path)"
                  >
                    <GitBranch :size="13" />
                    {{ p.branch || "HEAD" }}
                    <ChevronDown :size="12" />
                  </button>
                  <div
                    v-if="branchOpenFor === p.id"
                    class="absolute left-0 top-full z-30 mt-1 max-h-60 min-w-40 overflow-y-auto rounded-lg border border-border bg-panel shadow-lg"
                  >
                    <div v-if="branchLoading[p.id]" class="px-3 py-2 text-xs text-muted">
                      加载中…
                    </div>
                    <button
                      v-else
                      v-for="b in branchesCache[p.id] || []"
                      :key="b"
                      class="flex w-full items-center justify-between px-3 py-1.5 text-left text-sm text-text hover:bg-surface"
                      :class="b === p.branch ? 'font-medium text-primary' : ''"
                      @click="changeBranch(p.id, b)"
                    >
                      {{ b }}
                      <span v-if="b === p.branch" class="text-xs text-primary">✓</span>
                    </button>
                  </div>
                </div>
                <span v-if="p.missing" class="text-xs text-orange-500">仓库路径不存在</span>
              </div>
              <div class="truncate text-xs text-muted" :title="p.path">{{ p.path }}</div>
            </div>
            <span class="shrink-0 text-xs text-muted">{{ fmtDate(p.lastCommitAt) }}</span>
            <div class="flex shrink-0 flex-col items-end gap-0.5">
              <button
                class="rounded-lg p-1 text-muted hover:bg-red-50 hover:text-red-500"
                title="移除项目"
                @click="removeProject(p.id)"
              >
                <Trash2 :size="14" />
              </button>
              <span
                v-if="!p.missing && (dirtyMap[p.path] ?? 0) > 0"
                class="text-xs leading-none text-orange-500"
                :title="`${dirtyMap[p.path]} 个文件未提交（含未跟踪）`"
              >未提交 {{ dirtyMap[p.path] }}</span>
            </div>
          </div>
          <div v-if="projects.length === 0" class="py-10 text-center text-sm text-muted">
            暂无项目，点击右上角「导入项目」添加
          </div>
        </div>

        <!-- 提交预览（自动加载，按项目分组） -->
        <div class="border-t border-border">
          <div class="flex items-center gap-1.5 px-4 py-2.5 text-sm text-text">
            <ChevronRight :size="14" class="shrink-0 text-muted" />
            提交预览
            <span v-if="previewLoading" class="text-xs text-muted">加载中…</span>
          </div>
          <div class="max-h-56 overflow-y-auto border-t border-border px-4 py-2">
            <div
              v-if="projectPreviews.length === 0 && !previewLoading"
              class="py-3 text-center text-xs text-muted"
            >
              <template v-if="!hasIdentity">请先到「设置 → Git 身份」配置提交人身份</template>
              <template v-else>勾选项目后自动显示提交记录</template>
            </div>
            <div v-for="pc in projectPreviews" :key="pc.projectId" class="mb-2">
              <div class="sticky top-0 mb-1 bg-panel py-0.5 text-xs font-medium text-title">
                {{ displayName(pc.projectId, pc.projectName) }}
                <span class="font-normal text-muted">（{{ pc.commits.length }} 条）</span>
              </div>
              <div
                v-for="(c, i) in pc.commits"
                :key="i"
                class="flex items-start gap-2 border-b border-border py-1.5 text-xs last:border-b-0"
              >
                <span class="shrink-0 text-muted">{{ c.time.slice(0, 10) }}</span>
                <span class="shrink-0 font-mono text-primary">{{ c.hash }}</span>
                <span class="min-w-0 flex-1 break-all text-text">
                  {{ c.message.split("\n")[0] }}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- 右：日报结果 -->
      <div class="flex w-[45%] min-w-0 flex-col rounded-xl border border-border bg-panel">
        <div class="flex items-center gap-2 border-b border-border px-4 py-2.5">
          <Tabs v-model="mode" :tabs="modeTabs" size="sm" :disabled="generating" />
          <div class="ml-auto flex items-center gap-2">
            <button
              class="flex items-center gap-1 rounded-lg border border-border px-2.5 py-1.5 text-xs text-text hover:border-primary hover:text-primary disabled:opacity-50"
              :disabled="generating"
              @click="generate"
            >
              <RefreshCw :size="13" :class="generating ? 'animate-spin' : ''" />
              重新生成
            </button>
            <button
              class="flex items-center gap-1 rounded-lg bg-primary px-2.5 py-1.5 text-xs text-white hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
              :disabled="generating"
              @click="copyAll"
            >
              <Copy :size="13" />
              复制全文
            </button>
          </div>
        </div>

        <div class="min-h-0 flex-1 overflow-y-auto p-4">
          <div
            v-if="lastUsage"
            class="mb-2 text-xs text-muted"
          >本次消耗：{{ lastUsage.total }} token（输入 {{ lastUsage.prompt }} / 输出 {{ lastUsage.completion }}）</div>
          <template v-if="result">
            <div v-if="mode === 'preview'" class="markdown-body" v-html="renderedHtml"></div>
            <textarea
              v-else
              v-model="result"
              class="h-full min-h-75 w-full resize-none rounded-lg border border-border p-3 font-mono text-xs leading-relaxed text-title outline-none focus:border-primary"
            ></textarea>
            <div
              v-if="generating"
              class="mt-2 flex items-center gap-2 text-xs text-muted"
            >
              <span class="inline-block h-3 w-3 animate-pulse rounded-full bg-primary"></span>
              生成中…
            </div>
          </template>
          <div
            v-else-if="generating"
            class="flex h-full flex-col items-center justify-center gap-3 text-sm text-muted"
          >
            <Sparkles :size="28" class="animate-pulse text-primary" />
            {{ stage }}
          </div>
          <div
            v-else
            class="flex h-full flex-col items-center justify-center gap-3 text-sm text-muted"
          >
            <Sparkles :size="28" class="text-muted" />
            选择项目后点击「生成」
          </div>
        </div>
      </div>
    </div>

    <!-- 底部：生成按钮 -->
    <div class="flex shrink-0 items-center justify-center">
      <button
        class="flex items-center gap-2 rounded-lg bg-primary px-8 py-2 text-sm text-white shadow hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
        :disabled="!canGenerate"
        @click="generate"
      >
        <Sparkles :size="15" />
        {{ generating ? stage : "生成" }}
      </button>
    </div>

    <!-- 未提交文件确认弹窗 -->
    <div
      v-if="pendingDirtyList.length > 0"
      class="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
      @click.self="confirmDirtyDialog(false)"
    >
      <div class="w-105 rounded-xl bg-panel shadow-2xl">
        <div class="border-b border-border px-5 py-3.5 text-sm font-medium text-title">
          以下项目有文件未提交
        </div>
        <div class="max-h-60 overflow-y-auto px-5 py-3">
          <div
            v-for="d in pendingDirtyList"
            :key="d.p.id"
            class="flex items-center justify-between gap-3 py-1 text-sm"
          >
            <span class="min-w-0 truncate text-text" :title="d.p.path">
              {{ displayName(d.p.id, d.p.name) }}
            </span>
            <span class="shrink-0 text-xs text-orange-500">{{ d.dirty }} 个文件未提交</span>
          </div>
        </div>
        <div class="flex justify-end gap-2 border-t border-border px-5 py-3">
          <button
            class="rounded-lg border border-border px-4 py-1.5 text-sm text-text hover:border-primary hover:text-primary"
            @click="confirmDirtyDialog(false)"
          >
            取消
          </button>
          <button
            class="rounded-lg bg-primary px-4 py-1.5 text-sm text-white hover:opacity-90"
            @click="confirmDirtyDialog(true)"
          >
            忽略并继续生成
          </button>
        </div>
      </div>
    </div>

    <!-- 候选仓库弹窗 -->
    <div
      v-if="importDialogOpen"
      class="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
      @click.self="importDialogOpen = false"
    >
      <div class="flex max-h-[70vh] w-[560px] flex-col rounded-xl bg-panel shadow-2xl">
        <div class="border-b border-border px-5 py-3.5 text-sm font-medium text-title">
          发现 {{ candidates.length }} 个 git 仓库
        </div>
        <div class="flex-1 overflow-y-auto px-2 py-2">
          <label
            v-for="d in candidates"
            :key="d.path"
            class="flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 hover:bg-surface"
          >
            <input
              type="checkbox"
              class="h-4 w-4 accent-[#4F6BF6]"
              :checked="candidateChecked.has(d.path)"
              :disabled="projectStore.hasProject(d.path)"
              @change="toggleCandidate(d.path)"
            />
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span class="text-sm text-title">{{ d.name }}</span>
                <span v-if="projectStore.hasProject(d.path)" class="text-xs text-muted">
                  已导入
                </span>
                <GitBranch :size="13" class="text-muted" />
                <span class="text-xs text-muted">{{ d.branch }}</span>
              </div>
              <div class="truncate text-xs text-muted" :title="d.path">{{ d.path }}</div>
            </div>
            <span class="shrink-0 text-xs text-muted">{{ fmtDate(d.lastCommitAt) }}</span>
          </label>
        </div>
        <div class="flex justify-end gap-2 border-t border-border px-5 py-3">
          <button
            class="rounded-lg border border-border px-4 py-1.5 text-sm text-text hover:border-primary hover:text-primary"
            @click="importDialogOpen = false"
          >
            取消
          </button>
          <button
            class="rounded-lg bg-primary px-4 py-1.5 text-sm text-white hover:opacity-90"
            @click="confirmImport"
          >
            导入所选
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
