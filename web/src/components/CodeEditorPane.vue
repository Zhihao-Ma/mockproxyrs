<script setup lang="ts">
import { computed, ref } from "vue";

/**
 * 代码编辑器面板（行号 + 文本区）
 *
 * 纯展示/输入层，不含工具栏与校验逻辑，由 CodeEditor 在内联与弹窗中复用。
 * 行号槽与文本区共用同一套字号 / 行高 / 上下内边距，并通过 scrollTop 同步滚动，
 * 保证行号与文本逐行对齐。
 */

const props = withDefaults(
  defineProps<{
    /** 绑定值（v-model） */
    modelValue: string;
    /** 占位提示 */
    placeholder?: string;
    /** 内联模式下的可见行数（仅 inline 生效） */
    rows?: number;
    /** 弹窗模式下的固定高度，如 "70vh"（提供后忽略 rows） */
    height?: string;
    /** 撑满父容器高度（flex 布局下使用，提供后忽略 rows） */
    fill?: boolean;
    /** 是否只读 */
    readonly?: boolean;
    /** 是否处于错误态（高亮边框） */
    error?: boolean;
    /** 是否显示行号（内联预览截断时建议关闭，避免行号失真） */
    showGutter?: boolean;
  }>(),
  {
    placeholder: "",
    rows: 8,
    height: undefined,
    fill: false,
    readonly: false,
    error: false,
    showGutter: true,
  }
);

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
  (e: "change", value: string): void;
  (e: "blur"): void;
}>();

const textareaRef = ref<HTMLTextAreaElement | null>(null);
const gutterRef = ref<HTMLDivElement | null>(null);

const lineCount = computed(() => {
  if (!props.modelValue) return 1;
  return Math.max(props.modelValue.split("\n").length, 1);
});

const lineNumbers = computed(() => {
  const arr: number[] = [];
  for (let i = 1; i <= lineCount.value; i++) arr.push(i);
  return arr;
});

/**
 * 文本区滚动时同步行号。
 * 行号槽始终保持 overflow: hidden（不出现独立滚动条），
 * 直接写 scrollTop 仍会移动其内容，从而与文本区对齐。
 */
function onScroll() {
  if (gutterRef.value && textareaRef.value) {
    gutterRef.value.scrollTop = textareaRef.value.scrollTop;
  }
}

function onInput(e: Event) {
  const val = (e.target as HTMLTextAreaElement).value;
  emit("update:modelValue", val);
  emit("change", val);
}

defineExpose({ textareaRef });
</script>

<template>
  <div class="pane" :class="{ 'is-flex-height': !!height || fill }">
    <div v-if="showGutter" ref="gutterRef" class="pane__gutter" aria-hidden="true">
      <span v-for="n in lineNumbers" :key="n" class="pane__lineno">{{ n }}</span>
    </div>
    <textarea
      ref="textareaRef"
      class="pane__textarea"
      :class="{ 'is-error': error }"
      :style="height ? { height } : undefined"
      :rows="height || fill ? undefined : rows"
      :value="modelValue"
      :readonly="readonly"
      :placeholder="placeholder"
      spellcheck="false"
      @input="onInput"
      @scroll="onScroll"
      @blur="emit('blur')"
    />
  </div>
</template>

<style scoped lang="scss">
/* 行号与文本共用同一字号 / 行高 / 上下内边距，确保逐行对齐 */
$line-font-size: 13px;
$line-height: 1.6;
$pad-y: 12px;

.pane {
  display: flex;
  border: 1px solid #e8e8ed;
  border-radius: 8px;
  overflow: hidden;
  background: #fff;
  transition: border-color 0.2s ease;
}

.pane:focus-within {
  border-color: #667eea;
}

.pane__gutter {
  display: flex;
  flex-direction: column;
  padding: $pad-y 10px;
  background: #fafafa;
  color: #c0c0c5;
  font-family: "SF Mono", Monaco, Consolas, monospace;
  font-size: $line-font-size;
  line-height: $line-height;
  text-align: right;
  user-select: none;
  /* 始终隐藏自身滚动条，靠 scrollTop 同步移动内容 */
  overflow: hidden;
  flex-shrink: 0;
}

.pane__lineno {
  display: block;
  height: calc(#{$line-font-size} * #{$line-height});
  line-height: $line-height;
}

.pane__textarea {
  flex: 1;
  padding: $pad-y 14px;
  border: none;
  outline: none;
  resize: vertical;
  font-family: "SF Mono", Monaco, Consolas, monospace;
  font-size: $line-font-size;
  line-height: $line-height;
  color: #1d1d1f;
  background: transparent;
  white-space: pre;
  overflow: auto;
  tab-size: 2;
}

.pane__textarea::placeholder {
  color: #c0c0c5;
  font-family: "SF Mono", Monaco, Consolas, monospace;
}

.pane__textarea.is-error {
  background: rgba(245, 108, 108, 0.04);
}

/* 弹窗模式：让面板与文本区填满高度，禁用手动 resize */
.pane.is-flex-height {
  flex: 1;
  min-height: 0;
}

.pane.is-flex-height .pane__textarea {
  resize: none;
  height: 100%;
}
</style>
