<!--
  Copyright 2024 mazao

  Licensed under the Apache License, Version 2.0 (the "License");
  you may not use this file except in compliance with the License.
  You may obtain a copy of the License at

      http://www.apache.org/licenses/LICENSE-2.0

  Unless required by applicable law or agreed to in writing, software
  distributed under the License is distributed on an "AS IS" BASIS,
  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
  See the License for the specific language governing permissions and
  limitations under the License.
-->

<script setup lang="ts">
/**
 * MCP 服务器配置弹窗
 *
 * 启停 MCP 服务器、设置端口与自动启动，并提供两种 AI 客户端接入方式的配置片段：
 * - HTTP：直连桌面应用内的 MCP 端点（支持 Streamable HTTP 的客户端）
 * - stdio：通过 mockproxyrs-mcp 桥接程序（只支持子进程方式拉起服务器的客户端）
 */
import { computed } from "vue";
import { ElMessage } from "element-plus";
import { DocumentCopy } from "@element-plus/icons-vue";
import { useMcpStore } from "@/stores";

const store = useMcpStore();

const httpConfig = computed(() =>
  JSON.stringify(
    {
      mcpServers: {
        mockproxyrs: { type: "http", url: `http://127.0.0.1:${store.prefs.port}/mcp` },
      },
    },
    null,
    2
  )
);

const stdioConfig = computed(() =>
  JSON.stringify(
    {
      mcpServers: {
        mockproxyrs: { command: "/path/to/mockproxyrs-mcp", args: ["--port", String(store.prefs.port)] },
      },
    },
    null,
    2
  )
);

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    ElMessage({ message: "已复制到剪贴板", type: "success" });
  } catch {
    ElMessage({ message: "复制失败，请手动选择文本复制", type: "error" });
  }
}
</script>

<template>
  <!-- 挂到 body：侧边栏的 backdrop-filter 会形成独立层叠上下文，不脱离会被右侧内容区遮挡 -->
  <el-dialog
    v-model="store.dialogVisible"
    title="MCP 服务器"
    width="560px"
    class="mcp-dialog"
    append-to-body
  >
    <div class="mcp-section">
      <div class="mcp-status-row">
        <div class="status-badge" :class="{ active: store.running }">
          {{ store.running ? "运行中" : "已停止" }}
        </div>
        <code v-if="store.status.url" class="mcp-endpoint">{{ store.status.url }}</code>
      </div>

      <el-form label-position="top" class="mcp-form">
        <el-form-item label="监听端口">
          <el-input-number
            v-model="store.prefs.port"
            :min="1024"
            :max="65535"
            :step="1"
            controls-position="right"
            :disabled="store.running"
            placeholder="27120"
          />
          <span v-if="store.running" class="mcp-hint">停止后可修改端口</span>
        </el-form-item>
        <el-form-item>
          <div class="mcp-switch-row">
            <el-switch v-model="store.prefs.autoStart" />
            <span class="mcp-switch-label">启动应用时自动开启 MCP 服务器</span>
          </div>
        </el-form-item>
      </el-form>

      <div class="mcp-actions">
        <el-button v-if="!store.running" type="primary" @click="store.start()">启动</el-button>
        <el-button v-else type="danger" @click="store.stop()">停止</el-button>
      </div>
    </div>

    <div class="mcp-section">
      <div class="mcp-section-title">AI 客户端接入配置</div>
      <p class="mcp-desc">
        MCP 服务器随桌面应用运行，AI 对规则的修改会立即同步到运行中的代理服务。将下方配置加入客户端的 MCP 设置即可。
      </p>

      <div class="mcp-config">
        <div class="mcp-config__header">
          <span class="mcp-config__title">方式一：HTTP（推荐）</span>
          <el-button text size="small" :icon="DocumentCopy" @click="copy(httpConfig)">复制</el-button>
        </div>
        <p class="mcp-config__desc">适用于支持 Streamable HTTP 传输的客户端（如 Claude Code、Cursor）。</p>
        <pre class="mcp-config__code">{{ httpConfig }}</pre>
      </div>

      <div class="mcp-config">
        <div class="mcp-config__header">
          <span class="mcp-config__title">方式二：stdio（桥接）</span>
          <el-button text size="small" :icon="DocumentCopy" @click="copy(stdioConfig)">复制</el-button>
        </div>
        <p class="mcp-config__desc">
          适用于只支持子进程方式拉起服务器的客户端。需先构建桥接程序（<code>cargo build -p mockproxyrs-mcp</code>），并把
          <code>command</code> 改为它的实际路径。
        </p>
        <pre class="mcp-config__code">{{ stdioConfig }}</pre>
      </div>
    </div>

    <template #footer>
      <div class="dialog-footer">
        <el-button type="primary" @click="store.dialogVisible = false">关闭</el-button>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
/* 弹窗壳层（.el-dialog / .el-dialog__body）由 ElDialog 渲染且挂到了 body，scoped 命中不了，见文件底部全局样式 */
.mcp-section + .mcp-section {
  margin-top: 20px;
  padding-top: 20px;
  border-top: 1px solid #e8e8ed;
}

.mcp-status-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 16px;
}

.status-badge {
  padding: 4px 12px;
  border-radius: 20px;
  font-size: 13px;
  font-weight: 500;
  background: #f5f5f7;
  color: #86868b;
  transition: all 0.3s ease;
}

.status-badge.active {
  background: rgba(52, 199, 89, 0.12);
  color: #34c759;
}

.mcp-endpoint {
  font-size: 13px;
  color: #1d1d1f;
  background: #f5f5f7;
  padding: 4px 10px;
  border-radius: 6px;
  user-select: text;
}

.mcp-form :deep(.el-form-item) {
  margin-bottom: 12px;
}

.mcp-form :deep(.el-form-item__label) {
  font-weight: 500;
  color: #1d1d1f;
  padding-bottom: 6px;
}

.mcp-hint {
  margin-left: 12px;
  font-size: 12px;
  color: #86868b;
}

.mcp-switch-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.mcp-switch-label {
  font-size: 13px;
  color: #1d1d1f;
}

.mcp-actions {
  display: flex;
  justify-content: flex-end;
}

.mcp-actions .el-button {
  min-width: 100px;
  height: 40px;
}

.mcp-section-title {
  font-size: 14px;
  font-weight: 600;
  color: #1d1d1f;
  margin-bottom: 8px;
}

.mcp-desc {
  font-size: 13px;
  color: #86868b;
  line-height: 1.6;
  margin: 0 0 14px;
}

.mcp-config {
  padding: 12px 14px;
  background: #fafafa;
  border-radius: 10px;
  margin-bottom: 12px;
}

.mcp-config__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.mcp-config__title {
  font-size: 13px;
  font-weight: 600;
  color: #1d1d1f;
}

.mcp-config__desc {
  font-size: 12px;
  color: #86868b;
  line-height: 1.6;
  margin: 4px 0 8px;
}

.mcp-config__desc code {
  font-size: 12px;
  background: #f0f0f5;
  padding: 1px 5px;
  border-radius: 4px;
}

.mcp-config__code {
  margin: 0;
  padding: 10px 12px;
  background: #1d1d1f;
  color: #f5f5f7;
  border-radius: 8px;
  font-size: 12px;
  line-height: 1.6;
  overflow-x: auto;
  user-select: text;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
}

.dialog-footer .el-button {
  min-width: 80px;
  height: 40px;
  font-weight: 500;
}
</style>

<!-- 弹窗壳层样式：el-dialog 挂到 body 后 scoped 命中不了，需全局 -->
<style>
/* 内容超高时只滚动弹窗内部，而不是整个遮罩层：限制总高度，头尾固定，内容区自适应滚动 */
.mcp-dialog {
  display: flex !important;
  flex-direction: column !important;
  max-height: 84vh !important;
  /* 默认 margin-top 为 15vh，叠加限高后会超出视口导致遮罩层仍可滚动 */
  margin: 8vh auto 0 !important;
}

.mcp-dialog .el-dialog__header,
.mcp-dialog .el-dialog__footer {
  flex-shrink: 0;
}

.mcp-dialog .el-dialog__body {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
}
</style>
