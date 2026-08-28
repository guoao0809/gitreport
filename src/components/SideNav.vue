<script setup lang="ts">
import { Sparkles, History, Settings } from "lucide-vue-next";

export type ViewKey = "generate" | "history" | "settings";

defineProps<{ current: ViewKey }>();
defineEmits<{ change: [key: ViewKey] }>();

const items: { key: ViewKey; label: string; icon: typeof Sparkles }[] = [
  { key: "generate", label: "生成", icon: Sparkles },
  { key: "history", label: "历史", icon: History },
  { key: "settings", label: "设置", icon: Settings },
];
</script>

<template>
  <nav class="flex w-14 shrink-0 flex-col items-center bg-[#1F2329]">
    <button
      v-for="item in items"
      :key="item.key"
      :title="item.label"
      class="mb-1 flex h-10 w-10 items-center justify-center rounded-lg transition-colors"
      :class="
        current === item.key
          ? 'bg-primary text-white'
          : 'text-gray-400 hover:bg-white/10 hover:text-white'
      "
      @click="$emit('change', item.key)"
    >
      <component :is="item.icon" :size="18" />
    </button>
    <div class="mt-auto text-[10px] text-gray-600">v1.0.0</div>
  </nav>
</template>
