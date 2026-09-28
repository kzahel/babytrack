import org.gradle.api.tasks.Exec

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "org.babytrack.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.babytrack.app"
        minSdk = 26
        targetSdk = 35
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        versionCode = 1
        versionName = "0.1.0"
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }

    sourceSets.getByName("main").apply {
        java.srcDir(layout.buildDirectory.dir("generated/rust/kotlin"))
        jniLibs.srcDir(layout.buildDirectory.dir("generated/rust/jniLibs"))
    }
}

val buildRust by tasks.registering(Exec::class) {
    val repoRoot = rootProject.projectDir.resolve("../..").canonicalFile
    val generated = layout.buildDirectory.dir("generated/rust")
    inputs.files(fileTree(repoRoot.resolve("core")) { include("src/**", "Cargo.toml") })
    inputs.files(fileTree(repoRoot.resolve("core-ffi")) { include("src/**", "Cargo.toml") })
    inputs.files(fileTree(repoRoot.resolve("wire")) { include("src/**", "Cargo.toml") })
    inputs.file(repoRoot.resolve("Cargo.lock"))
    outputs.dir(generated)
    commandLine("bash", repoRoot.resolve("scripts/build_android_core.sh"), generated.get().asFile)
}

tasks.named("preBuild") { dependsOn(buildRust) }

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.10.01")
    implementation(composeBom)
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation(composeBom)
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
}
