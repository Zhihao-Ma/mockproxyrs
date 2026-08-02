<script setup lang="ts">
import Sidebar from "@/components/Sidebar.vue";
import Content from "@/components/Content.vue";
</script>

<template>
  <div class="app-container">
    <Sidebar />
    <Content />
  </div>
</template>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body,
html,
#app {
  height: 100%;
  font-family: -apple-system, BlinkMacSystemFont, "SF Pro Display", "SF Pro Text", "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  background: #f5f5f7;
}

.app-container {
  display: flex;
  height: 100vh;
  background: #f5f5f7;
}

/* Apple-style scrollbar */
::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: rgba(0, 0, 0, 0.2);
  border-radius: 4px;
}

::-webkit-scrollbar-thumb:hover {
  background: rgba(0, 0, 0, 0.3);
}

/* Element Plus overrides for Apple style */
.el-button {
  border-radius: 8px !important;
  font-weight: 500 !important;
  transition: all 0.2s ease !important;
}

.el-button--primary {
  background: #0071e3 !important;
  border-color: #0071e3 !important;
}

.el-button--primary:hover {
  background: #0077ed !important;
  border-color: #0077ed !important;
}

.el-button--danger {
  background: #ff3b30 !important;
  border-color: #ff3b30 !important;
}

.el-card {
  border: none !important;
  border-radius: 12px !important;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08) !important;
}

.el-input__wrapper {
  border-radius: 8px !important;
  box-shadow: 0 0 0 1px #d2d2d7 inset !important;
}

.el-input__wrapper:hover {
  box-shadow: 0 0 0 1px #86868b inset !important;
}

.el-input__wrapper.is-focus {
  box-shadow: 0 0 0 2px #0071e3 inset !important;
}

.el-switch.is-checked .el-switch__core {
  background-color: #34c759 !important;
  border-color: #34c759 !important;
}

.el-table {
  border-radius: 12px !important;
}

.el-table th.el-table__cell {
  background: #f5f5f7 !important;
  font-weight: 600 !important;
  color: #1d1d1f !important;
}

.el-table--striped .el-table__body tr.el-table__row--striped td.el-table__cell {
  background: #fafafa !important;
}

.el-menu {
  border-right: none !important;
  background: transparent !important;
}

.el-menu-item {
  border-radius: 8px !important;
  margin: 4px 8px !important;
  transition: all 0.2s ease !important;
}

.el-menu-item:hover {
  background: rgba(0, 0, 0, 0.04) !important;
}

.el-menu-item.is-active {
  background: #0071e3 !important;
  color: #fff !important;
}

.el-dialog {
  border-radius: 16px !important;
  overflow: hidden;
}

.el-dialog__header {
  padding: 20px 24px 16px !important;
  border-bottom: 1px solid #e8e8ed;
}

.el-dialog__title {
  font-size: 18px !important;
  font-weight: 600 !important;
  color: #1d1d1f !important;
}

.el-drawer {
  border-radius: 16px 0 0 16px !important;
}

.el-drawer__header {
  padding: 20px 24px !important;
  border-bottom: 1px solid #e8e8ed;
  margin-bottom: 0 !important;
}

.el-drawer__title {
  font-size: 18px !important;
  font-weight: 600 !important;
  color: #1d1d1f !important;
}

/* 高级 Mock 使用说明弹窗：固定尺寸，滚动条在内容区内部，定位对齐编辑器弹窗 */
.script-help-dialog {
  width: 640px !important;
  max-width: calc(100vw - 32px) !important;
  display: flex !important;
  flex-direction: column !important;
  max-height: 90vh !important;
  /* 对齐 ElDialog 的定位：水平 auto 居中、距顶 5vh，而非默认 transform 居中 */
  margin: 5vh auto 0 !important;
  top: 0 !important;
  transform: none !important;
  padding-bottom: 0 !important;
}

.script-help-dialog .el-message-box__header {
  flex-shrink: 0;
}

.script-help-dialog .el-message-box__content {
  flex: 1 1 auto;
  min-height: 0;
  overflow: hidden;
}

.script-help-dialog .el-message-box__message {
  /* 给明确高度上限，确保滚动条在弹窗内部而非撑破外层 */
  max-height: calc(90vh - 160px);
  overflow-y: auto;
  padding-right: 8px;
}

.script-help-dialog .el-message-box__action {
  flex-shrink: 0;
}
</style>
