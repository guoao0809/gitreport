<script setup lang="ts">
import { computed, ref, watch } from "vue";
import MarkdownIt from "markdown-it";
import DOMPurify from "dompurify";
import { useClipboard } from "@vueuse/core";
import { Copy, FileText } from "lucide-vue-next";
import type { ReportRecord, ReportType } from "../types";
import { useReportStore } from "../stores/reports";
import { showToast } from "../components/toast";
import Tabs from "../components/Tabs.vue";

const reportStore = useReportStore();

const type = ref<ReportType>("daily");
const tabs: { value: ReportType; label: string }[] = [
  { value: "daily", label: "日报" },
  { value: "weekly", label: "周报" },
  { value: "monthly", label: "月报" },
];
// 切换类型时清空当前查看的报告
watch(type, () => {
  activeId.value = "";
});

const list = computed<ReportRecord[]>(() => reportStore.listByType(type.value));

// 当前展开查看的报告
const activeId = ref<string>("");
const active = computed<ReportRecord | null>(
  () => list.value.find((r) => r.id === activeId.value) ?? null,
);

const md = new MarkdownIt({ breaks: true, linkify: true });
const renderedHtml = computed(() =>
  active.value ? DOMPurify.sanitize(md.render(active.value.content)) : "",
);

const { copy } = useClipboard({ legacy: true });
async function copyActive() {
  if (!active.value) return;
  await copy(active.value.content);
  showToast("已复制", "success");
}

function fmtGenerated(ts: number): string {
  const d = new Date(ts);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day} ${hh}:${mm}`;
}
</script>

<template>
  <div class="flex h-full flex-col gap-4 p-4">
    <!-- 顶部 Tab -->
    <Tabs v-model="type" :tabs="tabs" class="self-start">
      <template #default="{ tab }">
        {{ tab.label }}
        <span class="ml-1 text-xs opacity-70">（{{ reportStore.listByType(tab.value).length }}）</span>
      </template>
    </Tabs>

    <div class="flex min-h-0 flex-1 gap-4">
      <!-- 左：历史列表 -->
      <div class="flex w-72 shrink-0 flex-col rounded-xl border border-border bg-panel">
        <div class="border-b border-border px-4 py-3 text-sm font-medium text-title">
          {{ tabs.find((t) => t.value === type)?.label }}历史
        </div>
        <div class="flex-1 overflow-y-auto p-2">
          <div v-if="list.length === 0" class="py-10 text-center text-sm text-muted">
            暂无历史报告
          </div>
          <button
            v-for="r in list"
            :key="r.id"
            class="mb-1 flex w-full flex-col gap-0.5 rounded-lg px-3 py-2 text-left transition-colors"
            :class="activeId === r.id ? 'bg-primary/10' : 'hover:bg-surface'"
            @click="activeId = r.id"
          >
            <span class="text-sm" :class="activeId === r.id ? 'text-primary' : 'text-title'">
              {{ r.dateRange }}
            </span>
            <span class="text-xs text-muted">{{ fmtGenerated(r.generatedAt) }}</span>
          </button>
        </div>
      </div>

      <!-- 右：报告内容 -->
      <div class="flex min-w-0 flex-1 flex-col rounded-xl border border-border bg-panel">
        <div class="flex items-center justify-between border-b border-border px-4 py-2.5">
          <span class="text-sm text-text">
            {{ active ? active.dateRange : "选择左侧报告查看" }}
          </span>
          <button
            v-if="active"
            class="flex items-center gap-1 rounded-lg border border-border px-2.5 py-1.5 text-xs text-text hover:border-primary hover:text-primary"
            @click="copyActive"
          >
            <Copy :size="13" />
            复制全文
          </button>
        </div>
        <div class="min-h-0 flex-1 overflow-y-auto p-4">
          <div
            v-if="!active"
            class="flex h-full flex-col items-center justify-center gap-3 text-sm text-muted"
          >
            <FileText :size="28" class="text-muted" />
            从左侧选择一份历史报告查看
          </div>
          <div v-else class="markdown-body" v-html="renderedHtml"></div>
        </div>
      </div>
    </div>
  </div>
</template>
