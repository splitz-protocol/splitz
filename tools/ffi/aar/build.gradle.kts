// An Android wallet that depends on the packaged splitz AAR and nothing else.
//
// The dependency is the artefact, not the sources: `splitzAar` is the path to
// `splitz-release.aar`, and no generated Kotlin is compiled here. Whatever the
// AAR does not carry, the consumer does not get.
//
// The bill runs as a JVM unit test, so the native library it loads is the HOST
// cdylib named by `splitzLibDir` — never one of the four Android `.so` files
// the AAR carries under `jni/`. A macOS or Linux JVM cannot load those. The
// test prints the file JNA resolved so the run says which binary it executed.
plugins {
    id("com.android.library")
}

val splitzAar: String = providers.gradleProperty("splitzAar").get()
val splitzLibDir: String = providers.gradleProperty("splitzLibDir").get()

android {
    namespace = "cash.splitz.consumer"
    compileSdk = 36
    defaultConfig {
        minSdk = 21
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    // The one line a wallet writes. `files(...)` is how a local AAR is
    // declared; a published one would be a coordinate, and reaches the
    // consumer the same way.
    implementation(files(splitzAar))

    // JNA as a plain jar, not the `@aar` the module declares: an AAR's JNA
    // carries Android `.so` files and no `libjnidispatch` for this host, so a
    // JVM test would have a binding it cannot dispatch through. An AAR
    // assembled by `assembleRelease` carries no POM either, so the consumer
    // states this itself.
    testImplementation("net.java.dev.jna:jna:5.17.0")
    testImplementation("junit:junit:4.13.2")

    // On a device the picture inverts: JNA must be the `@aar`, because that
    // is the variant carrying `libjnidispatch.so` for each Android ABI. The
    // plain jar has only host dispatch libraries and cannot bind there.
    androidTestImplementation("net.java.dev.jna:jna:5.17.0@aar")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
}

tasks.withType<Test>().configureEach {
    systemProperty("jna.library.path", splitzLibDir)

    // The native library this run loads is not one of Gradle's declared
    // inputs, so a rebuilt `.dylib` leaves the task looking unchanged and the
    // lane reports a cached green without executing a line. A lane that can
    // pass without running is worse than no lane: it runs every time.
    outputs.upToDateWhen { false }
    outputs.cacheIf { false }
    testLogging {
        showStandardStreams = true
        events("passed", "failed")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}
