<script setup lang="ts">
import { reactive, ref, computed, onMounted, watch, nextTick } from "vue";
import { useRoute } from "vue-router";
import { ElMessageBox } from "element-plus";
import { getService, listRules, updateService, startService, stopService, addRule, updateRule, deleteRule, deleteRulesByService, validateScript } from "@/api";
import type { MockRule, Method } from "@/types";
import CodeEditor from "@/components/CodeEditor.vue";
import { useLayoutStore } from "@/stores";
import * as prettier from "prettier/standalone";
import * as parserBabel from "prettier/plugins/babel";
import * as estree from "prettier/plugins/estree";
import { Fold, Expand } from "@element-plus/icons-vue";

const route = useRoute();
const layoutStore = useLayoutStore();

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

/** 规则条目扩展客户端 key：已保存规则用 id，未保存新规则用临时 key（会话内稳定，不随排序/保存变化） */
interface RuleVM extends MockRule {
  _key: string;
}

const rules = ref<RuleVM[]>([]);
const isRunning = ref(false);

/** 折叠状态：key 为 RuleVM._key，true=收起；缺省视为展开 */
const collapsed = ref<Record<string, boolean>>({});

/** 内容区滚动容器引用，用于判断规则是否在当前可见范围内 */
const contentEl = ref<HTMLElement | null>(null);

/** 当前悬浮规则 URL 的提示内容：仅当 URL 被截断时赋值，空串则禁用 tooltip */
const hoverUrl = ref("");

/** 悬浮规则条目：判断 URL 是否被截断，截断才显示完整 URL 提示 */
function onNavItemEnter(rule: RuleVM, e: MouseEvent) {
  const urlEl = (e.currentTarget as HTMLElement).querySelector<HTMLElement>(".rule-nav__url");
  const truncated = urlEl ? urlEl.scrollWidth > urlEl.clientWidth : false;
  hoverUrl.value = truncated ? rule.urlPattern || "" : "";
}

function onNavItemLeave() {
  hoverUrl.value = "";
}

function isCollapsed(rule: RuleVM): boolean {
  return collapsed.value[rule._key] === true;
}

function setRuleCollapsed(rule: RuleVM, value: boolean) {
  collapsed.value[rule._key] = value;
}

function toggleRule(rule: RuleVM) {
  setRuleCollapsed(rule, !isCollapsed(rule));
}

function expandAll() {
  const next: Record<string, boolean> = {};
  for (const r of rules.value) next[r._key] = false;
  collapsed.value = next;
}

function collapseAll() {
  const next: Record<string, boolean> = {};
  for (const r of rules.value) next[r._key] = true;
  collapsed.value = next;
}

/** 是否所有规则都已展开（用于折叠/展开单一图标按钮的态与文案） */
const allExpanded = computed(() => {
  if (rules.value.length === 0) return false;
  return rules.value.every((r) => !isCollapsed(r));
});

/** 折叠/展开单一图标按钮：全展开时收起，否则全部展开 */
function toggleExpandAll() {
  if (allExpanded.value) {
    collapseAll();
  } else {
    expandAll();
  }
}

/** 是否所有规则都已启用（无规则时视为 false） */
const allEnabled = computed(() => {
  return rules.value.length > 0 && rules.value.every((r) => r.enabled);
});

/** 全部启用/全部禁用总开关 */
async function toggleAllEnabled(enabled: boolean) {
  for (let i = 0; i < rules.value.length; i++) {
    rules.value[i].enabled = enabled;
    // 仅保存已填写 URL 的规则；空白的新建规则留待后续填写时保存
    if (rules.value[i].urlPattern) {
      await saveRule(i);
    }
  }
}

/**
 * 判断规则卡片是否落在内容区当前可见范围内。
 * 规则展开且「在眼前」时才允许收起，否则点击侧边栏始终导航过去（滚动 + 展开）。
 * 用 getBoundingClientRect 比较，避免依赖 offsetParent 链。
 */
function isRuleInViewport(rule: RuleVM): boolean {
  const el = document.getElementById(rule._key);
  const scroll = contentEl.value;
  if (!el || !scroll) return false;
  const elRect = el.getBoundingClientRect();
  const scrollRect = scroll.getBoundingClientRect();
  return (
    elRect.top >= scrollRect.top &&
    elRect.top < scrollRect.bottom &&
    elRect.bottom > scrollRect.top
  );
}

/**
 * 侧边栏点击：
 * - 规则已展开且在可见范围内 → 收起（主动收起）。
 * - 规则已展开但不在可见范围 → 仅滚动定位，保持展开状态。
 * - 规则收起 → 展开并滚动定位高亮。
 */
async function handleNavClick(rule: RuleVM) {
  if (!isCollapsed(rule)) {
    if (isRuleInViewport(rule)) {
      setRuleCollapsed(rule, true);
      return;
    }
    scrollToAndHighlight(rule._key);
    return;
  }
  setRuleCollapsed(rule, false);
  // 等待展开渲染完成后再滚动，避免以收起态（更矮）的卡片定位导致头部偏移
  await nextTick();
  scrollToAndHighlight(rule._key);
}

async function handleStart() {
  await updateService({...form})
  await startService(form.id);
  recordTargetHistory(form.id, form.targetUrl);
  isRunning.value = true;
}

async function handleStop() {
  await stopService(form.id);
  isRunning.value = false;
}

/** 目标地址历史记录（localStorage 持久化，按服务区分，每服务最多 10 条） */
function targetHistoryKey(serviceId: string) {
  return `mockproxyrs:target-history:${serviceId}`;
}

function loadTargetHistory(serviceId: string): string[] {
  try {
    return JSON.parse(localStorage.getItem(targetHistoryKey(serviceId)) || "[]");
  } catch {
    return [];
  }
}

function recordTargetHistory(serviceId: string, url: string) {
  if (!url || !url.trim()) return;
  const list = loadTargetHistory(serviceId).filter((u) => u !== url.trim());
  list.unshift(url.trim());
  localStorage.setItem(targetHistoryKey(serviceId), JSON.stringify(list.slice(0, 10)));
}

/** el-autocomplete 下拉建议回调缓存，用于删除后即时刷新 */
let lastSuggestCb: ((results: { value: string }[]) => void) | null = null;

function removeTargetHistory(serviceId: string, url: string) {
  const list = loadTargetHistory(serviceId).filter((u) => u !== url);
  localStorage.setItem(targetHistoryKey(serviceId), JSON.stringify(list));
  // 立即刷新当前下拉框
  if (lastSuggestCb) {
    lastSuggestCb(list.map((u) => ({ value: u })));
  }
}

/** el-autocomplete 建议查询：聚焦时返回该服务最近 10 条历史 */
function queryTargetHistory(
  _queryString: string,
  cb: (results: { value: string }[]) => void
) {
  const list = loadTargetHistory(form.id);
  lastSuggestCb = cb;
  cb(list.map((u) => ({ value: u })));
}

function addNewRule() {
  const key = `new-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  rules.value.unshift({
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
    _key: key,
  });
  // 新增规则自动展开
  collapsed.value[key] = false;
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
    // _key 保持会话内稳定，不随保存迁移（折叠态与 DOM id 依赖它）
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
      return;
    }
  } else {
    rules.value.splice(index, 1);
  }
  delete collapsed.value[rule._key];
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
    collapsed.value = {};
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
      _key: r.id,
    }));
    // 首次加载默认全部收起
    const next: Record<string, boolean> = {};
    for (const r of rules.value) next[r._key] = true;
    collapsed.value = next;
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
    // 跳转到规则并高亮（已保存规则的 _key 即其 id）
    if (ruleId) {
      collapsed.value[ruleId] = false;
      // 等待展开渲染完成后再滚动定位
      await nextTick();
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
  <main class="proxy-wrap">
    <aside v-if="layoutStore.navVisible" class="rule-nav">
      <div class="rule-nav__header">规则列表</div>
      <div class="rule-nav__body">
        <div
          v-for="rule in rules"
          :key="rule._key"
          class="rule-nav__item"
          :class="{ 'is-active': !isCollapsed(rule), 'is-enabled': rule.enabled }"
          @click="handleNavClick(rule)"
          @mouseenter="onNavItemEnter(rule, $event)"
          @mouseleave="onNavItemLeave"
        >
          <span class="rule-nav__dot"></span>
          <el-tooltip
            :content="hoverUrl"
            :disabled="!hoverUrl"
            placement="right"
            :show-after="300"
          >
            <span class="rule-nav__url">
              {{ rule.urlPattern || "(未设置 URL)" }}
            </span>
          </el-tooltip>
        </div>
        <div v-if="rules.length === 0" class="rule-nav__empty">暂无规则</div>
      </div>
    </aside>
    <div ref="contentEl" class="container">
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
              <el-autocomplete
                :key="form.id"
                v-model="form.targetUrl"
                :fetch-suggestions="queryTargetHistory"
                trigger-on-focus
                clearable
                placeholder="输入目标地址，或从历史中选择"
              >
                <template #default="{ item }">
                  <div class="history-item">
                    <span class="history-item__url">{{ item.value }}</span>
                    <span
                      class="history-item__del"
                      title="删除该历史记录"
                      @click.stop="removeTargetHistory(form.id, item.value)"
                    >
                      ✕
                    </span>
                  </div>
                </template>
              </el-autocomplete>
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
          <div class="card-header-left">
            <span class="card-title">Mock 规则</span>
            <div class="all-enable">
              <span class="all-enable__label">全部启用</span>
              <el-switch
                :model-value="allEnabled"
                :disabled="rules.length === 0"
                @change="(val: string | number | boolean) => toggleAllEnabled(!!val)"
              />
            </div>
          </div>
          <div class="card-actions">
            <el-button type="danger" style="color: #fff;" plain :disabled="rules.length === 0" @click="handleClearRules">
              清空规则
            </el-button>
            <el-button type="primary" @click="addNewRule">
              添加规则
            </el-button>
            <el-tooltip :content="allExpanded ? '全部收起' : '全部展开'" placement="top">
              <el-button
                class="expand-toggle-btn"
                :disabled="rules.length === 0"
                :icon="allExpanded ? Fold : Expand"
                @click="toggleExpandAll"
              />
            </el-tooltip>
          </div>
        </div>
      </template>
      <div class="rule-container">
          <template v-for="(rule, index) in rules" :key="rule._key">
            <div class="rule-card" :id="rule._key">
              <!-- 收起态：折叠图标 + 启用开关 + URL -->
              <div v-if="isCollapsed(rule)" class="rule-collapsed" @click="toggleRule(rule)">
                <span class="rule-chevron">▸</span>
                <el-switch v-model="rule.enabled" @change="saveRule(index)" @click.stop />
                <span class="rule-collapsed__url">{{ rule.urlPattern || "(未设置 URL)" }}</span>
              </div>

              <!-- 展开态：完整表单 -->
              <template v-else>
                <div class="rule-header">
                  <div class="rule-header__left">
                    <span class="rule-chevron" @click="toggleRule(rule)">▾</span>
                    <div class="rule-switches">
                      <div class="switch-item">
                        <span class="switch-label">启用</span>
                        <el-switch v-model="rule.enabled" @change="saveRule(index)" />
                      </div>
                      <div class="switch-item">
                        <span class="switch-label">转发并记录</span>
                        <el-switch v-model="rule.forwardAndRecord" @change="saveRule(index)" />
                      </div>
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
                      <el-select v-model="rule.method" @change="saveRule(index)">
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
                        v-model.trim="rule.urlPattern"
                        :placeholder="rule.isRegex ? '正则表达式，如 /api/.*' : '精确匹配，如 /api/users'"
                        @blur="saveRule(index)"
                      >
                        <template #suffix>
                          <el-tooltip
                            :content="rule.isRegex ? '正则匹配（点击切换为精确匹配）' : '精确匹配（点击切换为正则匹配）'"
                            placement="top"
                          >
                            <span
                              class="regex-toggle"
                              :class="{ active: rule.isRegex }"
                              @click="rule.isRegex = !rule.isRegex; saveRule(index)"
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
                        v-model="rule.delayMs"
                        :min="0"
                        :step="100"
                        controls-position="right"
                        @change="saveRule(index)"
                      />
                    </div>
                  </div>
                  <div v-if="!usesAdvancedMock(rule)" class="form-group">
                    <CodeEditor
                      :model-value="rule.mockResponse"
                      language="json"
                      label="Mock 响应 (JSON)"
                      :rows="6"
                      :preview-lines="6"
                      placeholder='{"code": 200, "data": {}}'
                      :formatter="formatJsonString"
                      format-on-close
                      dialog-title="编辑 Mock 响应"
                      @update:model-value="(val: string) => { rule.mockResponse = val }"
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
                          :model-value="usesAdvancedMock(rule)"
                          @change="(val: string | number | boolean) => toggleAdvancedMock(index, val)"
                        />
                      </div>
                    </div>
                    <CodeEditor
                      v-if="usesAdvancedMock(rule)"
                      :model-value="rule.script || ''"
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
                      @update:model-value="(val: string) => { rule.script = val }"
                      @blur="saveRule(index)"
                      @save="saveRule(index)"
                    />
                  </div>
                </div>
              </template>
            </div>
          </template>
          <div v-if="rules.length === 0" class="empty-state">
            <div class="empty-icon">📝</div>
            <div class="empty-text">暂无规则，点击上方按钮添加</div>
          </div>
        </div>
    </el-card>
    </div>
  </main>
</template>

<style scoped lang="scss">
/* 页面级布局：左侧规则导航列 + 右侧内容区，占满父级高度不参与外层滚动 */
.proxy-wrap {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: stretch;
  background: #f5f5f7;
}

.rule-nav {
  width: 240px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background: rgba(255, 255, 255, 0.8);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-right: 1px solid rgba(0, 0, 0, 0.08);
}

.rule-nav__header {
  padding: 20px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.06);
  font-size: 11px;
  font-weight: 600;
  color: #86868b;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.rule-nav__body {
  flex: 1;
  min-height: 0;
  padding: 12px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.rule-nav__item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border-radius: 8px;
  cursor: pointer;
  user-select: none;
  transition: background 0.2s ease;
}

.rule-nav__item:hover {
  background: #f0f0f5;
}

.rule-nav__item.is-active {
  background: #eaf2ff;
}

.rule-nav__dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #d2d2d7;
  flex-shrink: 0;
}

.rule-nav__item.is-enabled .rule-nav__dot {
  background: #34c759;
}

.rule-nav__url {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  color: #86868b;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rule-nav__item.is-active .rule-nav__url {
  color: #1d1d1f;
  font-weight: 500;
}

.rule-nav__item.is-enabled .rule-nav__url {
  color: #1d1d1f;
}

.rule-nav__empty {
  padding: 12px;
  font-size: 13px;
  color: #c0c0c5;
  text-align: center;
}

.container {
  flex: 1;
  min-width: 0;
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

.card-header-left {
  display: flex;
  align-items: center;
  gap: 16px;
}

.all-enable {
  display: flex;
  align-items: center;
  gap: 8px;
}

.all-enable__label {
  font-size: 13px;
  font-weight: 500;
  color: #86868b;
}

.card-title {
  font-size: 17px;
  font-weight: 600;
  color: #1d1d1f;
}

.card-actions {
  display: flex;
  align-items: center;
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
  flex: 1;
  min-width: 0;
}

.rule-header__left {
  display: flex;
  align-items: center;
  gap: 12px;
}

.rule-chevron {
  width: 20px;
  text-align: center;
  font-size: 12px;
  color: #86868b;
  cursor: pointer;
  user-select: none;
  flex-shrink: 0;
}

.rule-collapsed {
  display: flex;
  align-items: center;
  gap: 16px;
  cursor: pointer;
  padding: 4px 0;
}

.rule-collapsed__url {
  font-size: 14px;
  font-weight: 500;
  color: #1d1d1f;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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

<!-- 目标地址历史下拉项样式：el-autocomplete 的 popper teleport 到 body，scoped 命中不了，需全局 -->
<style lang="scss">
.el-autocomplete-suggestion li .history-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
}

.history-item__url {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.history-item__del {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  font-size: 12px;
  line-height: 1;
  color: #c0c4cc;
  border-radius: 50%;
  cursor: pointer;
  flex-shrink: 0;
  transition: color 0.2s ease, background-color 0.2s ease;
}

.history-item__del:hover {
  color: #f56c6c;
  background-color: rgba(245, 108, 108, 0.1);
}
</style>
