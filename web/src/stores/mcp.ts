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
 * MCP 服务器状态管理
 *
 * MCP 服务器随桌面应用进程运行。这里维护运行状态、端口与「启动应用时自动开启」偏好
 * （localStorage 持久化），供侧边栏状态入口与配置弹窗共用。
 */

import { defineStore } from "pinia";
import { computed, reactive, ref, watch } from "vue";
import { getMcpStatus, startMcpServer, stopMcpServer } from "@/api";
import type { McpStatus } from "@/types";

const PREFS_KEY = "mockproxyrs:mcp";
/** 与后端 DEFAULT_MCP_PORT 一致 */
const DEFAULT_PORT = 27120;

interface McpPrefs {
  port: number;
  autoStart: boolean;
}

function loadPrefs(): McpPrefs {
  try {
    const raw = JSON.parse(localStorage.getItem(PREFS_KEY) || "{}");
    return {
      port: Number(raw.port) || DEFAULT_PORT,
      autoStart: raw.autoStart === true,
    };
  } catch {
    return { port: DEFAULT_PORT, autoStart: false };
  }
}

export const useMcpStore = defineStore("mcp", () => {
  /** 用户偏好：端口 + 是否随应用自动启动 */
  const prefs = reactive<McpPrefs>(loadPrefs());
  /** 后端实际状态（以后端为准，前端不自行推断） */
  const status = ref<McpStatus>({ running: false, port: null, url: null });
  const dialogVisible = ref(false);

  watch(prefs, (p) => localStorage.setItem(PREFS_KEY, JSON.stringify(p)), { deep: true });

  const running = computed(() => status.value.running);

  /** 同步后端状态 */
  async function refresh() {
    status.value = await getMcpStatus();
  }

  async function start() {
    status.value = await startMcpServer(prefs.port);
  }

  async function stop() {
    await stopMcpServer();
    status.value = await getMcpStatus();
  }

  async function toggle() {
    if (running.value) {
      await stop();
    } else {
      await start();
    }
  }

  /** 应用启动时调用：按偏好自动开启，否则只同步状态（热更新/重载后仍能显示真实状态） */
  async function init() {
    if (prefs.autoStart) {
      // 启动失败（如端口被占用）时 API 层已弹出提示，这里仅同步状态
      await start().catch(() => refresh());
    } else {
      await refresh();
    }
  }

  function openDialog() {
    dialogVisible.value = true;
  }

  return {
    prefs,
    status,
    running,
    dialogVisible,
    refresh,
    start,
    stop,
    toggle,
    init,
    openDialog,
  };
});
