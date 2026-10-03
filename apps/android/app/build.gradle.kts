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

    testOptions { unitTests.isIncludeAndroidResources = true }

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
    inputs.file(repoRoot.resolve("scripts/build_android_core.sh"))
    inputs.file(rootProject.projectDir.resolve("uniffi.toml"))
    outputs.dir(generated)
    commandLine("bash", repoRoot.resolve("scripts/build_android_core.sh"), generated.get().asFile)
}

tasks.named("preBuild") { dependsOn(buildRust) }

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.10.01")
    implementation(composeBom)
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.compose.material3:material3")
    // Apache-2.0 AndroidX icon set; activity icons per the interface design topic.
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.robolectric:robolectric:4.16.1")
    testImplementation(composeBom)
    testImplementation("androidx.compose.ui:ui-test-junit4")
    testImplementation("io.github.takahirom.roborazzi:roborazzi:1.39.0")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-compose:1.39.0")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
    androidTestImplementation(composeBom)
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.6.1")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
}

// Normal unit tests skip image generation; the gallery task records real Compose
// screens in Robolectric Native Graphics, without launching the production Activity.
val recordScreenGallery by tasks.registering {
    group = "verification"
    description = "Render synthetic Android screens for the scrolling gallery"
    dependsOn("testDebugUnitTest")
}
tasks.withType<Test>().configureEach {
    val recording = gradle.startParameter.taskNames.any { it.endsWith("recordScreenGallery") }
    systemProperty("babytrack.gallery.case", providers.gradleProperty("galleryCase").orElse("").get())
    val galleryViewport = providers.gradleProperty("galleryViewport").orElse("phone").get()
    require(galleryViewport in listOf("phone", "compact")) { "Use galleryViewport=phone or galleryViewport=compact" }
    val galleryDirectory = if (galleryViewport == "phone") "screen-gallery" else "screen-gallery-compact"
    systemProperty("babytrack.gallery.viewport", galleryViewport)
    systemProperty("babytrack.gallery.record", recording.toString())
    systemProperty("roborazzi.test.record", recording.toString())
    systemProperty("babytrack.gallery.output", layout.buildDirectory.dir("outputs/$galleryDirectory").get().asFile.absolutePath)
    if (recording) {
        filter { includeTestsMatching("org.babytrack.app.ScreenGalleryTest") }
        outputs.upToDateWhen { false }
    }
}
