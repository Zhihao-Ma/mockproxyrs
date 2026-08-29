<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { EditorView, basicSetup } from "codemirror";
import { placeholder } from "@codemirror/view";
import { EditorState, type Extension } from "@codemirror/state";
import { json } from "@codemirror/lang-json";
import { javascript } from "@codemirror/lang-javascript";

/**
 * CodeMirror 6 编辑器封装
 *
 * 供弹窗编辑器使用：带折叠 gutter（花括号/函数体）、语法高亮、括号匹配、
 * 行号与历史撤销。live 绑定通过监听 doc 变化上抛 update:modelValue，外部
 * modelValue 变化时做差异替换，避免回环。
 */

const props = withDefaults(
  defineProps<{
    /** 绑定值（v-model） */
    modelValue: string;
    /** 语言：json / javascript */
    language?: "json" | "javascript";
    /** 占位提示 */
    placeholder?: string;
    /** 撑满父容器高度（弹窗 flex 布局下使用） */
    fill?: boolean;
  }>(),
  {
    language: "json",
    placeholder: "",
    fill: false,
  }
);

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
}>();

const host = ref<HTMLDivElement | null>(null);
let view: EditorView | null = null;

/** 编辑期间由 syncDocFromValue 触发替换时置 true，抑制回传给父级的 update */
let suppressUpdate = false;

function langExtension(): Extension {
  switch (props.language) {
    case "javascript":
      return javascript();
    case "json":
      return json();
    default:
      return [];
  }
}

const updateListener = EditorView.updateListener.of((update) => {
  if (update.docChanged && !suppressUpdate) {
    emit("update:modelValue", update.state.doc.toString());
  }
});

/** 外部 modelValue 变化时同步到编辑器（内容不同才替换） */
function syncDocFromValue() {
  if (!view) return;
  const current = view.state.doc.toString();
  if (current !== props.modelValue) {
    suppressUpdate = true;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: props.modelValue },
    });
    suppressUpdate = false;
  }
}

onMounted(() => {
  if (!host.value) return;
  const extensions: Extension[] = [basicSetup, langExtension(), updateListener];
  if (props.placeholder) {
    extensions.push(placeholder(props.placeholder));
  }
  view = new EditorView({
    parent: host.value,
    state: EditorState.create({ doc: props.modelValue, extensions }),
  });
});

watch(() => props.modelValue, syncDocFromValue);

onBeforeUnmount(() => {
  view?.destroy();
  view = null;
});
</script>

<template>
  <div ref="host" class="cm-host" :class="{ 'is-fill': fill }"></div>
</template>

<style scoped lang="scss">
.cm-host {
  border: 1px solid #e8e8ed;
  border-radius: 8px;
  overflow: hidden;
  background: #fff;
}

.cm-host:focus-within {
  border-color: #667eea;
}

/* 弹窗模式：填满 flex 父容器高度，仅面板内部滚动 */
.cm-host.is-fill {
  min-height: 0;
}

.cm-host.is-fill :deep(.cm-editor) {
  height: 100%;
}

.cm-host :deep(.cm-scroller) {
  overflow: auto;
  font-family: "SF Mono", Monaco, Consolas, monospace;
  font-size: 13px;
}

.cm-host :deep(.cm-gutters) {
  background: #fafafa;
  color: #c0c0c5;
  border: none;
  font-family: "SF Mono", Monaco, Consolas, monospace;
  font-size: 13px;
}

.cm-host :deep(.cm-content) {
  font-family: "SF Mono", Monaco, Consolas, monospace;
}

.cm-host :deep(.cm-placeholder) {
  color: #c0c0c5;
}
</style>