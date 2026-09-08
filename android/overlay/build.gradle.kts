plugins {
    id("com.android.library")
}

// Where the Rust engine lives, and how to reach the NDK. Both overridable from
// gradle.properties or -P so this builds on a machine that is not this one.
val repoRoot = rootProject.file("..")
val cargoExecutable = (findProperty("ce.cargo") as String?) ?: "cargo"
val ndkHome = (findProperty("ce.ndk") as String?)
    ?: System.getenv("ANDROID_NDK_HOME")
    ?: "${android.sdkDirectory}/ndk/28.2.13676358"

android {
    namespace = "dev.marc.ce.overlay"
    compileSdk = 35

    defaultConfig {
        minSdk = 24
        // An Android library must not declare targetSdk - the host app's value
        // governs runtime behaviour, which is exactly what we want here.
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // Only so AGP can find llvm-strip for the .so it did not build itself;
    // nothing here compiles C.
    ndkVersion = "28.2.13676358"

    // Written by the cargoNdk task below; ignored by git.
    sourceSets["main"].jniLibs.srcDir("src/main/jniLibs")
}

/**
 * Build the Rust engine for both ABIs and drop the .so straight into a
 * jniLibs-shaped tree.
 *
 * `--no-default-features` is what keeps ratatui and crossterm out of the
 * Android build; `--lib` skips the TUI binary, which is gated behind
 * `required-features = ["tui"]` anyway.
 */
val cargoNdk by tasks.registering(Exec::class) {
    group = "build"
    description = "Cross-compile libce_engine.so for every shipped ABI"

    workingDir = repoRoot
    environment("ANDROID_NDK_HOME", ndkHome)
    commandLine(
        cargoExecutable, "ndk",
        // arm64-v8a covers every modern phone; armeabi-v7a is there so a
        // 32-bit-only device (minSdk 24 still allows one) gets a working
        // library instead of a silent UnsatisfiedLinkError; x86_64 is the
        // emulator.
        "-t", "arm64-v8a",
        "-t", "armeabi-v7a",
        "-t", "x86_64",
        "--platform", "24",
        "-o", file("src/main/jniLibs").absolutePath,
        "build", "--release", "--lib",
        "--no-default-features", "--features", "android",
    )

    inputs.dir(repoRoot.resolve("src"))
    inputs.file(repoRoot.resolve("Cargo.toml"))
    outputs.dir(file("src/main/jniLibs"))
}

// MergeSourceSetFolders is the task that actually consumes jniLibs, so hanging
// the dependency there is precise and survives variant-aware task graphs.
tasks.withType<com.android.build.gradle.tasks.MergeSourceSetFolders>().configureEach {
    dependsOn(cargoNdk)
}
