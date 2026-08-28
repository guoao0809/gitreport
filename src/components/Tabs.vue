<script setup lang="ts" generic="T extends string">
import { computed, nextTick, onMounted, ref, watch } from "vue";

const props = defineProps<{
  tabs: { label: string; value: T }[];
  modelValue: T;
  size?: "md" | "sm";
  disabled?: boolean;
}>();
const emit = defineEmits<{ "update:modelValue": [value: T] }>();

const wrapRef = ref<HTMLElement | null>(null);
const style = ref({ left: "0px", width: "0px" });

const pad = computed(() => (props.size === "sm" ? "px-3 py-1 text-xs" : "px-4 py-1.5 text-sm"));

/** 让滑块吸附到当前选中 tab 的实际位置/宽度（内容自适应不换行） */
function measure() {
  const active = wrapRef.value?.querySelector<HTMLElement>("[data-active='true']");
  if (!active) return;
  style.value = { left: `${active.offsetLeft}px`, width: `${active.offsetWidth}px` };
}

watch(
  () => props.modelValue,
  () => nextTick(measure),
  { flush: "post" },
);
onMounted(measure);
</script>

<template>
  <div
    ref="wrapRef"
    class="relative flex overflow-hidden rounded-lg border border-border"
  >
    <span
      class="absolute top-0 bottom-0 bg-primary transition-[left,width] duration-200 ease-out"
      :style="style"
    ></span>
    <button
      v-for="t in tabs"
      :key="t.value"
      class="relative z-10 shrink-0 whitespace-nowrap transition-colors disabled:cursor-not-allowed disabled:opacity-50"
      :class="[pad, t.value === modelValue ? 'text-white' : 'text-text hover:bg-surface']"
      :data-active="t.value === modelValue"
      :disabled="disabled"
      @click="emit('update:modelValue', t.value)"
    >
      <slot :tab="t">{{ t.label }}</slot>
    </button>
  </div>
</template>
