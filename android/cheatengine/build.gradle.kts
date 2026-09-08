import java.util.Properties

plugins {
    id("com.android.application")
}

android {
    namespace = "dev.marc.ce.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.marc.ce.app"
        minSdk = 24
        targetSdk = 35
        versionCode = 2
        versionName = "0.3.1"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    lint {
        // android:sharedUserId is deprecated since API 29 and lint says so.
        // It is also the only way two separately installed APKs can share one
        // process, which is the entire point of this build.
        disable += "Deprecated"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
}

dependencies {
    // implementation, not debugImplementation: unlike :sample, this app *is*
    // the tool. It also drags in the cargoNdk task transitively, so nothing
    // here has to know Rust exists.
    implementation(project(":overlay"))
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
