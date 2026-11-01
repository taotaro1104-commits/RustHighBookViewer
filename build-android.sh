#!/usr/bin/env bash
# Android 用の .so (3 ABI) と Kotlin バインディングを生成する。
# 使い方: ./build-android.sh [debug|release]   前提: ANDROID_NDK_HOME, cargo-ndk, Rust の android ターゲット
set -euo pipefail
cd "$(dirname "$0")"
PROFILE="${1:-release}"
FLAG=$([ "$PROFILE" = release ] && echo --release || echo "")
APP=apps/android/app/src/main
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o "$APP/jniLibs" build -p pv-ffi $FLAG
# 依存クレート（sevenz-rust2）が cdylib も出すため、cargo-ndk が余分な .so を集めてしまう。pv_ffi 以外は不要
find "$APP/jniLibs" -name "*.so" ! -name "libpv_ffi.so" -delete
# ライブラリモードでバインディングを生成する（.so の埋め込みメタデータを読む）
LIB="target/aarch64-linux-android/$PROFILE/libpv_ffi.so"
cargo run -q -p pv-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library "$LIB" --language kotlin --no-format --out-dir "$APP/java"
echo "done: $APP/jniLibs, $APP/java/uniffi"
