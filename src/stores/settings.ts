import { defineStore } from "pinia";
import { ref, watch } from "vue";
import type { AIConfig, AIIdentity, ReportType, SettingsSection } from "../types";
import { loadApiKey, saveApiKey, deleteApiKey } from "../api";

// localStorage 持久化键（AI 配置只存非敏感字段，apiKey 存系统钥匙串）
const AI_KEY = "gitreport-ai-meta";
const IDENTITIES_KEY = "gitreport-identities";
const TEMPLATES_KEY = "gitreport-templates";
const ACTIVE_ID_KEY = "gitreport-active-identity";

// 三类报告的默认提示词模板
const DEFAULT_TEMPLATES: Record<ReportType, string> = {
  daily:
    "你是一名资深工程师的日报助手。请根据以下 git 提交记录，生成一份结构化的中文工作日报，包含：今日完成（对提交做业务化归纳，不要罗列 commit）；",
  weekly:
    "你是一名资深工程师的周报助手。请根据以下一周的 git 提交记录，生成一份结构化的中文工作周报，包含：一、本周完成；二、下周计划；三、问题与风险。对提交做业务化归纳，突出成果与影响。",
  monthly:
    "你是一名资深工程师的月报助手。请根据以下一个月的 git 提交记录，生成一份结构化的中文工作月报，包含：一、本月工作总结；二、重点成果；三、下月计划。按模块或项目维度归纳，突出业务价值。",
};

/** 从 localStorage 读 JSON，key 不存在或解析失败时返回默认值 */
function read<T>(key: string, fallback: T): T {
  const raw = localStorage.getItem(key);
  if (!raw) return fallback;
  try {
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

export const useSettingStore = defineStore("settings", () => {
  // AI 接口配置（未配置时为 null）
  const ai = ref<AIConfig | null>(null);
  // apiKey 是否已就绪（启动时 keyring 后台回填，回填完成前生成按钮应禁用）
  const keyReady = ref(false);
  // git 身份（name/email 多组，用于识别“自己的提交”）
  const identities = ref<AIIdentity[]>([]);
  // 当前选中的身份下标（用于统计时切换人），-1 表示未选中
  const activeIdentityIndex = ref(-1);
  // 各报告类型的提示词模板
  const templates = ref<Record<ReportType, string>>({ ...DEFAULT_TEMPLATES });
  // 设置页待定位区块（跨视图跳转用：GenerateView 点「设置 git 身份」→ App 切视图 → SettingsView 定位）
  const openSection = ref<SettingsSection | "">("");

  // 状态变化自动写回 localStorage（ai 除外，apiKey 存钥匙串，由 saveAi 单独处理）
  watch(identities, (v) => localStorage.setItem(IDENTITIES_KEY, JSON.stringify(v)), { deep: true });
  watch(activeIdentityIndex, (v) => localStorage.setItem(ACTIVE_ID_KEY, String(v)));
  watch(templates, (v) => localStorage.setItem(TEMPLATES_KEY, JSON.stringify(v)), { deep: true });

  /** 应用启动时从 localStorage + 系统钥匙串恢复 */
  async function load() {
    identities.value = read<AIIdentity[]>(IDENTITIES_KEY, []);
    activeIdentityIndex.value = read<number>(ACTIVE_ID_KEY, -1);
    // 与默认模板合并，缺失的类型回退到默认值
    templates.value = { ...DEFAULT_TEMPLATES, ...read<Partial<Record<ReportType, string>>>(TEMPLATES_KEY, {}) };

    // 旧数据迁移：旧版把完整配置（含明文 apiKey）存在 "gitreport-ai"，
    // 迁移到钥匙串 + "gitreport-ai-meta" 后删除旧明文。
    const legacy = localStorage.getItem("gitreport-ai");
    if (legacy) {
      try {
        const old = JSON.parse(legacy) as AIConfig;
        const { apiKey, ...meta } = old;
        localStorage.setItem(AI_KEY, JSON.stringify(meta));
        if (apiKey) {
          await saveApiKey(apiKey).catch(() => {});
        }
      } catch {
        /* 旧数据损坏则忽略 */
      }
      localStorage.removeItem("gitreport-ai");
    }

    // AI 配置：非敏感字段从 localStorage，apiKey 从系统钥匙串
    const meta = read<Omit<AIConfig, "apiKey"> | null>(AI_KEY, null);
    if (meta) {
      // 先用空 key 立即可用（窗口不因钥匙串读取而等待），后台读取成功后回填
      ai.value = { ...meta, apiKey: "" };
      loadApiKey()
        .then((apiKey) => {
          ai.value = { ...meta, apiKey: apiKey ?? "" };
          keyReady.value = true;
        })
        .catch((e) => {
          console.error("读取 API key 失败：", e);
          keyReady.value = true;
        });
    } else {
      ai.value = null;
      keyReady.value = true;
    }
  }

  /** 保存 AI 接口配置：apiKey 存系统钥匙串，其余字段存 localStorage */
  async function saveAi(cfg: AIConfig) {
    ai.value = cfg;
    const { apiKey, ...meta } = cfg;
    localStorage.setItem(AI_KEY, JSON.stringify(meta));
    if (apiKey) {
      await saveApiKey(apiKey);
    } else {
      // apiKey 清空时删除钥匙串条目
      try {
        await deleteApiKey();
      } catch {
        /* 删除失败不阻断 */
      }
    }
    keyReady.value = true;
  }

  /** 保存 git 身份列表 */
  function saveIdentities(list: AIIdentity[]) {
    identities.value = list;
    // 选中下标越界则重置
    if (activeIdentityIndex.value >= list.length) {
      activeIdentityIndex.value = list.length > 0 ? 0 : -1;
    }
  }

  /** 切换当前选中的身份 */
  function setActiveIdentity(index: number) {
    activeIdentityIndex.value = index;
  }

  /** 更新某类报告的提示词模板 */
  function saveTemplate(type: ReportType, text: string) {
    templates.value[type] = text;
  }

  /** 请求跳转到设置页指定区块（供 GenerateView 无身份时调用） */
  function openSettingsSection(section: SettingsSection) {
    openSection.value = section;
  }

  return { ai, identities, activeIdentityIndex, templates, keyReady, openSection, load, saveAi, saveIdentities, setActiveIdentity, saveTemplate, openSettingsSection };
});
