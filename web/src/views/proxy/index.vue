<script setup lang="ts">
import { reactive, ref, computed, onMounted, onUnmounted, watch, nextTick } from "vue";
import { useRoute } from "vue-router";
import { ElMessageBox } from "element-plus";
import { getService, listRules, updateService, startService, stopService, addRule, updateRule, deleteRule, deleteRulesByService, validateScript } from "@/api";
import {
  addVariant as addVariantApi,
  renameVariant as renameVariantApi,
  updateVariant as updateVariantApi,
  selectVariant as selectVariantApi,
  deleteVariant as deleteVariantApi,
  subscribeRuleChanges,
} from "@/api";
import type { MockRule, Method, ResponseKind, ResponseVariant, RuleChangeEvent } from "@/types";
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
  /**
   * 上次从后端加载或保存成功后的原始值（不含变体）。
   * 用于两件事：脏检查（没变就不发保存）与收到规则变更事件时判断本条是否有未保存改动。
   * 未保存的新规则为 null。
   */
  _snapshot: MockRule | null;
}

/** 参与脏检查与局部刷新的规则自身字段（不含变体，变体走专用接口） */
const RULE_FIELDS = [
  "urlPattern",
  "isRegex",
  "method",
  "enabled",
  "forwardAndRecord",
  "mockResponse",
  "delayMs",
  "useScript",
  "selectedVariantId",
] as const satisfies readonly (keyof MockRule)[];

/** 规则自身字段（含变体列表）的浅拷贝，作为快照；变体列表只用于编辑器内容比对，不参与规则字段脏检查 */
function snapshotOf(rule: MockRule): MockRule {
  return { ...rule, responseVariants: rule.responseVariants.map((v) => ({ ...v })) };
}

/** 两条规则在自身字段上是否一致（忽略变体列表内容，变体由后端维护） */
function ruleFieldsEqual(a: MockRule, b: MockRule): boolean {
  return RULE_FIELDS.every((k) => a[k] === b[k]);
}

/** 本条规则是否有未保存改动 */
function isDirty(rule: RuleVM): boolean {
  return rule._snapshot === null || !ruleFieldsEqual(rule, rule._snapshot);
}

const rules = ref<RuleVM[]>([]);
const isRunning = ref(false);

/** 折叠状态：key 为 RuleVM._key，true=收起；缺省视为展开 */
const collapsed = ref<Record<string, boolean>>({});

/** 内容区滚动容器引用，用于判断规则是否在当前可见范围内 */
const contentEl = ref<HTMLElement | null>(null);

/** 当前 hover 到的规则条目 key，用于只在被截断时按条目显示 tooltip */
const hoveredKey = ref("");

/** 判断规则 URL 是否已被截断（超出省略） */
function isUrlTruncated(e: MouseEvent): boolean {
  const urlEl = (e.currentTarget as HTMLElement).querySelector<HTMLElement>(".rule-nav__url");
  return urlEl ? urlEl.scrollWidth > urlEl.clientWidth : false;
}

/** 悬浮整个规则条目：URL 截断时记录该条目 key 以显示 tooltip */
function onNavItemEnter(rule: RuleVM, e: MouseEvent) {
  if (isUrlTruncated(e)) {
    hoveredKey.value = rule._key;
  }
}

function onNavItemLeave() {
  hoveredKey.value = "";
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
    delayMs: null,
    useScript: false,
    responseVariants: [],
    selectedVariantId: null,
    _key: key,
    _snapshot: null,
  });
  // 新增规则自动展开
  collapsed.value[key] = false;
}

/**
 * 保存规则自身字段：只发相对快照有变化的字段；没变化则不发请求。
 * 变体内容不在这里——有激活变体时编辑器内容走 saveActiveResponse。
 */
async function saveRule(index: number) {
  const rule = rules.value[index];
  if (!rule.urlPattern) return;

  if (!rule.id) {
    const id = await addRule({
      serviceId: form.id,
      urlPattern: rule.urlPattern,
      isRegex: rule.isRegex,
      method: rule.method,
      enabled: rule.enabled,
      forwardAndRecord: rule.forwardAndRecord,
      mockResponse: rule.mockResponse,
      delayMs: rule.delayMs ?? null,
      useScript: rule.useScript,
    });
    rule.id = id;
    // _key 保持会话内稳定，不随保存迁移（折叠态与 DOM id 依赖它）
    rule._snapshot = snapshotOf(rule);
    return;
  }

  // 局部更新：只携带变化的字段，其它页面改的字段不会被这里覆盖
  const snap = rule._snapshot ?? snapshotOf(rule);
  const params: Parameters<typeof updateRule>[0] = { id: rule.id };
  for (const k of RULE_FIELDS) {
    if (k === "selectedVariantId") continue; // 激活项由变体接口维护
    const cur = rule[k];
    const old = snap[k];
    if (cur !== old) {
      // 延迟为 null/undefined 视为相同
      if (k === "delayMs" && (cur ?? null) === (old ?? null)) continue;
      (params as unknown as Record<string, unknown>)[k] = cur;
    }
  }
  if (Object.keys(params).length === 1) return; // 只有 id，没变化

  await updateRule(params);
  rule._snapshot = snapshotOf(rule);
}

/**
 * 保存编辑器里的响应内容：有激活变体时写变体（类型保持），否则写规则本身（走 saveRule 的脏检查）。
 * 后端返回最新完整规则，直接回填，保证本地状态与仓库一致。
 */
async function saveActiveResponse(rule: RuleVM) {
  if (!rule.id) {
    await saveRule(rules.value.indexOf(rule));
    return;
  }
  const variant = getActiveVariant(rule);
  if (variant) {
    const snapVariant = rule._snapshot?.responseVariants.find((v) => v.id === variant.id);
    if (snapVariant && snapVariant.content === variant.content) return; // 没变化
    applyServerRule(rule, await updateVariantApi(rule.id, variant.id, variant.kind, variant.content));
  } else {
    await saveRule(rules.value.indexOf(rule));
  }
}

// ==================== 响应集管理 ====================

/** 获取规则当前激活的响应变体，无变体或未选中时返回 null */
function getActiveVariant(rule: RuleVM): ResponseVariant | null {
  if (!rule.selectedVariantId || rule.responseVariants.length === 0) return null;
  return rule.responseVariants.find(v => v.id === rule.selectedVariantId) || null;
}

/** 当前生效响应的类型：激活变体的类型，否则规则默认响应的类型 */
function getActiveKind(rule: RuleVM): ResponseKind {
  return getActiveVariant(rule)?.kind ?? (rule.useScript ? "script" : "static");
}

/** 编辑器标题：响应名 + 类型 */
function responseLabel(rule: RuleVM): string {
  const name = getActiveVariant(rule)?.name ?? "默认";
  const kind = getActiveKind(rule) === "script" ? "JS 脚本" : "JSON";
  return rule.responseVariants.length > 0 ? `Mock 响应 (${kind}) — ${name}` : `Mock 响应 (${kind})`;
}

/** 编辑器占位符：随当前生效响应的类型切换 */
function responsePlaceholder(rule: RuleVM): string {
  return getActiveKind(rule) === "script"
    ? "return { code: 0, data: request.query };"
    : '{"code": 200, "data": {}}';
}

/** 获取规则当前显示的响应内容（变体优先，回退到默认响应） */
function getActiveResponse(rule: RuleVM): string {
  const variant = getActiveVariant(rule);
  return variant ? variant.content : rule.mockResponse;
}

/** 设置规则当前显示的响应内容 */
function setActiveResponse(rule: RuleVM, val: string) {
  const variant = getActiveVariant(rule);
  if (variant) {
    variant.content = val;
  } else {
    rule.mockResponse = val;
  }
}

/**
 * 切换当前生效响应的类型（JSON / JS 脚本）。
 * 每条响应只有一个内容槽：切类型量清空内容；内容非空时先确认。
 * - 默认响应：改 useScript + 清空 mockResponse，走 saveRule 的局部更新
 * - 变体：调 updateVariant 整体替换类型与内容，用后端返回回填
 */
async function switchActiveKind(rule: RuleVM, kind: ResponseKind) {
  if (getActiveKind(rule) === kind) return;
  const hasContent = getActiveResponse(rule).trim() !== "";
  if (hasContent) {
    try {
      await ElMessageBox.confirm(
        `切换为${kind === "script" ? "JS 脚本" : "JSON"}将清空当前响应内容，是否继续？`,
        "切换响应类型",
        { confirmButtonText: "确定", cancelButtonText: "取消", type: "warning" },
      );
    } catch {
      return; // 用户取消
    }
  }
  const variant = getActiveVariant(rule);
  if (variant) {
    applyServerRule(rule, await updateVariantApi(rule.id, variant.id, kind, ""));
  } else {
    rule.useScript = kind === "script";
    rule.mockResponse = "";
    await saveRule(rules.value.indexOf(rule));
  }
}

/**
 * 用后端返回的最新规则回填本地条目（保留 _key / 折叠态）。
 * 变体操作与规则变更事件都走这里，保证本地状态与仓库一致。
 */
function applyServerRule(local: RuleVM, server: MockRule) {
  Object.assign(local, server, {
    responseVariants: server.responseVariants ?? [],
    selectedVariantId: server.selectedVariantId ?? null,
    _snapshot: snapshotOf(server),
  });
}

/** 选中某个响应变体；再点一次已激活的 tab 则取消激活，回退到规则本身的 mockResponse */
async function selectVariant(rule: RuleVM, variantId: string) {
  if (!rule.id) return;
  const next = rule.selectedVariantId === variantId ? null : variantId;
  applyServerRule(rule, await selectVariantApi(rule.id, next));
}

/** 响应变体右键菜单状态：记录正在操作的规则、变体与菜单位置 */
const variantMenu = reactive<{
  visible: boolean;
  rule: RuleVM | null;
  variantId: string;
  x: number;
  y: number;
}>({ visible: false, rule: null, variantId: "", x: 0, y: 0 });

/** 右键点击响应变体 tab：打开重命名/删除菜单 */
function openVariantMenu(rule: RuleVM, variantId: string, e: MouseEvent) {
  variantMenu.visible = true;
  variantMenu.rule = rule;
  variantMenu.variantId = variantId;
  variantMenu.x = e.clientX;
  variantMenu.y = e.clientY;
}

function closeVariantMenu() {
  variantMenu.visible = false;
}

/** 菜单中的重命名操作 */
function menuRenameVariant() {
  if (variantMenu.rule) {
    renameVariant(variantMenu.rule, variantMenu.variantId);
  }
  closeVariantMenu();
}

/** 菜单中的删除操作 */
function menuDeleteVariant() {
  if (variantMenu.rule) {
    deleteVariant(variantMenu.rule, variantMenu.variantId);
  }
  closeVariantMenu();
}

/** 新增响应变体 */
async function addVariant(rule: RuleVM) {
  if (!rule.id) return;
  // 以当前生效的内容为起点，便于在其基础上修改
  await applyServerRule(
    rule,
    await addVariantApi(rule.id, `响应 ${rule.responseVariants.length + 1}`, getActiveResponse(rule) || "{}"),
  );
}

/** 重命名响应变体 */
function renameVariant(rule: RuleVM, variantId: string) {
  if (!rule.id) return;
  const variant = rule.responseVariants.find((v) => v.id === variantId);
  if (!variant) return;
  ElMessageBox.prompt("请输入新名称", "重命名", {
    inputValue: variant.name,
    confirmButtonText: "确定",
    cancelButtonText: "取消",
  })
    .then(async ({ value }) => {
      if (value) {
        applyServerRule(rule, await renameVariantApi(rule.id, variantId, value));
      }
    })
    .catch(() => {});
}

/** 删除响应变体 */
async function deleteVariant(rule: RuleVM, variantId: string) {
  if (!rule.id) return;
  const variant = rule.responseVariants.find((v) => v.id === variantId);
  if (!variant) return;
  try {
    await ElMessageBox.confirm(`确定删除响应 "${variant.name}" 吗？此操作不可撤销。`, "删除确认", {
      confirmButtonText: "确定",
      cancelButtonText: "取消",
      type: "warning",
    });
    applyServerRule(rule, await deleteVariantApi(rule.id, variantId));
  } catch {
    // 用户取消
  }
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
    rules.value = ruleList.map((r) => ({
      ...r,
      useScript: r.useScript,
      responseVariants: r.responseVariants ?? [],
      selectedVariantId: r.selectedVariantId ?? null,
      _key: r.id,
      _snapshot: snapshotOf(r),
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

/**
 * 处理后端推送的规则变更：另一端（MCP / 其它页面）改了规则时局部刷新。
 * 本条有未保存改动的规则跳过，避免把用户正在编辑的内容冲掉。
 */
function onRuleChange(event: RuleChangeEvent) {
  if (event.type === "deleted") {
    if (event.serviceId !== form.id) return;
    const idx = rules.value.findIndex((r) => r.id === event.ruleId);
    if (idx >= 0 && !isDirty(rules.value[idx])) rules.value.splice(idx, 1);
    return;
  }
  const server = event.rule;
  if (server.serviceId !== form.id) return;
  const local = rules.value.find((r) => r.id === server.id);
  if (!local) {
    // 另一端新建的规则：追加到列表（后端是最新者在前）
    rules.value.unshift({
      ...server,
      responseVariants: server.responseVariants ?? [],
      selectedVariantId: server.selectedVariantId ?? null,
      _key: server.id,
      _snapshot: snapshotOf(server),
    });
    collapsed.value[server.id] = true;
    return;
  }
  if (isDirty(local)) return;
  applyServerRule(local, server);
}

onMounted(async () => {
  // 全局点击/滚动时关闭响应集右键菜单
  document.addEventListener("click", closeVariantMenu);
  document.addEventListener("scroll", closeVariantMenu, true);
  // 常驻订阅规则变更，不随网络监控录制开关
  await subscribeRuleChanges(onRuleChange);

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

onUnmounted(() => {
  document.removeEventListener('click', closeVariantMenu);
  document.removeEventListener('scroll', closeVariantMenu, true);
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
        <el-tooltip
          v-for="rule in rules"
          :key="rule._key"
          :content="rule.urlPattern || ''"
          :visible="hoveredKey === rule._key"
          placement="right"
          manual
        >
          <div
            class="rule-nav__item"
            :class="{ 'is-active': !isCollapsed(rule), 'is-enabled': rule.enabled }"
            @click="handleNavClick(rule)"
            @mouseenter="onNavItemEnter(rule, $event)"
            @mouseleave="onNavItemLeave"
          >
            <span class="rule-nav__dot"></span>
            <span class="rule-nav__url">
              {{ rule.urlPattern || "(未设置 URL)" }}
            </span>
          </div>
        </el-tooltip>
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
                  <div class="form-group">
                    <!-- 响应区头部：左变体 tab（有多条命名响应时展示），右类型切换 -->
                    <div class="response-header">
                      <div v-if="rule.responseVariants.length > 0" class="variant-tabs">
                        <div
                          v-for="variant in rule.responseVariants"
                          :key="variant.id"
                          class="variant-tab"
                          :class="{ 'is-active': rule.selectedVariantId === variant.id }"
                          @click="selectVariant(rule, variant.id)"
                          @contextmenu.prevent="openVariantMenu(rule, variant.id, $event)"
                        >
                          {{ variant.name }}<span v-if="variant.kind === 'script'" class="variant-tab__kind">JS</span>
                        </div>
                        <div
                          class="variant-tab variant-tab--add"
                          title="新增响应"
                          @click="addVariant(rule)"
                        >
                          +
                        </div>
                      </div>
                      <div v-else class="variant-tabs">
                        <div
                          class="variant-tab variant-tab--add"
                          title="新增响应"
                          @click="addVariant(rule)"
                        >
                          + 响应集
                        </div>
                      </div>
                      <div class="response-kind">
                        <span
                          v-if="getActiveKind(rule) === 'script'"
                          class="help-icon"
                          title="使用说明"
                          @click="showScriptHelp"
                        >?</span>
                        <el-radio-group
                          :model-value="getActiveKind(rule)"
                          size="small"
                          @change="(val: string | number | boolean | undefined) => switchActiveKind(rule, val as ResponseKind)"
                        >
                          <el-radio-button value="static">JSON</el-radio-button>
                          <el-radio-button value="script">JS 脚本</el-radio-button>
                        </el-radio-group>
                      </div>
                    </div>
                    <!-- 单一内容槽：语言 / 格式化 / 校验随当前生效响应的类型切换 -->
                    <CodeEditor
                      :key="`${rule.id || rule._key}:${rule.selectedVariantId ?? 'default'}:${getActiveKind(rule)}`"
                      :model-value="getActiveResponse(rule)"
                      :language="getActiveKind(rule) === 'script' ? 'javascript' : 'json'"
                      :label="responseLabel(rule)"
                      :rows="getActiveKind(rule) === 'script' ? 8 : 6"
                      :preview-lines="getActiveKind(rule) === 'script' ? 8 : 6"
                      :placeholder="responsePlaceholder(rule)"
                      :formatter="getActiveKind(rule) === 'script' ? formatScriptString : formatJsonString"
                      :validator="getActiveKind(rule) === 'script' ? validateScriptString : undefined"
                      :validate-text="getActiveKind(rule) === 'script' ? '校验语法' : ''"
                      :format-text="getActiveKind(rule) === 'script' ? '格式化' : ''"
                      :format-on-close="getActiveKind(rule) !== 'script'"
                      :dialog-title="getActiveKind(rule) === 'script' ? '编辑 JS 脚本' : '编辑 Mock 响应'"
                      :help="getActiveKind(rule) === 'script' ? showScriptHelp : undefined"
                      @update:model-value="(val: string) => { setActiveResponse(rule, val) }"
                      @blur="saveActiveResponse(rule)"
                      @save="saveActiveResponse(rule)"
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
    <!-- 响应集 tab 右键菜单：重命名 / 删除 -->
    <Teleport to="body">
      <div
        v-if="variantMenu.visible"
        class="variant-context-menu"
        :style="{ left: variantMenu.x + 'px', top: variantMenu.y + 'px' }"
      >
        <div class="variant-context-menu__item" @click="menuRenameVariant">重命名</div>
        <div class="variant-context-menu__item variant-context-menu__item--danger" @click="menuDeleteVariant">删除</div>
      </div>
    </Teleport>
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
  /* 翻转省略方向：尾部完整展示，开头用省略号代替 */
  direction: rtl;
  text-align: left;
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
  /* 翻转省略方向：尾部完整展示，开头用省略号代替 */
  direction: rtl;
  text-align: left;
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

/* 响应区头部：左 tab 条、右类型切换 */
.response-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}

.response-header .variant-tabs {
  flex: 1;
  min-width: 0;
}

.response-kind {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
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

/* 响应集 tab条 */
.variant-tabs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.variant-tab {
  padding: 5px 12px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 500;
  color: #86868b;
  background: #f0f0f5;
  cursor: pointer;
  user-select: none;
  transition: all 0.15s ease;
  white-space: nowrap;
}

.variant-tab:hover {
  background: #e8e8ed;
  color: #1d1d1f;
}

.variant-tab.is-active {
  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
  color: #fff;
  box-shadow: 0 2px 8px rgba(102, 126, 234, 0.3);
}

/* 脚本变体的小标记 */
.variant-tab__kind {
  margin-left: 4px;
  padding: 0 4px;
  border-radius: 3px;
  font-size: 10px;
  font-weight: 600;
  line-height: 14px;
  color: #86868b;
  background: rgba(0, 0, 0, 0.06);
}

.variant-tab.is-active .variant-tab__kind {
  color: #fff;
  background: rgba(255, 255, 255, 0.25);
}

.variant-tab--add {
  background: transparent;
  border: 1px dashed #d2d2d7;
  color: #86868b;
}

.variant-tab--add:hover {
  border-color: #0071e3;
  color: #0071e3;
  background: transparent;
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

/* 响应集右键菜单：Teleport 到 body，scoped 命中不了，需全局 */
.variant-context-menu {
  position: fixed;
  z-index: 9999;
  min-width: 120px;
  padding: 4px;
  background: #fff;
  border-radius: 8px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.12);
  border: 1px solid rgba(0, 0, 0, 0.06);
}

.variant-context-menu__item {
  padding: 7px 12px;
  font-size: 13px;
  color: #1d1d1f;
  border-radius: 5px;
  cursor: pointer;
  user-select: none;
}

.variant-context-menu__item:hover {
  background: #f0f0f5;
}

.variant-context-menu__item--danger {
  color: #f56c6c;
}

.variant-context-menu__item--danger:hover {
  background: rgba(245, 108, 108, 0.1);
}
</style>
