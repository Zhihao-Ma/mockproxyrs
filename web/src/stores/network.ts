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
 * 网络监控状态管理
 *
 * 管理请求日志的存储和展示：
 * - recording: 是否正在录制
 * - logData: 请求日志列表
 * - logDataView: 反序展示（最新在前）
 */

import { defineStore } from "pinia";
import { ref, computed } from "vue";
import type { ResponseEvent } from "@/types";

/** 最大日志条数 */
const MAX_LOG_DATA = 50;

export const useNetworkStore = defineStore("network", () => {
  /** 是否正在录制 */
  const recording = ref(false);

  /** 请求日志列表 */
  const logData = ref<ResponseEvent[]>([]);

  /** 反序展示（最新请求在前） */
  const logDataView = computed(() => [...logData.value].reverse());

  /** 切换录制状态 */
  function toggleRecording() {
    recording.value = !recording.value;
  }

  /**
   * 添加日志记录
   *
   * 超过最大条数时，移除最早的记录。
   */
  function pushLogData(data: ResponseEvent) {
    if (logData.value.length >= MAX_LOG_DATA) {
      logData.value.shift();
    }
    logData.value.push(data);
  }

  /** 清空所有日志 */
  function clearLogData() {
    logData.value = [];
  }

  return {
    recording,
    logData,
    logDataView,
    toggleRecording,
    pushLogData,
    clearLogData,
  };
});
