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
 * - logData: 请求日志列表（最新在前，最多承载 2×MAX_LOG_DATA）
 * - logDataView: 仅展示前 MAX_LOG_DATA 条
 */

import { defineStore } from "pinia";
import { ref, computed } from "vue";
import type { ResponseEvent } from "@/types";

/** 展示的日志条数上限（logData 实际可承载 2 倍，超出后一次性截断） */
const MAX_LOG_DATA = 50;

export const useNetworkStore = defineStore("network", () => {
  /** 是否正在录制 */
  const recording = ref(false);

  /** 请求日志列表（最新在前，最多承载 2×MAX_LOG_DATA） */
  const logData = ref<ResponseEvent[]>([]);

  /** 仅展示前 MAX_LOG_DATA 条（最新请求在前） */
  const logDataView = computed(() => logData.value.slice(0, MAX_LOG_DATA));

  /** 切换录制状态 */
  function toggleRecording() {
    recording.value = !recording.value;
  }

  /**
   * 添加日志记录（最新在前）。
   *
   * 平时直接 unshift 到头部；累计达到 2 倍上限时，一次性截断尾部
   * （最旧的部分），只保留前 MAX_LOG_DATA 条。把"每次 push 都清理"
   * 摊销成"每 MAX 条才清理一次"。
   */
  function pushLogData(data: ResponseEvent) {
    logData.value.unshift(data);
    if (logData.value.length >= 2 * MAX_LOG_DATA) {
      logData.value.splice(MAX_LOG_DATA);
    }
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
