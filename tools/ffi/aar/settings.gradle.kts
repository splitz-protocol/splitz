// A wallet's own build, not this repository's: the splitz module is not a
// project here. It is a binary the consumer resolves like any other.
pluginManagement {
    repositories { google(); mavenCentral(); gradlePluginPortal() }
    plugins {
        id("com.android.library") version "9.4.1"
    }
}
dependencyResolutionManagement {
    repositories { google(); mavenCentral() }
}
rootProject.name = "aar-consumer"
