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
 * 全局布局状态管理
 *
 * 协调左侧主侧边栏（navbar）与规则列表侧边栏的联动：
 * - navVisible：规则列表侧边栏是否可见。
 * - 规则列表展开时 navbar 收窄为精简宽，隐藏时恢复全宽，
 *   由 Sidebar（navbar）根据当前路由 + navVisible 综合计算宽度。
 */

import { defineStore } from "pinia";
import { ref } from "vue";

export const useLayoutStore = defineStore("layout", () => {
  /** 规则列表侧边栏是否可见（默认展开） */
  const navVisible = ref(true);

  function toggleNav() {
    navVisible.value = !navVisible.value;
  }

  return {
    navVisible,
    toggleNav,
  };
});