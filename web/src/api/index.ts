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
 * API 接口层
 *
 * 封装所有 Tauri 命令调用，提供类型安全的 API 接口。
 * 所有错误会通过 ElMessage 显示，并重新抛出。
 */

import { invoke } from "@tauri-apps/api/core";
import { Channel } from "@tauri-apps/api/core";
import { ElMessage } from "element-plus";
import type {
  MockService,
  MockServiceDetail,
  MockRule,
  ServiceStatus,
  ResponseEvent,
  CreateServiceParams,
  UpdateServiceParams,
  CreateRuleParams,
  UpdateRuleParams,
} from "@/types";

/**
 * 统一请求封装
 *
 * 调用 Tauri 命令，自动处理错误并显示提示。
 */
async function request<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    ElMessage({
      message: String(error),
      type: "error",
    });
    throw error;
  }
}

// ==================== 服务管理 ====================

/** 获取所有服务 */
export function listServices(): Promise<MockService[]> {
  return request<MockService[]>("list_services");
}

/** 获取服务状态列表 */
export function listServiceStatus(): Promise<ServiceStatus[]> {
  return request<ServiceStatus[]>("list_service_status");
}

/** 获取单个服务 */
export function getService(id: string): Promise<MockServiceDetail | null> {
  return request<MockServiceDetail | null>("get_service", { id });
}

/** 添加服务 */
export function addService(params: CreateServiceParams): Promise<string> {
  return request<string>("add_service", { ...params });
}

/** 更新服务 */
export function updateService(params: UpdateServiceParams): Promise<void> {
  return request<void>("update_service", { ...params });
}

/** 删除服务 */
export function deleteService(id: string): Promise<void> {
  return request<void>("delete_service", { id });
}

/** 启动服务 */
export function startService(id: string): Promise<void> {
  return request<void>("start_service", { id });
}

/** 停止服务 */
export function stopService(id: string): Promise<void> {
  return request<void>("stop_service", { id });
}

// ==================== 规则管理 ====================

/** 获取服务的规则列表 */
export function listRules(serviceId: string): Promise<MockRule[]> {
  return request<MockRule[]>("list_rules", { serviceId });
}

/** 添加规则 */
export function addRule(params: CreateRuleParams): Promise<string> {
  return request<string>("add_rule", { params });
}

/** 更新规则 */
export function updateRule(params: UpdateRuleParams): Promise<void> {
  return request<void>("update_rule", { ...params });
}

/** 删除规则 */
export function deleteRule(id: string): Promise<void> {
  return request<void>("delete_rule", { id });
}

/** 删除服务的所有规则 */
export function deleteRulesByService(serviceId: string): Promise<void> {
  return request<void>("delete_rules_by_service", { serviceId });
}

/** 校验 JS 脚本语法（仅诊断，不影响保存） */
export function validateScript(script: string): Promise<void> {
  return request<void>("validate_script", { script });
}

// ==================== 事件通道 ====================

/** 创建事件通道 */
export async function createChannel(onMessage: (event: ResponseEvent) => void): Promise<void> {
  const channel = new Channel<ResponseEvent>();
  channel.onmessage = onMessage;
  await request<void>("create_channel", { onEvent: channel });
}

/** 销毁事件通道 */
export function destroyChannel(): Promise<void> {
  return request<void>("destroy_channel");
}
