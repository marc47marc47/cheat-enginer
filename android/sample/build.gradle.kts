import java.util.Properties

plugins {
    id("com.android.application")
}

android {
    namespace = "dev.marc.ce.sample"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.marc.ce.sample"
        minSdk = 24
        targetSdk = 35
        versionCode = 2
        versionName = "0.3.1"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
}

dependencies {
    // debugImplementation only. The sample's source never references the
    // overlay - it installs itself from its own ContentProvider - so the
    // release variant compiles with the library completely absent. That is the
    // property `verifyOverlayNotInRelease` below checks.
    debugImplementation(project(":overlay"))
}

/**
 * Fails the build if the engine ever reaches a release APK. Cheap insurance
 * against someone turning `debugImplementation` into `implementation`.
 */
val verifyOverlayNotInRelease by tasks.registering {
    group = "verification"
    description = "Assert libce_engine.so is absent from the release APK"
    dependsOn("assembleRelease")
    doLast {
        val apks = fileTree(layout.buildDirectory.dir("outputs/apk/release")) {
            include("*.apk")
        }
        apks.forEach { apk ->
            val leaked = zipTree(apk).matching { include("**/libce_engine.so") }.files
            check(leaked.isEmpty()) { "overlay engine leaked into $apk" }
        }
    }
}

// ---------------------------------------------------------------------------
// 共用簽章設定
//
// sharedUserId 要求所有參與的 APK 用「同一張憑證」簽名，所以這段在
// :cheatengine、:game、:sample 三個模組裡是刻意重複的 —— 任何一支漏掉，
// 安裝時會得到 INSTALL_FAILED_SHARED_USER_INCOMPATIBLE，而那個錯誤訊息
// 完全指不到重點。
//
// android/signing.properties 存在就用它（由 ./keystore.sh 產生、已 gitignore），
// 不存在就沿用 AGP 預設的 debug keystore，開發流程完全不變。
// ---------------------------------------------------------------------------
val sharedSigning: Properties? =
    rootProject.file("signing.properties").let { f ->
        if (!f.exists()) null else Properties().apply { f.inputStream().use { load(it) } }
    }

android {
    if (sharedSigning != null) {
        signingConfigs {
            create("shared") {
                storeFile = rootProject.file(sharedSigning.getProperty("storeFile"))
                storePassword = sharedSigning.getProperty("storePassword")
                keyAlias = sharedSigning.getProperty("keyAlias")
                keyPassword = sharedSigning.getProperty("keyPassword")
            }
        }
        buildTypes {
            getByName("debug") { signingConfig = signingConfigs.getByName("shared") }
            getByName("release") { signingConfig = signingConfigs.getByName("shared") }
        }
    }
}
