import { defineStore } from "pinia";
import { ref, watch } from "vue";
import type { ReportRecord, ReportType } from "../types";

const STORAGE_KEY = "gitreport-reports";

/** 从 localStorage 读历史报告，无数据返回空数组 */
function readReports(): ReportRecord[] {
  const raw = localStorage.getItem(STORAGE_KEY);
  if (!raw) return [];
  try {
    return JSON.parse(raw) as ReportRecord[];
  } catch {
    return [];
  }
}

export const useReportStore = defineStore("reports", () => {
  const reports = ref<ReportRecord[]>([]);

  // 状态变化自动持久化（新报告追加、内容编辑均会触发）
  watch(
    reports,
    (v) => localStorage.setItem(STORAGE_KEY, JSON.stringify(v)),
    { deep: true },
  );

  /** 应用启动时从 localStorage 恢复 */
  function load() {
    reports.value = readReports();
  }

  /** 新增一条报告记录 */
  function addReport(r: ReportRecord) {
    reports.value.push(r);
  }

  /** 按类型筛选，generatedAt 倒序（最新在前） */
  function listByType(type: ReportType): ReportRecord[] {
    return reports.value
      .filter((r) => r.type === type)
      .sort((a, b) => b.generatedAt - a.generatedAt);
  }

  return { reports, load, addReport, listByType };
});
