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
  /** mock 响应内容 */
  mockResponse: string;
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
  /** mock 响应内容 */
  mockBody: string | null;
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
 */
export interface CreateRuleParams {
  serviceId: string;
  urlPattern: string;
  isRegex?: boolean;
  method?: Method;
  enabled: boolean;
  forwardAndRecord?: boolean;
  mockResponse: string;
}

/**
 * 更新规则参数
 */
export interface UpdateRuleParams {
  id: string;
  serviceId: string;
  urlPattern: string | null;
  isRegex: boolean;
  method: Method | string;
  enabled: boolean;
  forwardAndRecord: boolean;
  mockResponse: string;
}
