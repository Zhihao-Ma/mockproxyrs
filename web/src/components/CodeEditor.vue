<script setup lang="ts">
import { computed, ref } from "vue";
import CodeEditorPane from "@/components/CodeEditorPane.vue";

/**
 * 代码编辑器组件
 *
 * 内联区域为只读预览，点击任意位置即弹出大尺寸编辑弹窗；
 * 格式化、校验、行号、错误展示等高级能力均在弹窗内完成。
 * 通过 props 注入「格式化」与「校验」回调，由父组件提供具体逻辑，
 * 从而保持编辑器本身与业务（JSON / JS 脚本）解耦。
 *
 * 内联预览固定高度，且只渲染末尾若干行（预览最新内容），
 * 完整内容在弹窗中编辑。
 */

const props = withDefaults(
  defineProps<{
    /** 绑定值（v-model） */
    modelValue: string;
    /** 语言类型，仅用于占位与语义说明 */
    language?: "json" | "javascript";
    /** 标签文案 */
    label?: string;
    /** 占位提示 */
    placeholder?: string;
    /** 内联预览的可见行数（固定高度） */
    rows?: number;
    /** 内联预览最多渲染的末尾行数，0 表示不截断 */
    previewLines?: number;
    /** 是否可格式化（不提供 formatter 时隐藏按钮） */
    formatter?: (source: string) => Promise<string>;
    /** 是否可校验（不提供 validator 时隐藏按钮） */
    validator?: (source: string) => Promise<void>;
    /** 关闭弹窗时自动格式化（仅当提供了 formatter 时生效） */
    formatOnClose?: boolean;
    /** 校验按钮文案 */
    validateText?: string;
    /** 格式化按钮文案 */
    formatText?: string;
    /** 弹窗标题 */
    dialogTitle?: string;
    /** 内联预览的空状态提示 */
    emptyText?: string;
    /** 标题旁帮助按钮的回调；传入则显示问号 icon，点击触发 */
    help?: () => void;
  }>(),
  {
    language: "json",
    label: "",
    placeholder: "",
    rows: 6,
    previewLines: 0,
    formatter: undefined,
    validator: undefined,
    formatOnClose: false,
    validateText: "校验",
    formatText: "格式化",
    dialogTitle: "编辑",
    emptyText: "点击编辑",
    help: undefined,
  }
);

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
  /** 弹窗关闭时触发，父组件可据此保存 */
  (e: "blur"): void;
  /** 内容被内部动作（校验/格式化）改写后触发，父组件应据此持久化 */
  (e: "save"): void;
}>();

/**
 * 内联预览文本：仅保留末尾 previewLines 行，聚焦最新内容；
 * previewLines 为 0 时不截断，渲染全部内容。
 */
const previewValue = computed(() => {
  if (!props.modelValue || !props.previewLines) return props.modelValue;
  const lines = props.modelValue.split("\n");
  if (lines.length <= props.previewLines) return props.modelValue;
  return lines.slice(lines.length - props.previewLines).join("\n");
});

/** 校验/格式化错误信息 */
const error = ref("");
/** 按钮加载态 */
const validating = ref(false);
const formatting = ref(false);
/** 「已保存」提示态 */
const saved = ref(false);
let savedTimer: ReturnType<typeof setTimeout> | null = null;

/** 弹窗显隐 */
const dialogVisible = ref(false);

function setValue(val: string) {
  emit("update:modelValue", val);
  // 内容变更后清除上一次校验错误
  if (error.value) error.value = "";
}

async function format() {
  if (!props.formatter || formatting.value) return;
  formatting.value = true;
  error.value = "";
  try {
    const formatted = await props.formatter(props.modelValue);
    emit("update:modelValue", formatted);
    emit("save");
    markSaved();
  } catch (e) {
    error.value = String(e);
  } finally {
    formatting.value = false;
  }
}

async function validate() {
  if (!props.validator || validating.value) return;
  validating.value = true;
  error.value = "";
  try {
    await props.validator(props.modelValue);
    // 校验通过后顺带格式化，保持与历史交互一致（校验 = 校验 + 格式化）
    if (props.formatter) {
      const formatted = await props.formatter(props.modelValue);
      emit("update:modelValue", formatted);
    }
    emit("save");
    markSaved();
  } catch (e) {
    error.value = String(e);
  } finally {
    validating.value = false;
  }
}

/**
 * 弹窗关闭：按需自动格式化，并统一触发失焦保存。
 */
async function onDialogClosed() {
  if (props.formatOnClose && props.formatter) {
    await format();
  }
  emit("blur");
}

/** 触发保存提示态（校验/格式化成功后调用） */
function markSaved() {
  saved.value = true;
  if (savedTimer) clearTimeout(savedTimer);
  savedTimer = setTimeout(() => {
    saved.value = false;
  }, 1500);
}

defineExpose({ format, validate, markSaved });
</script>

<template>
  <div class="code-editor">
    <div v-if="label" class="code-editor__header">
      <label class="code-editor__label">{{ label }}</label>
      <span v-if="saved" class="code-editor__saved">已保存</span>
    </div>

    <!-- 内联只读预览，点击弹出大编辑器 -->
    <div
      class="code-editor__preview"
      :class="{ 'is-error': !!error, 'is-empty': !modelValue }"
      role="button"
      tabindex="0"
      @click="dialogVisible = true"
      @keydown.enter.prevent="dialogVisible = true"
      @keydown.space.prevent="dialogVisible = true"
    >
      <CodeEditorPane
        v-if="modelValue"
        :model-value="previewValue"
        :rows="rows"
        readonly
        :show-gutter="false"
        :error="!!error"
      />
      <div v-else class="code-editor__empty">
        {{ placeholder || emptyText }}
      </div>
      <span class="code-editor__expand-hint">点击放大编辑</span>
    </div>

    <div v-if="error" class="code-editor__error">{{ error }}</div>

    <el-dialog
      v-model="dialogVisible"
      width="80%"
      class="code-editor__dialog"
      append-to-body
      destroy-on-close
      @closed="onDialogClosed"
    >
      <template #header>
        <div class="code-editor__dialog-title">
          <span class="el-dialog__title">{{ dialogTitle || label }}</span>
          <span
            v-if="help"
            class="help-icon"
            title="使用说明"
            @click="help"
          >?</span>
        </div>
      </template>
      <div class="code-editor__dialog-toolbar">
        <el-button
          v-if="validator !== undefined"
          size="small"
          :loading="validating"
          :disabled="!modelValue"
          @click="validate"
        >
          {{ validateText }}
        </el-button>
        <el-button
          v-if="formatter !== undefined"
          size="small"
          :loading="formatting"
          :disabled="!modelValue"
          @click="format"
        >
          {{ formatText }}
        </el-button>
      </div>
      <CodeEditorPane
        class="code-editor__dialog-pane"
        :model-value="modelValue"
        fill
        :placeholder="placeholder"
        :error="!!error"
        @update:model-value="setValue"
      />
      <div v-if="error" class="code-editor__error">{{ error }}</div>
      <template #footer>
        <el-button @click="dialogVisible = false">完成</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped lang="scss">
.code-editor {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 100%;
}

.code-editor__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.code-editor__label {
  font-size: 13px;
  font-weight: 500;
  color: #86868b;
}

.code-editor__saved {
  font-size: 12px;
  color: #34c759;
}

.code-editor__preview {
  position: relative;
  cursor: pointer;
  border-radius: 8px;
  overflow: hidden;
  transition: box-shadow 0.2s ease;
}

.code-editor__preview :deep(.pane) {
  pointer-events: none;
  border-color: #e8e8ed;
}

.code-editor__preview:hover :deep(.pane) {
  border-color: #667eea;
}

.code-editor__preview:focus-visible {
  outline: 2px solid #667eea;
  outline-offset: 2px;
}

.code-editor__preview.is-empty {
  border: 1px dashed #d8d8de;
  border-radius: 8px;
}

.code-editor__expand-hint {
  position: absolute;
  right: 10px;
  bottom: 8px;
  padding: 2px 8px;
  border-radius: 4px;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  font-size: 12px;
  opacity: 0;
  transition: opacity 0.2s ease;
  pointer-events: none;
}

.code-editor__preview:hover .code-editor__expand-hint,
.code-editor__preview:focus-visible .code-editor__expand-hint {
  opacity: 1;
}

.code-editor__empty {
  padding: 18px 14px;
  font-size: 13px;
  color: #c0c0c5;
  font-family: "SF Mono", Monaco, Consolas, monospace;
}

.code-editor__error {
  margin-top: 4px;
  padding: 8px 12px;
  background: #fef0f0;
  border: 1px solid #fde2e2;
  border-radius: 6px;
  color: #f56c6c;
  font-size: 13px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
  font-family: "SF Mono", Monaco, Consolas, monospace;
}

.code-editor__dialog-toolbar {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  margin-bottom: 12px;
  flex-shrink: 0;
}

/* 面板占满剩余高度，内容过长时仅面板内部滚动 */
.code-editor__dialog-pane {
  flex: 1;
  min-height: 0;
}

.code-editor__dialog .code-editor__error {
  flex-shrink: 0;
}
</style>

<!--
  下方弹窗样式使用非 scoped 全局块：
  ElDialog 开启 append-to-body 后会被 teleport 到 body 下，
  脱离本组件 DOM 树，scoped 的 :deep() 无法命中，
  因此高度 / flex 布局规则会失效。这里用 .code-editor__dialog 前缀限定作用域。
-->
<style lang="scss">
/* class 落在 .el-dialog 本身或其外层包裹上，两种都覆盖 */
.code-editor__dialog.el-dialog,
.code-editor__dialog .el-dialog {
  margin-top: 5vh;
  margin-bottom: 0;
  height: 90vh;
  display: flex;
  flex-direction: column;
}

.code-editor__dialog .el-dialog__header {
  flex-shrink: 0;
  margin-right: 0;
}

.code-editor__dialog-title {
  display: flex;
  align-items: center;
  gap: 6px;
}

.code-editor__dialog .help-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  font-size: 11px;
  font-weight: 600;
  line-height: 1;
  color: #86868b;
  border: 1px solid #d2d2d7;
  border-radius: 50%;
  cursor: pointer;
  user-select: none;
  transition: color 0.2s ease, border-color 0.2s ease;
}

.code-editor__dialog .help-icon:hover {
  color: #0071e3;
  border-color: #0071e3;
}

.code-editor__dialog .el-dialog__body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding-top: 12px;
  overflow: hidden;
}

.code-editor__dialog .el-dialog__footer {
  flex-shrink: 0;
}
</style>
