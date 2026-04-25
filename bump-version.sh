#!/bin/bash
set -euo pipefail

# ===================== 自动版本升级 =====================
echo "🔍 读取当前版本..."

TAURI_CONF="desktop/src-tauri/tauri.conf.json"
CURR_VER=$(grep -oE '"version": "[0-9]+\.[0-9]+\.[0-9]+"' "$TAURI_CONF" | cut -d'"' -f4)

MAJOR=$(echo "$CURR_VER" | cut -d. -f1)
MINOR=$(echo "$CURR_VER" | cut -d. -f2)
PATCH=$(echo "$CURR_VER" | cut -d. -f3)
NEW_PATCH=$((PATCH + 1))
NEW_VER="${MAJOR}.${MINOR}.${NEW_PATCH}"

echo "✅ 当前版本: $CURR_VER"
echo "✅ 新版本:   $NEW_VER"
echo ""

# 要更新的文件
FILES=(
  "web/package.json"
  "desktop/package.json"
  "desktop/src-tauri/tauri.conf.json"
  "desktop/src-tauri/Cargo.toml"
)

# 批量替换版本
for f in "${FILES[@]}"; do
  echo "🔄 更新: $f"
  sed -i.bak -E "s/\"version\": \"[0-9]+\.[0-9]+\.[0-9]+\"/\"version\": \"${NEW_VER}\"/g" "$f"
  sed -i.bak -E "s/version = \"[0-9]+\.[0-9]+\.[0-9]+\"/version = \"${NEW_VER}\"/g" "$f"
  rm -f "${f}.bak"
done

echo ""

# ===================== 自动安装依赖 =====================
echo "📦 安装 web 依赖..."
cd web && npm install && cd ..

echo "📦 安装 desktop 依赖..."
cd desktop && npm install && cd ..

echo ""
echo "🎉 版本升级完成！新版本: $NEW_VER"
echo ""
echo "👉 提交命令："
echo "   git add ."
echo "   git commit -m \"chore: bump version to $NEW_VER\""
