<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from "vue";
import { Eye, EyeOff, Plus, Trash2, RotateCcw, Bot, UserRound, FileCode2, Download, RefreshCw, ChevronDown } from "lucide-vue-next";
import type { AIConfig, AIProtocol, ReportType, SettingsSection } from "../types";
import { TEMPLATE_VARS } from "../types";
import { testAiConnection, getGitIdentity, listModels } from "../api";
import { useSettingStore } from "../stores/settings";
import { showToast } from "../components/toast";
import Tabs from "../components/Tabs.vue";
import { ComboboxRoot, ComboboxAnchor, ComboboxInput, ComboboxTrigger, ComboboxPortal, ComboboxContent, ComboboxViewport, ComboboxItem, ComboboxEmpty } from "reka-ui";

const settingStore = useSettingStore();

type SectionKey = SettingsSection;
const section = ref<SettingsSection>("ai");

const sections: { key: SectionKey; label: string; icon: typeof Bot }[] = [
  { key: "ai", label: "AI 模型", icon: Bot },
  { key: "identities", label: "Git 身份", icon: UserRound },
  { key: "templates", label: "提示词模板", icon: FileCode2 },
];

// 跨视图跳转定位：GenerateView 请求打开某区块时，切到该区块并清空请求
watch(
  () => settingStore.openSection,
  (s) => {
    if (s) {
      section.value = s;
      settingStore.openSection = "";
    }
  },
);

// ===== AI 模型 =====
const form = reactive<AIConfig>({
  protocol: "openai",
  baseUrl: "",
  model: "",
  apiKey: "",
});
// store 异步加载完成后（或后续变化时）同步到表单；仅首次有值或值变化时覆盖
watch(
  () => settingStore.ai,
  (ai) => {
    if (ai) Object.assign(form, ai);
  },
  { immediate: true },
);
const showKey = ref(false);
const testing = ref(false);
const testResult = ref<{ ok: boolean; elapsed: string } | null>(null);

// 获取模型列表
const models = ref<string[]>([]);
const listLoading = ref(false);
const modelOpen = ref(false);
/** reka-ui 内建过滤：模型名包含输入关键字 */
function modelFilter(value: string, search: string) {
  return value.toLowerCase().includes(search.toLowerCase());
}
async function fetchModels() {
  if (!form.baseUrl || !form.apiKey) {
    showToast("请先填写 Base URL 和 API Key", "error");
    return;
  }
  listLoading.value = true;
  try {
    const list = await listModels({ ...form });
    models.value = list;
    showToast(`获取到 ${list.length} 个模型`, "success");
  } catch (e) {
    models.value = [];
    showToast(`获取模型失败：${e}`, "error");
  } finally {
    listLoading.value = false;
  }
}

function pickProtocol(p: AIProtocol) {
  form.protocol = p;
}

async function testConnection() {
  if (!form.baseUrl || !form.model || !form.apiKey) {
    showToast("请先填写完整配置", "error");
    return;
  }
  testing.value = true;
  testResult.value = null;
  try {
    const elapsed = await testAiConnection({ ...form });
    testResult.value = { ok: true, elapsed };
  } catch (e) {
    testResult.value = { ok: false, elapsed: String(e) };
  } finally {
    testing.value = false;
  }
}

async function saveAi() {
  if (!form.baseUrl || !form.model || !form.apiKey) {
    showToast("请先填写完整配置", "error");
    return;
  }
  try {
    await settingStore.saveAi({ ...form });
    showToast("配置已保存", "success");
  } catch (e) {
    showToast(`保存失败：${e}`, "error");
  }
}

// ===== Git 身份 =====
const identities = reactive(
  settingStore.identities.length > 0
    ? settingStore.identities.map((i) => ({ ...i }))
    : [{ name: "", email: "" }],
);

function addIdentity() {
  identities.push({ name: "", email: "" });
}
function removeIdentity(i: number) {
  identities.splice(i, 1);
}
const identityErrors = reactive<boolean[]>([]);
/** 失焦校验：只填了姓名或邮箱其中一个时标红并提示 */
function validateIdentity(i: number) {
  const ident = identities[i];
  if (!ident) return;
  const hasName = !!ident.name.trim();
  const hasEmail = !!ident.email.trim();
  identityErrors[i] = hasName !== hasEmail;
  if (identityErrors[i]) showToast("每组身份需要同时填写姓名和邮箱", "error");
}
// 输入即自动保存：过滤掉完全为空的行，其余 trim 后写回 store
watch(
  identities,
  () => {
    const valid = identities.filter((i) => i.name.trim() || i.email.trim());
    settingStore.saveIdentities(valid.map((i) => ({ name: i.name.trim(), email: i.email.trim() })));
  },
  { deep: true },
);

// 切出 Git 身份页时，删除未填写完整的行（姓名或邮箱任一为空）
watch(section, (newVal, oldVal) => {
  if (oldVal !== "identities" || newVal === "identities") return;
  const cleaned = identities.filter((i) => i.name.trim() && i.email.trim());
  if (cleaned.length === identities.length) return; // 无残行
  identities.splice(0, identities.length, ...cleaned);
  // 清空后保留一个空输入框，方便下次编辑
  if (cleaned.length === 0) identities.push({ name: "", email: "" });
});

/** 从本机 git config 读取当前身份（--global / --system，无需导入项目） */
const readingIdentity = ref(false);
async function readFromGitConfig() {
  readingIdentity.value = true;
  try {
    // 传空 path：不依赖未导入的项目，直接读本机 git config
    const ident = await getGitIdentity("");
    // 已存在完全相同 → 复用；否则优先填充空白项，再否则追加
    let idx = identities.findIndex((i) => i.name === ident.name && i.email === ident.email);
    if (idx < 0) {
      const blank = identities.findIndex((i) => !i.name.trim() && !i.email.trim());
      if (blank >= 0) {
        identities[blank] = { ...ident };
        idx = blank;
      } else {
        identities.push({ ...ident });
        idx = identities.length - 1;
      }
    }
    settingStore.saveIdentities(identities.map((i) => ({ ...i })));
    settingStore.setActiveIdentity(idx);
    showToast(`已读取 git 身份：${ident.name} <${ident.email}>`, "success");
  } catch (e) {
    showToast(`读取 git 配置失败：${e}`, "error");
  } finally {
    readingIdentity.value = false;
  }
}

// ===== 提示词模板 =====
const tplType = ref<ReportType>("daily");
const tplTexts = reactive<Record<ReportType, string>>({
  daily: settingStore.templates.daily,
  weekly: settingStore.templates.weekly,
  monthly: settingStore.templates.monthly,
});
const tplText = computed({
  get: () => tplTexts[tplType.value],
  set: (v: string) => {
    tplTexts[tplType.value] = v;
  },
});

const tplTabs: { value: ReportType; label: string }[] = [
  { value: "daily", label: "日报模板" },
  { value: "weekly", label: "周报模板" },
  { value: "monthly", label: "月报模板" },
];

const textareaRef = ref<HTMLTextAreaElement | null>(null);
function insertVar(key: string) {
  const ta = textareaRef.value;
  if (!ta) return;
  const start = ta.selectionStart;
  const end = ta.selectionEnd;
  const text = tplTexts[tplType.value];
  tplTexts[tplType.value] = text.slice(0, start) + key + text.slice(end);
  nextTick(() => {
    ta.focus();
    ta.selectionStart = ta.selectionEnd = start + key.length;
  });
}

function restoreDefault() {
  // 恢复为空串表示「回到默认」？——不，模板 store 无重置接口，恢复出厂用空串占位
  // ponytail: saveTemplate 写入的是字符串，无默认值接口；这里清空让用户自行填写或查看文档
  tplTexts[tplType.value] = "";
  showToast("已清空，保存后生效", "info");
}

function saveTemplate() {
  if (!tplTexts[tplType.value].trim()) {
    showToast("模板内容不能为空", "error");
    return;
  }
  settingStore.saveTemplate(tplType.value, tplTexts[tplType.value]);
  showToast("模板已保存", "success");
}
</script>

<template>
  <div class="flex h-full">
    <!-- 左侧二级导航 -->
    <div class="w-44 shrink-0 border-r border-border bg-panel p-3">
      <button
        v-for="s in sections"
        :key="s.key"
        class="mb-1 flex w-full items-center gap-2 rounded-lg px-3 py-2 text-sm transition-colors"
        :class="
          section === s.key
            ? 'bg-primary/10 text-primary font-medium'
            : 'text-text hover:bg-surface'
        "
        @click="section = s.key"
      >
        <component :is="s.icon" :size="15" />
        {{ s.label }}
      </button>
    </div>

    <!-- 右侧内容 -->
    <div class="flex-1 overflow-y-auto p-6">
      <!-- AI 模型 -->
      <div v-if="section === 'ai'" class="mx-auto max-w-xl">
        <h2 class="mb-5 text-lg font-semibold text-title">AI 模型</h2>

        <div class="mb-4">
          <label class="mb-1.5 block text-sm text-text">接口协议</label>
          <div class="flex gap-4">
            <label class="flex cursor-pointer items-center gap-1.5 text-sm text-text">
              <input
                type="radio"
                value="openai"
                class="accent-[#4F6BF6]"
                :checked="form.protocol === 'openai'"
                @change="pickProtocol('openai')"
              />
              OpenAI 兼容
            </label>
            <label class="flex cursor-pointer items-center gap-1.5 text-sm text-text">
              <input
                type="radio"
                value="anthropic"
                class="accent-[#4F6BF6]"
                :checked="form.protocol === 'anthropic'"
                @change="pickProtocol('anthropic')"
              />
              Anthropic
            </label>
          </div>
        </div>

        <div class="mb-4">
          <label class="mb-1.5 block text-sm text-text">Base URL</label>
          <input
            v-model="form.baseUrl"
            type="text"
            placeholder="https://api.openai.com/v1"
            class="w-full rounded-lg border border-border px-3 py-2 text-sm text-title outline-none focus:border-primary"
          />
        </div>

        <div class="mb-4">
          <label class="mb-1.5 block text-sm text-text">模型名称</label>
          <div class="flex gap-2">
            <ComboboxRoot
              v-model="form.model"
              :filter-function="modelFilter"
              :open="modelOpen"
              @update:open="(v) => (modelOpen = v)"
              class="min-w-0 flex-1"
            >
              <ComboboxAnchor
                class="flex min-w-0 flex-1 cursor-pointer items-center gap-1 rounded-lg border border-border px-3 py-2 focus-within:border-primary"
              >
                <ComboboxInput
                  class="min-w-0 flex-1 bg-transparent text-sm text-title outline-none placeholder:text-muted"
                  placeholder="gpt-4o-mini / claude-sonnet-4-5 等"
                  @focus="modelOpen = true"
                  @click="modelOpen = true"
                />
                <ComboboxTrigger v-if="models.length > 0" class="text-muted">
                  <ChevronDown :size="14" />
                </ComboboxTrigger>
              </ComboboxAnchor>
              <ComboboxPortal>
                <ComboboxContent
                  position="popper"
                  align="start"
                  side="bottom"
                  :side-offset="6"
                  class="model-picker-popup z-30 max-h-72 overflow-y-auto rounded-lg border border-border bg-panel shadow-lg"
                >
                  <ComboboxViewport class="p-1">
                    <ComboboxItem
                      v-for="m in models"
                      :key="m"
                      :value="m"
                      class="cursor-pointer rounded-md px-3 py-2 text-left text-sm text-text outline-none data-highlighted:bg-surface data-highlighted:text-title data-state-checked:text-primary"
                    >
                      {{ m }}
                    </ComboboxItem>
                    <ComboboxEmpty class="px-3 py-2 text-sm text-muted">
                      暂无模型，先点「获取模型」
                    </ComboboxEmpty>
                  </ComboboxViewport>
                </ComboboxContent>
              </ComboboxPortal>
            </ComboboxRoot>
            <button
              class="flex shrink-0 items-center gap-1 rounded-lg border border-border px-3 py-2 text-sm text-text hover:border-primary hover:text-primary disabled:opacity-50"
              :disabled="listLoading"
              @click="fetchModels"
            >
              <RefreshCw :size="14" :class="listLoading ? 'animate-spin' : ''" />
              {{ listLoading ? "获取中…" : "获取模型" }}
            </button>
          </div>
        </div>

        <div class="mb-4">
          <label class="mb-1.5 block text-sm text-text">API Key</label>
          <div class="relative">
            <input
              v-model="form.apiKey"
              :type="showKey ? 'text' : 'password'"
              placeholder="sk-..."
              class="w-full rounded-lg border border-border px-3 py-2 pr-10 text-sm text-title outline-none focus:border-primary"
            />
            <button
              class="absolute top-1/2 right-2 -translate-y-1/2 text-muted hover:text-text"
              :title="showKey ? '隐藏' : '显示'"
              @click="showKey = !showKey"
            >
              <Eye v-if="!showKey" :size="15" />
              <EyeOff v-else :size="15" />
            </button>
          </div>
        </div>

        <div v-if="testResult" class="mb-4 text-sm" :class="testResult.ok ? 'text-green-600' : 'text-red-500'">
          <template v-if="testResult.ok">✓ 连接成功，耗时 {{ testResult.elapsed }}</template>
          <template v-else>✗ 连接失败：{{ testResult.elapsed }}</template>
        </div>

        <div class="flex gap-3">
          <button
            class="rounded-lg border border-border px-4 py-2 text-sm text-text hover:border-primary hover:text-primary disabled:opacity-50"
            :disabled="testing"
            @click="testConnection"
          >
            {{ testing ? "测试中…" : "测试连接" }}
          </button>
          <button
            class="rounded-lg bg-primary px-4 py-2 text-sm text-white hover:opacity-90"
            @click="saveAi"
          >
            保存配置
          </button>
        </div>
      </div>

      <!-- Git 身份 -->
      <div v-else-if="section === 'identities'" class="mx-auto max-w-xl">
        <div class="mb-5 flex items-center justify-between">
          <h2 class="text-lg font-semibold text-title">Git 身份</h2>
          <div class="flex gap-2">
            <button
              class="flex items-center gap-1 rounded-lg border border-border px-3 py-1.5 text-sm text-text hover:border-primary hover:text-primary disabled:opacity-50"
              :disabled="readingIdentity"
              @click="readFromGitConfig"
            >
              <Download :size="14" :class="readingIdentity ? 'animate-spin' : ''" />
              {{ readingIdentity ? "读取中…" : "从 git config 读取" }}
            </button>
            <button
              class="flex items-center gap-1 rounded-lg border border-border px-3 py-1.5 text-sm text-text hover:border-primary hover:text-primary"
              @click="addIdentity"
            >
              <Plus :size="14" />
              添加身份
            </button>
          </div>
        </div>
        <p class="mb-4 text-xs text-muted">
          用于识别「自己的提交」，点击身份卡片可切换当前统计的人。推荐用「从 git config 读取」自动填充，
          无需手动输入；也可手动新增他人身份。
        </p>

        <div
          v-for="(ident, i) in identities"
          :key="i"
          class="mb-3 flex items-start gap-2 rounded-xl border p-3 transition-colors"
          :class="
            settingStore.activeIdentityIndex === i
              ? 'border-primary bg-primary/5'
              : 'border-border bg-panel'
          "
        >
          <div class="flex flex-1 gap-3">
            <button
              class="mt-1.5 flex h-4 w-4 shrink-0 items-center justify-center rounded-full border"
              :class="
                settingStore.activeIdentityIndex === i ? 'border-primary' : 'border-border'
              "
              title="设为当前统计身份"
              @click="settingStore.setActiveIdentity(i)"
            >
              <span
                v-if="settingStore.activeIdentityIndex === i"
                class="h-2 w-2 rounded-full bg-primary"
              ></span>
            </button>
            <div class="flex-1">
              <label class="mb-1 block text-xs text-muted">姓名</label>
              <input
                v-model="ident.name"
                type="text"
                placeholder="如：zhangsan"
                @blur="validateIdentity(i)"
                @input="identityErrors[i] = false"
                class="w-full rounded-lg border px-3 py-1.5 text-sm text-title outline-none focus:border-primary"
                :class="identityErrors[i] ? 'border-red-500' : 'border-border'"
              />
            </div>
            <div class="flex-1">
              <label class="mb-1 block text-xs text-muted">邮箱</label>
              <input
                v-model="ident.email"
                type="text"
                placeholder="如：zhangsan@example.com"
                @blur="validateIdentity(i)"
                @input="identityErrors[i] = false"
                class="w-full rounded-lg border px-3 py-1.5 text-sm text-title outline-none focus:border-primary"
                :class="identityErrors[i] ? 'border-red-500' : 'border-border'"
              />
            </div>
          </div>
          <button
            class="mt-6 rounded-lg p-1.5 text-muted hover:bg-red-50 hover:text-red-500"
            title="删除"
            :disabled="identities.length <= 1"
            @click="removeIdentity(i)"
          >
            <Trash2 :size="15" />
          </button>
        </div>
      </div>

      <!-- 提示词模板 -->
      <div v-else class="mx-auto flex max-w-4xl gap-6">
        <div class="flex-1">
          <h2 class="mb-5 text-lg font-semibold text-title">提示词模板</h2>
          <Tabs v-model="tplType" :tabs="tplTabs" class="mb-3" />
          <textarea
            ref="textareaRef"
            v-model="tplText"
            class="h-[420px] w-full resize-none rounded-xl border border-border p-4 font-mono text-xs leading-relaxed text-title outline-none focus:border-primary"
            placeholder="输入提示词模板，用下方变量插入动态内容…"
          ></textarea>
          <div class="mt-3 flex gap-3">
            <button
              class="flex items-center gap-1 rounded-lg border border-border px-4 py-2 text-sm text-text hover:border-primary hover:text-primary"
              @click="restoreDefault"
            >
              <RotateCcw :size="14" />
              恢复默认
            </button>
            <button
              class="rounded-lg bg-primary px-4 py-2 text-sm text-white hover:opacity-90"
              @click="saveTemplate"
            >
              保存模板
            </button>
          </div>
        </div>

        <!-- 右侧变量帮助 -->
        <div class="w-56 shrink-0 rounded-xl border border-border bg-panel p-4">
          <div class="mb-3 text-sm font-medium text-title">可用变量</div>
          <div
            v-for="v in TEMPLATE_VARS"
            :key="v.key"
            class="mb-3 rounded-lg border border-border p-2.5"
          >
            <div class="font-mono text-xs text-primary">{{ v.key }}</div>
            <div class="mt-0.5 flex items-center justify-between">
              <span class="text-xs text-muted">{{ v.label }}</span>
              <button
                class="text-xs text-primary hover:underline"
                @click="insertVar(v.key)"
              >
                插入
              </button>
            </div>
          </div>
          <p class="mt-2 text-xs leading-relaxed text-muted">
            变量在生成时被替换为实际内容；模板中未使用的变量不会被替换。
          </p>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 模型下拉弹层与输入框等宽：reka-ui 在 content 上暴露 --reka-popper-anchor-width */
.model-picker-popup {
  min-width: var(--reka-popper-anchor-width);
}
</style>
