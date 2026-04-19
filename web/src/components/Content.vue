<template>
  <el-main class="main-content">
    <router-view />
  </el-main>
</template>

<script setup lang="ts"></script>

<style scoped>
.main-content {
  padding: 0;
  background: #f5f5f7;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
</style>
