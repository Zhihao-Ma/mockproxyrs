<script setup lang="ts">
import { ref, reactive, onMounted } from "vue";
import { useRouter, useRoute } from "vue-router";
import { ElMessage } from "element-plus";
import { listServices, addService } from "@/api";
import type { MockService } from "@/types";

const router = useRouter();
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
  <el-aside width="240px" class="aside">
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
