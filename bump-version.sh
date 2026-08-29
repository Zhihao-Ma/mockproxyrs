#!/bin/bash
set -euo pipefail

# ===================== 自动版本升级 =====================
# 用法: ./bump-version.sh [major|minor|patch]  (默认 patch)
echo "🔍 读取当前版本..."

TAURI_CONF="desktop/src-tauri/tauri.conf.json"
CURR_VER=$(grep -oE '"version": "[0-9]+\.[0-9]+\.[0-9]+"' "$TAURI_CONF" | cut -d'"' -f4)
[ -n "$CURR_VER" ] || { echo "❌ 无法从 $TAURI_CONF 读取版本号" >&2; exit 1; }

MAJOR=$(echo "$CURR_VER" | cut -d. -f1)
MINOR=$(echo "$CURR_VER" | cut -d. -f2)
PATCH=$(echo "$CURR_VER" | cut -d. -f3)

BUMP="${1:-patch}"
case "$BUMP" in
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  patch) PATCH=$((PATCH + 1)) ;;
  *) echo "❌ 未知版本级别: $BUMP（可选: major / minor / patch）" >&2; exit 1 ;;
esac
NEW_VER="${MAJOR}.${MINOR}.${PATCH}"

echo "✅ 当前版本: $CURR_VER"
echo "✅ 升级级别: $BUMP"
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

# Cargo.lock 单独精确处理：只改 name = "mockproxyrs" 块内的 version，
# 避免误改 alloc-stdlib / rand_chacha 等恰好同版本的第三方依赖
CARGO_LOCK="Cargo.lock"
if [ -f "$CARGO_LOCK" ]; then
  echo "🔄 更新: $CARGO_LOCK"
  awk -v new="$NEW_VER" '
    /^name = "mockproxyrs"$/ { found=1; print; next }
    found && /^version = / {
      gsub(/"[0-9]+\.[0-9]+\.[0-9]+"/, "\"" new "\"")
      found=0
    }
    { print }
  ' "$CARGO_LOCK" > "$CARGO_LOCK.tmp" && mv "$CARGO_LOCK.tmp" "$CARGO_LOCK"
fi

echo ""

# ===================== 自动安装依赖 =====================
echo "📦 安装 web 依赖..."
(cd web && npm install)

echo "📦 安装 desktop 依赖..."
(cd desktop && npm install)

echo ""
echo "🎉 版本升级完成！新版本: $NEW_VER"
echo ""
echo "👉 提交命令（精确 add，勿用 git add . 以免卷入无关文件）："
echo "   git add web/package.json web/package-lock.json desktop/package.json desktop/package-lock.json desktop/src-tauri/tauri.conf.json desktop/src-tauri/Cargo.toml Cargo.lock"
echo "   git commit -m \"chore: bump version to $NEW_VER\""
