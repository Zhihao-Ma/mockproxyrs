/**
 * Copyright 2024 mazao
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

/**
 * 类型定义
 *
 * 定义前后端交互的数据类型，与 Rust 后端结构体对应。
 * 字段名使用 camelCase，通过 serde 的 rename_all 自动转换。
 */

/**
 * Mock 服务配置
 */
export interface MockService {
  id: string;
  name: string;
  /** 监听地址，如 "127.0.0.1:8080" */
  listenAddr: string;
  /** 目标地址，如 "https://example.com" */
  targetUrl: string;
}

/**
 * Mock 服务详情（包含运行状态）
 */
export interface MockServiceDetail {
  id: string;
  name: string;
  /** 监听地址，如 "127.0.0.1:8080" */
  listenAddr: string;
  /** 目标地址，如 "https://example.com" */
  targetUrl: string;
  /** 是否运行中 */
  running: boolean;
}

/**
 * 响应类型：静态内容或 JS 脚本
 *
 * 规则的默认响应与每条响应变体都有自己的类型；每条响应只有一个内容槽，切换类型意味着内容被替换。
 */
export type ResponseKind = "static" | "script";

/**
 * Mock 响应变体（响应集管理）
 */
export interface ResponseVariant {
  id: string;
  ruleId: string;
  name: string;
  /** 响应类型 */
  kind: ResponseKind;
  /** 响应内容：静态响应体或 JS 脚本（函数体），由 kind 决定 */
  content: string;
}

/**
 * HTTP 请求方法
 */
export type Method = "ALL" | "GET" | "POST" | "PUT" | "DELETE";

/**
 * Mock 规则
 */
export interface MockRule {
  id: string;
  serviceId: string;
  /** URL 匹配模式 */
  urlPattern: string;
  /** 是否为正则匹配（true=正则匹配，false=精确匹配） */
  isRegex: boolean;
  /** HTTP 方法（ALL 表示匹配所有方法） */
  method: Method;
  /** 是否启用 mock */
  enabled: boolean;
  /** 是否转发并记录 */
  forwardAndRecord: boolean;
  /** 默认响应内容：未选中任何变体时生效；useScript 为 true 时是 JS 脚本，否则是静态响应体 */
  mockResponse: string;
  /** 延迟响应毫秒数 */
  delayMs?: number | null;
  /** 默认响应是否为 JS 脚本（true=执行脚本，false=使用静态 mockResponse） */
  useScript: boolean;
  /** 响应变体列表 */
  responseVariants: ResponseVariant[];
  /** 当前选中的响应变体 ID，null 表示使用规则的默认响应 */
  selectedVariantId: string | null;
}

/**
 * 服务运行状态
 */
export interface ServiceStatus {
  serviceId: string;
  running: boolean;
  startedAt: number | null;
}

/**
 * MCP 服务器状态
 */
export interface McpStatus {
  running: boolean;
  port: number | null;
  /** AI 客户端配置用的完整地址，如 "http://127.0.0.1:27120/mcp" */
  url: string | null;
}

/**
 * 响应事件（从后端推送）
 */
export interface ResponseEvent {
  serviceId: string;
  serviceName: string;
  /** 请求 URL */
  url: string;
  /** URL 匹配模式 */
  urlPattern: string | null;
  /** HTTP 方法 */
  method: string;
  /** 是否正则匹配· */
  isRegex: boolean;
  /** 匹配的规则 ID */
  matchedRuleId: string | null;
  /** 是否命中 mock */
  isMock: boolean;
  /** 是否转发了请求 */
  forwarded: boolean;
  /** 实际响应内容 */
  responseBody: string;
  /** 请求体（payload）原始字符串，可能为空 */
  requestBody: string;
  /** mock 响应内容 */
  mockBody: string | null;
  /** 请求头（键值对，header 名小写） */
  requestHeaders: Record<string, string>;
  /** 响应头（键值对，header 名小写） */
  responseHeaders: Record<string, string>;
  /** 时间戳（毫秒） */
  timestamp: number;
}

/**
 * 创建服务参数
 */
export interface CreateServiceParams {
  name: string;
  listenAddr: string;
  targetUrl: string;
}

/**
 * 更新服务参数
 */
export interface UpdateServiceParams {
  id: string;
  name: string;
  listenAddr: string;
  targetUrl: string;
}

/**
 * 创建规则参数
 *
 * 新规则没有响应变体；创建后用 addVariant 追加。
 */
export interface CreateRuleParams {
  serviceId: string;
  urlPattern: string;
  isRegex?: boolean;
  method?: Method;
  enabled: boolean;
  forwardAndRecord?: boolean;
  /** 默认响应内容（useScript 为 true 时是 JS 脚本） */
  mockResponse: string;
  delayMs?: number | null;
  useScript?: boolean;
}

/**
 * 更新规则参数（只传要改的字段，未传的保持不变）
 *
 * 多个页面同时编辑同一规则时各自只写自己改过的字段，互不覆盖。
 * 响应变体与激活项不在这里，由 addVariant / selectVariant 等专用接口维护。
 */
export interface UpdateRuleParams {
  id: string;
  urlPattern?: string;
  isRegex?: boolean;
  /** HTTP 方法（ALL/GET/POST/PUT/DELETE，大小写不敏感） */
  method?: Method | string;
  enabled?: boolean;
  forwardAndRecord?: boolean;
  /** 默认响应内容（useScript 为 true 时是 JS 脚本） */
  mockResponse?: string;
  delayMs?: number | null;
  useScript?: boolean;
}

/**
 * 规则变更事件（从后端推送，type 为区分字段的标签）
 *
 * 规则被任一入口（本页面、MCP、将来的 web 端）改动后推送；前端据此局部刷新。
 */
export type RuleChangeEvent =
  | { type: "updated"; rule: MockRule }
  | { type: "deleted"; ruleId: string; serviceId: string };
