plugins {
    kotlin("jvm") version "2.0.21"
    application
}

repositories {
    mavenCentral()
}

dependencies {
    implementation("net.java.dev.jna:jna:5.17.0")
    implementation("com.google.code.gson:gson:2.11.0")
}

sourceSets {
    main {
        kotlin.srcDir("..")
        kotlin.srcDir(layout.buildDirectory.dir("generated"))
    }
}

application {
    mainClass.set("Native_smokeKt")
}

tasks.named<JavaExec>("run") {
    val repoRoot = projectDir.resolve("../../..").canonicalFile
    systemProperty("jna.library.path", repoRoot.resolve("target/debug").absolutePath)
    args(repoRoot.absolutePath)
}
