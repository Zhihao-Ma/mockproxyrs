<script setup lang="ts">
import { reactive, ref, onMounted, watch } from "vue";
import { useRoute } from "vue-router";
import { ElMessageBox } from "element-plus";
import { getService, listRules, updateService, startService, stopService, addRule, updateRule, deleteRule, deleteRulesByService, validateScript } from "@/api";
import type { MockRule, Method } from "@/types";
import CodeEditor from "@/components/CodeEditor.vue";
import * as prettier from "prettier/standalone";
import * as parserBabel from "prettier/plugins/babel";
import * as estree from "prettier/plugins/estree";
import { ru } from "element-plus/lib/locale/index.js";

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
    useScript: false,
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
      useScript: rule.useScript,
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
      useScript: rule.useScript,
    });
    rule.id = id;
  }
}

function usesAdvancedMock(rule: MockRule) {
  return Boolean(rule.useScript);
}

function toggleAdvancedMock(index: number, enabled: boolean | string | number) {
  const rule = rules.value[index];
  rule.useScript = !!enabled;
  if (enabled && !rule.script) {
    rule.script = 'return { code: 0, data: {} };';
  }
  saveRule(index);
}

/** 高级 Mock 脚本使用说明 */
function showScriptHelp() {
  const html = `
    <div style="line-height:1.7;font-size:13px;text-align:left;">
      <p style="margin:0 0 10px;">脚本以 <b>普通函数体</b> 形式执行，通过 <code>return</code> 返回 Mock 响应。</p>

      <p style="margin:0 0 6px;"><b>可用变量</b></p>
      <table style="width:100%;border-collapse:collapse;margin-bottom:12px;font-size:12px;">
        <thead>
          <tr style="background:#f5f5f7;">
            <th style="padding:6px 8px;border:1px solid #e0e0e0;text-align:left;">变量</th>
            <th style="padding:6px 8px;border:1px solid #e0e0e0;text-align:left;">类型</th>
            <th style="padding:6px 8px;border:1px solid #e0e0e0;text-align:left;">说明</th>
          </tr>
        </thead>
        <tbody>
          <tr><td style="padding:6px 8px;border:1px solid #e0e0e0;"><code>request.method</code></td><td style="padding:6px 8px;border:1px solid #e0e0e0;">string</td><td style="padding:6px 8px;border:1px solid #e0e0e0;">HTTP 方法，如 "GET"</td></tr>
          <tr><td style="padding:6px 8px;border:1px solid #e0e0e0;"><code>request.url</code></td><td style="padding:6px 8px;border:1px solid #e0e0e0;">string</td><td style="padding:6px 8px;border:1px solid #e0e0e0;">完整请求 URL（含 query）</td></tr>
          <tr><td style="padding:6px 8px;border:1px solid #e0e0e0;"><code>request.path</code></td><td style="padding:6px 8px;border:1px solid #e0e0e0;">string</td><td style="padding:6px 8px;border:1px solid #e0e0e0;">路径部分（不含 query）</td></tr>
          <tr><td style="padding:6px 8px;border:1px solid #e0e0e0;"><code>request.query</code></td><td style="padding:6px 8px;border:1px solid #e0e0e0;">object</td><td style="padding:6px 8px;border:1px solid #e0e0e0;">query 参数键值对</td></tr>
          <tr><td style="padding:6px 8px;border:1px solid #e0e0e0;"><code>request.headers</code></td><td style="padding:6px 8px;border:1px solid #e0e0e0;">object</td><td style="padding:6px 8px;border:1px solid #e0e0e0;">请求头键值对</td></tr>
          <tr><td style="padding:6px 8px;border:1px solid #e0e0e0;"><code>request.body</code></td><td style="padding:6px 8px;border:1px solid #e0e0e0;">string</td><td style="padding:6px 8px;border:1px solid #e0e0e0;">请求体原始字符串，可用 <code>JSON.parse</code> 解析</td></tr>
          <tr><td style="padding:6px 8px;border:1px solid #e0e0e0;"><code>console.log/error</code></td><td style="padding:6px 8px;border:1px solid #e0e0e0;">function</td><td style="padding:6px 8px;border:1px solid #e0e0e0;">输出日志到应用日志文件</td></tr>
        </tbody>
      </table>

      <p style="margin:0 0 6px;"><b>返回值规则</b></p>
      <ul style="margin:0 0 12px;padding-left:20px;">
        <li>返回 <b>字符串</b>：直接作为响应体，状态码 200</li>
        <li>返回 <b>普通对象</b>：自动 JSON 序列化为响应体，状态码 200</li>
        <li>返回含 <code>status</code> 字段的对象：按完整响应解析
          <ul style="margin:4px 0;padding-left:20px;">
            <li><code>status</code>：状态码（100–599）</li>
            <li><code>headers</code>：可选，响应头对象</li>
            <li><code>body</code>：可选，响应体字符串</li>
          </ul>
        </li>
        <li>未 <code>return</code> 或返回 <code>undefined</code>：报错</li>
      </ul>

      <p style="margin:0 0 6px;"><b>示例</b></p>
      <pre style="background:#1d1d1f;color:#f5f5f7;padding:12px;border-radius:8px;font-size:12px;white-space:pre-wrap;margin:0;">// 读取请求体并动态返回
const data = JSON.parse(request.body);
return {
  status: 201,
  headers: { "x-mock": "1" },
  body: JSON.stringify({ id: 1, name: data.name })
};</pre>
    </div>
  `;
  ElMessageBox.alert(html, "高级 Mock 脚本使用说明", {
    dangerouslyUseHTMLString: true,
    confirmButtonText: "知道了",
    customClass: "script-help-dialog",
    center: true,
    closeOnClickModal: true,
  });
}

/** 格式化 JSON：非法 JSON 时抛错交由编辑器展示 */
function formatJsonString(source: string): Promise<string> {
  return Promise.resolve(JSON.stringify(JSON.parse(source), null, 4));
}

/** 格式化 JS 脚本：解析失败时回退原文本，保证不破坏内容 */
function formatScriptString(source: string): Promise<string> {
  return prettier
    .format(source, {
      parser: "babel",
      plugins: [parserBabel, estree],
      semi: false,
      singleQuote: false,
      printWidth: 80,
      tabWidth: 2,
    })
    .catch(() => source);
}

/** 校验 JS 脚本语法 */
function validateScriptString(source: string): Promise<void> {
  return validateScript(source);
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
      useScript: r.useScript,
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
                <CodeEditor
                  :model-value="item.mockResponse"
                  language="json"
                  label="Mock 响应 (JSON)"
                  :rows="6"
                  :preview-lines="6"
                  placeholder='{"code": 200, "data": {}}'
                  :formatter="formatJsonString"
                  format-on-close
                  dialog-title="编辑 Mock 响应"
                  @update:model-value="(val: string) => { item.mockResponse = val }"
                  @blur="saveRule(index)"
                  @save="saveRule(index)"
                />
              </div>
              <div class="form-group">
                <div class="advanced-header">
                  <div class="advanced-title">
                    <label class="form-label">高级 Mock（JS 脚本）</label>
                    <span
                      class="help-icon"
                      title="使用说明"
                      @click="showScriptHelp"
                    >?</span>
                  </div>
                  <div class="advanced-actions">
                    <el-switch
                      :model-value="usesAdvancedMock(item)"
                      @change="(val: string | number | boolean) => toggleAdvancedMock(index, val)"
                    />
                  </div>
                </div>
                <CodeEditor
                  v-if="usesAdvancedMock(item)"
                  :model-value="item.script || ''"
                  language="javascript"
                  :rows="8"
                  :preview-lines="8"
                  placeholder="return { code: 0, data: request.query };"
                  :formatter="formatScriptString"
                  :validator="validateScriptString"
                  validate-text="校验语法"
                  format-text="格式化"
                  dialog-title="编辑高级 Mock 脚本"
                  :help="showScriptHelp"
                  @update:model-value="(val: string) => { item.script = val }"
                  @blur="saveRule(index)"
                  @save="saveRule(index)"
                />
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

.advanced-title {
  display: flex;
  align-items: center;
  gap: 6px;
}

.help-icon {
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

.help-icon:hover {
  color: #0071e3;
  border-color: #0071e3;
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
