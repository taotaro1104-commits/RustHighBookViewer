import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "jp.perfectviewer.rs"
    compileSdk = 35

    defaultConfig {
        applicationId = "jp.perfectviewer.rs"
        minSdk = 26
        targetSdk = 35
        versionCode = 5
        versionName = "0.1.4"
        // Rust 側でビルドする ABI と一致させる
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64") }
    }

    // リリース署名。秘密情報は Git に入れない keystore.properties から読む（無ければ署名なしでビルド）
    // 必要なキー: storeFile / storePassword / keyAlias / keyPassword
    val keystoreProps = Properties().apply {
        val f = rootProject.file("keystore.properties")
        if (f.exists()) f.inputStream().use { load(it) }
    }
    val hasSigning = listOf("storeFile", "storePassword", "keyAlias", "keyPassword").all { keystoreProps.containsKey(it) }
    signingConfigs {
        if (hasSigning) {
            create("release") {
                storeFile = file(keystoreProps.getProperty("storeFile"))
                storePassword = keystoreProps.getProperty("storePassword")
                keyAlias = keystoreProps.getProperty("keyAlias")
                keyPassword = keystoreProps.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            if (hasSigning) signingConfig = signingConfigs.getByName("release")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
    packaging {
        jniLibs { useLegacyPackaging = false }
    }
}

// Rust の .so と UniFFI の Kotlin バインディングを生成する（-PskipRust で省略）
val rustRoot = rootProject.projectDir.resolve("../..")
val buildRust by tasks.registering(Exec::class) {
    workingDir = rustRoot
    val profile = if (gradle.startParameter.taskNames.any { it.contains("Release", ignoreCase = true) }) "release" else "debug"
    // Windows では System32 の bash (WSL) ではなく Git for Windows の bash を使う
    val gitBash = file("C:/Program Files/Git/bin/bash.exe")
    val bash = if (org.gradle.internal.os.OperatingSystem.current().isWindows && gitBash.exists()) gitBash.path else "bash"
    commandLine(bash, "./build-android.sh", profile)
    onlyIf { !project.hasProperty("skipRust") }
}
tasks.matching { it.name == "preBuild" }.configureEach { dependsOn(buildRust) }

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.10.01"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.documentfile:documentfile:1.0.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    // UniFFI の Kotlin バインディングが使う
    implementation("net.java.dev.jna:jna:5.15.0@aar")
}
