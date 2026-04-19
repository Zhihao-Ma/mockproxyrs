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
 * 路由配置
 *
 * 定义应用的路由结构：
 * - `/` - 首页，服务列表
 * - `/proxy` - 服务配置页，管理 Mock 规则
 * - `/network` - 网络监控页，实时查看请求
 */

import { createRouter, createWebHistory } from "vue-router";
import type { RouteRecordRaw } from "vue-router";

const routes: RouteRecordRaw[] = [
  {
    path: "/",
    name: "Home",
    redirect: "/network",
  },
  {
    path: "/proxy",
    name: "Proxy",
    component: () => import("@/views/proxy/index.vue"),
  },
  {
    path: "/network",
    name: "Network",
    component: () => import("@/views/network/index.vue"),
  },
];

const router = createRouter({
  history: createWebHistory(),
  routes,
});

export default router;
