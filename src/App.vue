<script setup lang="ts">
import { ref, onMounted } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import TitleBar from "./components/TitleBar.vue";
import SideNav, { type ViewKey } from "./components/SideNav.vue";
import Toast from "./components/Toast.vue";
import GenerateView from "./views/GenerateView.vue";
import HistoryView from "./views/HistoryView.vue";
import SettingsView from "./views/SettingsView.vue";
import { useSettingStore } from "./stores/settings";
import { useProjectStore } from "./stores/projects";
import { useReportStore } from "./stores/reports";
import { checkGit } from "./api";

// 启动时恢复持久化数据（settings 含异步 keyring 读取）
useSettingStore().load();
useProjectStore().load();
useReportStore().load();

const currentView = ref<ViewKey>("generate");
function setView(key: ViewKey) {
  currentView.value = key;
}

// ===== git 环境检测 =====
const gitReady = ref(true);
const gitChecking = ref(true);
onMounted(async () => {
  // 模拟未安装 git：localStorage 设 gitreport-mock-no-git=1 后重启即可触发
  const mockNoGit = localStorage.getItem("gitreport-mock-no-git") === "1";
  if (mockNoGit) {
    gitReady.value = false;
    gitChecking.value = false;
    return;
  }
  try {
    gitReady.value = await checkGit();
  } catch {
    gitReady.value = false;
  } finally {
    gitChecking.value = false;
  }
});

function downloadGit() {
  openUrl("https://git-scm.com/download/win");
}
</script>

<template>
  <div class="flex h-screen flex-col">
    <TitleBar />
    <div class="flex flex-1 overflow-hidden">
      <SideNav :current="currentView" @change="setView" />
      <main class="flex-1 overflow-y-auto bg-surface">
        <GenerateView v-if="currentView === 'generate'" />
        <HistoryView v-else-if="currentView === 'history'" />
        <SettingsView v-else />
      </main>
    </div>
    <Toast />

    <!-- git 未安装拦截层 -->
    <div
      v-if="!gitChecking && !gitReady"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
    >
      <div class="mx-4 flex w-96 flex-col items-center gap-4 rounded-xl bg-panel p-8 text-center shadow-2xl">
        <div class="text-4xl">⚠️</div>
        <h2 class="text-lg font-semibold text-title">未检测到 Git</h2>
        <p class="text-sm leading-relaxed text-text">
          本应用依赖 Git 来统计提交记录。请先安装 Git 后再使用。
        </p>
        <button
          class="rounded-lg bg-primary px-6 py-2 text-sm text-white hover:opacity-90"
          @click="downloadGit"
        >
          前往下载 Git
        </button>
      </div>
    </div>
  </div>
</template>
