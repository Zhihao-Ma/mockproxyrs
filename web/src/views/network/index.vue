<script setup lang="ts">
import { ref, computed } from "vue";
import { useRouter } from "vue-router";
import { createChannel, destroyChannel, addRule, updateRule } from "@/api";
import { useNetworkStore } from "@/stores/network";
import { storeToRefs } from "pinia";
import type { ResponseEvent } from "@/types";

const router = useRouter();
const networkStore = useNetworkStore();
const { recording, logDataView } = storeToRefs(networkStore);

const showDrawer = ref(false);
const selectedRequestDetails = ref<{
  response: string;
  mockBody: string | null;
  isMock: boolean;
  forwarded: boolean;
}>({
  response: "",
  mockBody: null,
  isMock: false,
  forwarded: false,
});

const filterText = ref("");

const filteredLogData = computed(() => {
  if (filterText.value) {
    return logDataView.value.filter((item) => {
      return item.url.includes(filterText.value);
    });
  }
  return logDataView.value;
});

const btnTitle = computed(() => (recording.value ? "停止录制" : "录制"));

async function toggleRecording() {
  if (recording.value) {
    await destroyChannel();
  } else {
    await createChannel((event: ResponseEvent) => {
      networkStore.pushLogData(event);
    });
  }
  networkStore.toggleRecording();
}

async function applyRule(row: ResponseEvent) {
  console.log(row);
  if (!row.matchedRuleId) {
  // 没有匹配的规则直接新增
    const path = row.url.split('?')[0];
    await addRule({
      serviceId: row.serviceId,
      urlPattern: path,
      mockResponse: row.responseBody,
      enabled: true,
    })
  } else {
    await updateRule({
      id: row.matchedRuleId,
      serviceId: row.serviceId,
      urlPattern: row.urlPattern,
      isRegex: row.isRegex,
      method: row.method,
      enabled: true,
      forwardAndRecord: true,
      mockResponse: row.responseBody,
    });
  }

}

function jumpToRule(row: ResponseEvent) {
  router.push({ path: "/proxy", query: { id: row.serviceId, ruleId: row.matchedRuleId } });
}

async function applyAndJump(row: ResponseEvent) {
  await applyRule(row);
  jumpToRule(row);
}

function rowClick(row: ResponseEvent) {
  selectedRequestDetails.value = {
    response: row.responseBody,
    mockBody: row.mockBody,
    isMock: row.isMock,
    forwarded: row.forwarded,
  };
  showDrawer.value = true;
}
</script>

<template>
  <main class="container">
    <div class="page-header">
      <h2 class="page-title">网络监控</h2>
      <p class="page-subtitle">实时查看 Mock 服务的请求和响应</p>
    </div>

    <div class="toolbar">
      <el-button :type="recording ? 'danger' : 'primary'" size="large" @click="toggleRecording">
        <span class="btn-icon">{{ recording ? '⏹' : '⏺' }}</span>
        {{ btnTitle }}
      </el-button>
      <el-button size="large" @click="networkStore.clearLogData">
        清空记录
      </el-button>
    </div>

    <el-input v-model="filterText" placeholder="过滤关键字" size="large" class="filter-input" />

    <el-card class="table-card">
      <el-table
        :data="filteredLogData"
        style="width: 100%"
        class="network-table"
        stripe
        @row-click="rowClick"
      >
        <el-table-column prop="serviceName" label="服务" width="120">
          <template #default="scope">
            <span class="service-tag">{{ scope.row.serviceName }}</span>
          </template>
        </el-table-column>
        <el-table-column prop="url" label="URL" min-width="240" show-overflow-tooltip>
          <template #default="scope">
            <span class="url-text">{{ scope.row.url }}</span>
          </template>
        </el-table-column>
        <el-table-column prop="isMock" label="Mock" width="80" align="center">
          <template #default="scope">
            <span class="badge" :class="{ success: scope.row.isMock }">
              {{ scope.row.isMock ? "是" : "否" }}
            </span>
          </template>
        </el-table-column>
        <el-table-column prop="forwarded" label="转发" width="80" align="center">
          <template #default="scope">
            <span class="badge" :class="{ info: scope.row.forwarded }">
              {{ scope.row.forwarded ? "是" : "否" }}
            </span>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="240" align="center">
          <template #default="scope">
            <div class="action-buttons">
              <el-tooltip
                :content="'应用真实响应数据并启用mock'"
                placement="top"
              >
                <el-button :disabled="!scope.row.responseBody" size="small" @click.stop="applyRule(scope.row)">应用</el-button>
              </el-tooltip>
              <el-tooltip
                :content="'跳转到规则'"
                placement="top"
              >
                <el-button :disabled="!scope.row.matchedRuleId" size="small" @click.stop="jumpToRule(scope.row)">跳转</el-button>
              </el-tooltip>
              <el-tooltip
                :content="'应用真实响应数据并启用mock'"
                placement="top"
              >
                <el-button :disabled="!scope.row.responseBody" size="small" type="primary" @click.stop="applyAndJump(scope.row)">
                  应用并跳转
                </el-button>
              </el-tooltip>
            </div>
          </template>
        </el-table-column>
        <el-table-column prop="responseBody" label="响应" min-width="200" show-overflow-tooltip />
      </el-table>
    </el-card>

    <el-drawer v-model="showDrawer" title="请求详情" size="480px">
      <div class="detail-section">
        <div class="detail-header">
          <h3 class="detail-title">
            响应内容
            <span v-if="selectedRequestDetails.isMock" class="mock-badge">Mock</span>
            <span v-if="selectedRequestDetails.forwarded" class="forward-badge">转发</span>
          </h3>
        </div>
        <pre class="code-block">{{ selectedRequestDetails.response }}</pre>
      </div>
      <div v-if="selectedRequestDetails.mockBody" class="detail-section">
        <div class="detail-header">
          <h3 class="detail-title">Mock 响应</h3>
        </div>
        <pre class="code-block">{{ selectedRequestDetails.mockBody }}</pre>
      </div>
    </el-drawer>
  </main>
</template>

<style scoped lang="scss">
.container {
  flex: 1;
  padding: 32px;
  overflow-y: auto;
  background: #f5f5f7;
}

.page-header {
  margin-bottom: 24px;
}

.page-title {
  font-size: 28px;
  font-weight: 700;
  color: #1d1d1f;
  margin: 0 0 8px 0;
  letter-spacing: -0.5px;
}

.page-subtitle {
  font-size: 15px;
  color: #86868b;
  margin: 0;
}

.toolbar {
  display: flex;
  gap: 12px;
  margin-bottom: 24px;
}

.toolbar .el-button {
  height: 44px;
  font-weight: 500;
  min-width: 120px;
}

.btn-icon {
  font-size: 16px;
  margin-right: 6px;
}

.table-card {
  border: none;
  border-radius: 16px;
  overflow: hidden;
}

.network-table {
  border-radius: 12px;
}

.network-table :deep(.el-table__header-wrapper) {
  th {
    background: #f5f5f7 !important;
    font-weight: 600;
    color: #1d1d1f;
    font-size: 13px;
  }
}

.network-table :deep(.el-table__row) {
  cursor: pointer;
  transition: background 0.2s ease;
}

.network-table :deep(.el-table__row:hover) {
  background: rgba(0, 113, 227, 0.04) !important;
}

.service-tag {
  display: inline-block;
  padding: 4px 10px;
  background: rgba(0, 113, 227, 0.1);
  color: #0071e3;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 500;
}

.url-text {
  font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
  font-size: 13px;
  color: #1d1d1f;
}

.badge {
  display: inline-block;
  padding: 4px 8px;
  background: #f5f5f7;
  color: #86868b;
  border-radius: 4px;
  font-size: 12px;
  font-weight: 500;
}

.badge.success {
  background: rgba(52, 199, 89, 0.12);
  color: #34c759;
}

.badge.info {
  background: rgba(0, 113, 227, 0.1);
  color: #0071e3;
}

.action-buttons {
  display: flex;
  gap: 8px;
  justify-content: center;
}

.action-buttons .el-button {
  font-size: 12px;
  padding: 6px 12px;
}

.detail-section {
  margin-bottom: 24px;
}

.detail-header {
  margin-bottom: 12px;
}

.detail-title {
  font-size: 16px;
  font-weight: 600;
  color: #1d1d1f;
  margin: 0;
  display: flex;
  align-items: center;
  gap: 8px;
}

.mock-badge,
.forward-badge {
  font-size: 11px;
  font-weight: 500;
  padding: 3px 8px;
  border-radius: 4px;
}

.mock-badge {
  background: rgba(52, 199, 89, 0.12);
  color: #34c759;
}

.forward-badge {
  background: rgba(0, 113, 227, 0.1);
  color: #0071e3;
}

.code-block {
  background: #1d1d1f;
  color: #f5f5f7;
  padding: 16px;
  border-radius: 12px;
  overflow: auto;
  max-height: 400px;
  font-family: "SF Mono", Monaco, "Cascadia Code", monospace;
  font-size: 13px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
}
</style>
