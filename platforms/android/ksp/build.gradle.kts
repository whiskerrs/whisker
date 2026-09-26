// Root build script — no plugins applied here; per-module
// `build.gradle.kts` brings them in.

plugins {
    kotlin("jvm") version "2.4.20" apply false
}

allprojects {
    group = "rs.whisker"
    version = "0.1.0"
}
