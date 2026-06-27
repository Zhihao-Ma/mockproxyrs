<script setup lang="ts">
import { reactive, ref, onMounted, watch } from "vue";
import { useRoute } from "vue-router";
import { ElMessageBox } from "element-plus";
import { getService, listRules, updateService, startService, stopService, addRule, updateRule, deleteRule, deleteRulesByService, validateScript } from "@/api";
import type { MockRule, Method } from "@/types";

const route = useRoute();

/** HTTP 方法选项 */
const methodOptions: { value: Method; label: string }[] = [
  { value: "ALL", label: "ALL" },
  { value: "GET", label: "GET" },
  { value: "POST", label: "POST" },
  { value: "PUT", label: "PUT" },
  { value: "DELETE", label: "DELETE" },
];

const form = reactive({
  id: "",
  name: "",
  listenAddr: "",
  targetUrl: "",
});

const rules = ref<MockRule[]>([]);
const isRunning = ref(false);
/** 脚本校验错误，key 为规则在 rules 中的索引 */
const scriptErrors = ref<Record<number, string>>({});

async function handleStart() {
  await updateService({...form})
  await startService(form.id);
  isRunning.value = true;
}

async function handleStop() {
  await stopService(form.id);
  isRunning.value = false;
}

function addNewRule() {
  rules.value.push({
    id: "",
    serviceId: form.id,
    urlPattern: "",
    isRegex: false,
    method: "ALL",
    enabled: false,
    forwardAndRecord: false,
    mockResponse: "",
    script: null,
    delayMs: null,
    advancedEnabled: false,
  });
}

async function saveRule(index: number) {
  const rule = rules.value[index];
  if (!rule.urlPattern) return;

  if (rule.id) {
    await updateRule({
      id: rule.id,
      serviceId: rule.serviceId,
      urlPattern: rule.urlPattern,
      isRegex: rule.isRegex,
      method: rule.method,
      enabled: rule.enabled,
      forwardAndRecord: rule.forwardAndRecord,
      mockResponse: rule.mockResponse,
      script: rule.script || null,
      delayMs: rule.delayMs ?? null,
    });
  } else {
    const id = await addRule({
      serviceId: form.id,
      urlPattern: rule.urlPattern,
      isRegex: rule.isRegex,
      method: rule.method,
      enabled: rule.enabled,
      forwardAndRecord: rule.forwardAndRecord,
      mockResponse: rule.mockResponse,
      script: rule.script || null,
      delayMs: rule.delayMs ?? null,
    });
    rule.id = id;
  }
}

function formatJson(index: number) {
  const rule = rules.value[index];
  if (rule.mockResponse) {
    try {
      const jsonObj = JSON.parse(rule.mockResponse);
      rule.mockResponse = JSON.stringify(jsonObj, null, 4);
      saveRule(index);
    } catch (e) {
      // ignore parse error
    }
  }
}

function usesAdvancedMock(rule: MockRule) {
  return Boolean(rule.advancedEnabled);
}

function toggleAdvancedMock(index: number, enabled: boolean | string | number) {
  const rule = rules.value[index];
  rule.advancedEnabled = !!enabled;
  if (enabled && !rule.script) {
    rule.script = 'return { code: 0, data: {} };';
  }
  saveRule(index);
}

async function handleValidateScript(index: number) {
  const rule = rules.value[index];
  const script = rule.script || "";
  try {
    await validateScript(script);
    rule.script = formatScript(script);
    delete scriptErrors.value[index];
    saveRule(index);
  } catch (error) {
    scriptErrors.value[index] = String(error);
  }
}

function formatScript(source: string): string {
  const lines = source.split("\n");
  let indent = 0;
  const formatted = lines.map((line) => {
    const trimmed = line.trim();
    if (!trimmed) return "";
    // 减少缩进：以 } 或 ] 开头的行
    if (trimmed.startsWith("}") || trimmed.startsWith("]")) {
      indent = Math.max(0, indent - 1);
    }
    const result = "  ".repeat(indent) + trimmed;
    // 增加缩进：以 { 或 [ 结尾且不是 } 或 ] 开头（避免 "{}" 双计数）
    const opens = (trimmed.match(/\{/g) || []).length;
    const closes = (trimmed.match(/\}/g) || []).length;
    const openBrackets = (trimmed.match(/\[/g) || []).length;
    const closeBrackets = (trimmed.match(/\]/g) || []).length;
    indent += opens - closes + openBrackets - closeBrackets;
    indent = Math.max(0, indent);
    return result;
  });
  return formatted.join("\n").trim();
}

async function handleDeleteRule(index: number) {
  const rule = rules.value[index];
  if (rule.id) {
    try {
      await deleteRule(rule.id);
      rules.value.splice(index, 1);
    } catch {
      // API 层已显示错误信息
    }
  } else {
    rules.value.splice(index, 1);
  }
}

async function handleClearRules() {
  if (rules.value.length === 0) return;
  try {
    await ElMessageBox.confirm("确定要清空所有规则吗？此操作不可撤销。", "确认清空", {
      confirmButtonText: "确定",
      cancelButtonText: "取消",
      type: "warning",
    });
    await deleteRulesByService(form.id);
    rules.value = [];
  } catch {
    // 用户取消或 API 层已显示错误信息
  }
}

async function loadService(id: string) {
  const service = await getService(id);
  if (service) {
    Object.assign(form, {
      id: service.id,
      name: service.name,
      listenAddr: service.listenAddr,
      targetUrl: service.targetUrl,
    });
    isRunning.value = service.running;
    const ruleList = await listRules(id);
    rules.value = ruleList.map(r => ({
      ...r,
      advancedEnabled: Boolean(r.script && r.script.trim()),
    }));
  }
}

function scrollToAndHighlight(id: string) {
  const el = document.getElementById(id)
  if (!el) return

  // 平滑滚动到元素
  el.scrollIntoView({
    behavior: 'smooth',
    block: 'center'
  })

  // 高亮效果
  el.classList.add('highlight-flash')
  setTimeout(() => {
    el.classList.remove('highlight-flash')
  }, 800)
}

onMounted(async () => {
  const id = route.query.id as string;
  const ruleId = route.query.ruleId as string;
  if (id) {
    await loadService(id);
    // 跳转到规则并高亮
    if (ruleId) {
      scrollToAndHighlight(ruleId);
    }
  }
});

watch(
  () => route.query.id,
  (newId) => {
    if (newId) {
      loadService(newId as string);
    }
  }
);
</script>

<template>
  <main class="container">
    <div class="page-header">
      <h2 class="page-title">服务配置</h2>
      <p class="page-subtitle">配置 Mock 服务的监听地址和转发规则</p>
    </div>

    <el-card class="config-card">
      <template #header>
        <div class="card-header">
          <span class="card-title">基本信息</span>
          <div class="status-badge" :class="{ active: isRunning }">
            {{ isRunning ? '运行中' : '已停止' }}
          </div>
        </div>
      </template>
      <el-form :model="form" label-position="top" class="config-form">
        <el-row :gutter="24">
          <el-col :span="8">
            <el-form-item label="服务名称">
              <el-input v-model="form.name" />
            </el-form-item>
          </el-col>
          <el-col :span="8">
            <el-form-item label="监听地址">
              <el-input v-model="form.listenAddr" />
            </el-form-item>
          </el-col>
          <el-col :span="8">
            <el-form-item label="目标地址">
              <el-input v-model="form.targetUrl" />
            </el-form-item>
          </el-col>
        </el-row>
        <div class="action-bar">
          <el-button v-if="!isRunning" type="primary" size="large" @click="handleStart">
            启动服务
          </el-button>
          <el-button v-else type="danger" size="large" @click="handleStop">
            停止服务
          </el-button>
        </div>
      </el-form>
    </el-card>

    <el-card class="rules-card">
      <template #header>
        <div class="card-header">
          <span class="card-title">Mock 规则</span>
          <div class="card-actions">
            <el-button type="danger" style="color: #fff;" plain :disabled="rules.length === 0" @click="handleClearRules">
              清空规则
            </el-button>
            <el-button type="primary" @click="addNewRule">
              添加规则
            </el-button>
          </div>
        </div>
      </template>
      <div class="rule-container">
        <template v-for="(item, index) in rules" :key="item.id || index">
          <div class="rule-card" :id="item.id">
            <div class="rule-header">
              <div class="rule-switches">
                <div class="switch-item">
                  <span class="switch-label">启用</span>
                  <el-switch v-model="item.enabled" @change="saveRule(index)" />
                </div>
                <div class="switch-item">
                  <span class="switch-label">转发并记录</span>
                  <el-switch v-model="item.forwardAndRecord" @change="saveRule(index)" />
                </div>
              </div>
              <el-button type="danger" text class="delete-btn" @click="handleDeleteRule(index)">
                删除
              </el-button>
            </div>
            <div class="rule-body">
              <div class="form-row">
                <div class="form-group form-group-method">
                  <label class="form-label">方法</label>
                  <el-select v-model="item.method" @change="saveRule(index)">
                    <el-option
                      v-for="opt in methodOptions"
                      :key="opt.value"
                      :label="opt.label"
                      :value="opt.value"
                    />
                  </el-select>
                </div>
                <div class="form-group form-group-url">
                  <label class="form-label">URL 匹配模式</label>
                  <el-input
                    v-model.trim="item.urlPattern"
                    :placeholder="item.isRegex ? '正则表达式，如 /api/.*' : '精确匹配，如 /api/users'"
                    @blur="saveRule(index)"
                  >
                    <template #suffix>
                      <el-tooltip
                        :content="item.isRegex ? '正则匹配（点击切换为精确匹配）' : '精确匹配（点击切换为正则匹配）'"
                        placement="top"
                      >
                        <span
                          class="regex-toggle"
                          :class="{ active: item.isRegex }"
                          @click="item.isRegex = !item.isRegex; saveRule(index)"
                        >
                          .*
                        </span>
                      </el-tooltip>
                    </template>
                  </el-input>
                </div>
              </div>
              <div class="form-row">
                <div class="form-group form-group-delay">
                  <label class="form-label">延迟(ms)</label>
                  <el-input-number
                    v-model="item.delayMs"
                    :min="0"
                    :step="100"
                    controls-position="right"
                    @change="saveRule(index)"
                  />
                </div>
              </div>
              <div v-if="!usesAdvancedMock(item)" class="form-group">
                <label class="form-label">Mock 响应 (JSON)</label>
                <el-input
                  v-model.trim="item.mockResponse"
                  type="textarea"
                  :rows="6"
                  placeholder='{"code": 200, "data": {}}'
                  @blur="formatJson(index)"
                />
              </div>
              <div class="form-group">
                <div class="advanced-header">
                  <label class="form-label">高级 Mock（JS 脚本）</label>
                  <div class="advanced-actions">
                    <el-switch
                      :model-value="usesAdvancedMock(item)"
                      @change="(val: string | number | boolean) => toggleAdvancedMock(index, val)"
                    />
                    <el-button
                      size="small"
                      :disabled="!item.script"
                      @click="handleValidateScript(index)"
                    >
                      校验语法
                    </el-button>
                  </div>
                </div>
                <el-input
                  v-if="usesAdvancedMock(item)"
                  v-model="item.script"
                  type="textarea"
                  :rows="8"
                  placeholder="return { code: 0, data: request.query };"
                  @blur="saveRule(index)"
                />
                <div v-if="scriptErrors[index]" class="script-error">
                  {{ scriptErrors[index] }}
                </div>
              </div>
            </div>
          </div>
        </template>
        <div v-if="rules.length === 0" class="empty-state">
          <div class="empty-icon">📝</div>
          <div class="empty-text">暂无规则，点击上方按钮添加</div>
        </div>
      </div>
    </el-card>
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

.config-card,
.rules-card {
  margin-bottom: 24px;
  border: none;
  border-radius: 16px;
  overflow: hidden;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.card-title {
  font-size: 17px;
  font-weight: 600;
  color: #1d1d1f;
}

.card-actions {
  display: flex;
  gap: 12px;
}

.status-badge {
  padding: 6px 14px;
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

.config-form {
  padding: 8px 0;
}

.config-form :deep(.el-form-item__label) {
  font-weight: 500;
  color: #1d1d1f;
  padding-bottom: 8px;
}

.action-bar {
  margin-top: 24px;
  padding-top: 24px;
  border-top: 1px solid #e8e8ed;
}

.action-bar .el-button {
  min-width: 120px;
  height: 44px;
  font-weight: 500;
}

.btn-icon {
  font-size: 16px;
  margin-right: 4px;
}

.rule-container {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.rule-card {
  background: #fafafa;
  border-radius: 12px;
  padding: 20px;
  transition: all 0.2s ease;
}

.rule-card:hover {
  background: #f5f5f7;
}

.rule-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}

.rule-switches {
  display: flex;
  gap: 32px;
}

.switch-item {
  display: flex;
  align-items: center;
  gap: 10px;
}

.switch-label {
  font-size: 14px;
  font-weight: 500;
  color: #1d1d1f;
}

.delete-btn {
  font-size: 14px;
  font-weight: 500;
  color: #fff;
}

.rule-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.form-row {
  display: flex;
  gap: 16px;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.form-group-method {
  width: 120px;
  flex-shrink: 0;
}

.form-group-url {
  flex: 1;
}

.form-label {
  font-size: 13px;
  font-weight: 500;
  color: #86868b;
}

.empty-state {
  text-align: center;
  padding: 48px 24px;
  color: #86868b;
}

.empty-icon {
  font-size: 48px;
  margin-bottom: 16px;
  opacity: 0.5;
}

.empty-text {
  font-size: 15px;
}

.regex-toggle {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 600;
  font-family: "SF Mono", Monaco, Consolas, monospace;
  cursor: pointer;
  background: #f0f0f0;
  color: #86868b;
  transition: all 0.2s ease;
  user-select: none;
}

.regex-toggle:hover {
  background: #e8e8ed;
  color: #1d1d1f;
}

.regex-toggle.active {
  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
  color: #fff;
  box-shadow: 0 2px 8px rgba(102, 126, 234, 0.3);
}

.regex-toggle.active:hover {
  box-shadow: 0 4px 12px rgba(102, 126, 234, 0.4);
}

.script-error {
  margin-top: 8px;
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

.highlight-flash {
  animation: highlightFlash 0.8s ease;
}

.form-group-delay {
  width: 160px;
  flex-shrink: 0;
}

.advanced-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.advanced-actions {
  display: flex;
  align-items: center;
  gap: 12px;
}

@keyframes highlightFlash {
  0% {
    box-shadow: 0 0 0 2px rgba(255, 255, 255, 0), inset 0 0 0 100px rgba(245, 108, 108, 0);
  }
  30% {
    box-shadow: 0 0 0 2px white, 0 0 0 6px rgba(245, 108, 108, 0.6);
    background-color: rgba(245, 108, 108, 0.15);
  }
  100% {
    box-shadow: 0 0 0 2px rgba(255, 255, 255, 0), inset 0 0 0 100px rgba(245, 108, 108, 0);
  }
}
</style>
