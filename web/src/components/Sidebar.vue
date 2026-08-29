<script setup lang="ts">
import { ref, reactive, onMounted } from "vue";
import { useRouter, useRoute } from "vue-router";
import { ElMessage } from "element-plus";
import { listServices, addService } from "@/api";
import type { MockService } from "@/types";
import { useLayoutStore } from "@/stores";

const router = useRouter();
const layoutStore = useLayoutStore();
const dialogVisible = ref(false);
const services = ref<MockService[]>([]);

const form = reactive({
  name: "",
  listenAddr: "",
  targetUrl: "",
});

const initForm = {
  name: "",
  listenAddr: "127.0.0.1:8080",
  targetUrl: "",
};

const route = useRoute();
const activeMenu = computed(() => {
  const serviceId = route.query.id
  if (serviceId) {
    return serviceId + ''
  }
  return '-1'
})

/**
 * 是否在服务配置页（/proxy）：该页存在规则列表侧边栏，按钮文案为「规则列表」；
 * 其他页（如 /network）按钮文案为「展开/收起」。
 */
const isProxyRoute = computed(() => route.path === "/proxy");

/**
 * navbar 收窄状态：全局持久（navVisible），跨路由保持；
 * navVisible=true 时 navbar 收窄为精简宽，两页一致，避免切换突兀。
 */
const isNarrow = computed(() => layoutStore.navVisible);

/**
 * 底部按钮配置：同一状态在不同页面含义不同。
 * - 规则页：控制规则列表显隐，文案固定「规则列表」。
 * - 其他页：控制 navbar 宽窄，navVisible=true（收窄）→「展开」，false（全宽）→「收起」。
 */
const toggleBtn = computed(() => {
  if (isProxyRoute.value) {
    return {
      text: "规则列表",
      icon: layoutStore.navVisible ? "⟨" : "⟩",
      title: layoutStore.navVisible ? "隐藏规则列表" : "显示规则列表",
    };
  }
  return {
    text: layoutStore.navVisible ? "展开" : "收起",
    icon: layoutStore.navVisible ? "⟩" : "⟨",
    title: layoutStore.navVisible ? "展开" : "收起",
  };
});

function handleToggleRules() {
  layoutStore.toggleNav();
}

function resetForm() {
  Object.assign(form, initForm);
}

function showAddDialog() {
  resetForm();
  dialogVisible.value = true;
}

function closeDialog() {
  resetForm();
  dialogVisible.value = false;
}

async function handleAddService() {
  await addService({
    name: form.name,
    listenAddr: form.listenAddr,
    targetUrl: form.targetUrl,
  });
  dialogVisible.value = false;
  resetForm();
  await loadServices();
  ElMessage({
    message: "新增成功！",
    type: "success",
  });
}

async function loadServices() {
  services.value = await listServices();
}

function handleSelect(index: string) {
  if (index === "-1") {
    router.push({ path: "/network" });
    return;
  }
  const item = services.value.filter((s) => s.id === index)?.[0];
  if (item) {
    router.push({ path: "/proxy", query: { id: item.id, _t: Date.now() } });
  } else {
    router.push({ path: "/network" });
  }
}

onMounted(() => {
  loadServices();
});
</script>

<template>
  <el-aside :width="isNarrow ? '160px' : '240px'" class="aside" :class="{ 'is-narrow': isNarrow }">
    <div class="sidebar-header">
      <h1 class="logo">Mockproxyrs</h1>
    </div>

    <div class="sidebar-content">
      <div class="section-title">服务列表</div>
      <el-button class="add-btn" @click="showAddDialog">
        <span class="add-icon">+</span>
        新建服务
      </el-button>

      <el-menu :default-active="activeMenu" class="service-menu" @select="handleSelect">
        <el-menu-item index="-1">
          <div class="menu-item-content">
            <span class="menu-icon">🌐</span>
            <span class="menu-text">Network</span>
          </div>
        </el-menu-item>
        <el-menu-item v-for="(item, _index) in services" :key="item.id" :index="item.id + ''">
          <div class="menu-item-content">
            <span class="menu-icon">⚡</span>
            <span class="menu-text">{{ item.name }}</span>
          </div>
        </el-menu-item>
      </el-menu>
    </div>

    <!-- 底部按钮：同一状态按页面区分语义（规则页=规则列表，其他页=展开/收起） -->
    <div class="sidebar-footer">
      <button
        class="nav-toggle-btn"
        type="button"
        :title="toggleBtn.title"
        @click="handleToggleRules"
      >
        <span class="nav-toggle-btn__icon">{{ toggleBtn.icon }}</span>
        <span class="nav-toggle-btn__text">{{ toggleBtn.text }}</span>
      </button>
    </div>

    <teleport to="body">
        <el-dialog v-model="dialogVisible" title="新建 Mock 服务" width="480px" @close="closeDialog">
        <el-form :model="form" label-position="top" class="dialog-form">
            <el-form-item label="服务名称">
            <el-input v-model="form.name" placeholder="输入服务名称" />
            </el-form-item>
            <el-form-item label="监听地址">
            <el-input v-model="form.listenAddr" placeholder="127.0.0.1:8080" />
            </el-form-item>
            <el-form-item label="目标地址">
            <el-input v-model="form.targetUrl" placeholder="https://example.com" />
            </el-form-item>
        </el-form>
        <template #footer>
            <div class="dialog-footer">
            <el-button @click="dialogVisible = false">取消</el-button>
            <el-button type="primary" @click="handleAddService">创建</el-button>
            </div>
        </template>
        </el-dialog>
    </teleport>
  </el-aside>
</template>

<style scoped>
.aside {
  background: rgba(255, 255, 255, 0.8);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-right: 1px solid rgba(0, 0, 0, 0.08);
  display: flex;
  flex-direction: column;
}

.sidebar-header {
  padding: 24px 20px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.06);
}

.logo {
  font-size: 22px;
  font-weight: 700;
  color: #1d1d1f;
  letter-spacing: -0.5px;
  margin: 0;
}

.sidebar-content {
  flex: 1;
  padding: 16px 12px;
  overflow-y: auto;
}

/* 收窄态：减少内边距，缩小 logo */
.aside.is-narrow .sidebar-header {
  padding: 24px 16px;
}

.aside.is-narrow .logo {
  font-size: 19px;
}

.aside.is-narrow .sidebar-content {
  padding: 16px 10px;
}

/* 底部切换按钮区 */
.sidebar-footer {
  padding: 12px;
  border-top: 1px solid rgba(0, 0, 0, 0.06);
}

.nav-toggle-btn {
  width: 100%;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  border: none;
  background: rgba(0, 0, 0, 0.04);
  border-radius: 10px;
  cursor: pointer;
  color: #86868b;
  font-size: 13px;
  font-weight: 500;
  transition: all 0.2s ease;
}

.nav-toggle-btn:hover {
  background: rgba(0, 0, 0, 0.08);
  color: #1d1d1f;
}

.nav-toggle-btn__icon {
  font-size: 16px;
  line-height: 1;
  display: inline-flex;
  align-items: center;
}

.section-title {
  font-size: 11px;
  font-weight: 600;
  color: #86868b;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  padding: 8px 12px;
  margin-bottom: 8px;
}

.add-btn {
  width: 100%;
  height: 44px;
  background: #0071e3 !important;
  border: none !important;
  color: #fff !important;
  font-size: 14px;
  font-weight: 500;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  margin-bottom: 16px;
  border-radius: 10px !important;
  transition: all 0.2s ease !important;
}

.add-btn:hover {
  background: #0077ed !important;
  transform: scale(1.02);
}

.add-icon {
  font-size: 18px;
  font-weight: 400;
}

.service-menu {
  border: none;
  background: transparent;
}

.menu-item-content {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
}

.menu-icon {
  font-size: 16px;
  opacity: 0.8;
}

.menu-text {
  font-size: 14px;
  font-weight: 500;
}

.dialog-form {
  padding: 8px 0;
}

.dialog-form :deep(.el-form-item__label) {
  font-weight: 500;
  color: #1d1d1f;
  padding-bottom: 8px;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
}

.dialog-footer .el-button {
  min-width: 80px;
  height: 40px;
  font-weight: 500;
}
</style>
