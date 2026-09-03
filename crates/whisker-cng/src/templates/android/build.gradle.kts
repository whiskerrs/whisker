// Root build file. Module-specific configuration in app/build.gradle.kts.

plugins {
    id("com.android.application") version "8.6.1" apply false
    id("com.android.library") version "8.6.1" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
}

// Module packages are independently publishable, so their build scripts depend
// on the published authoring API coordinate. Inside a Whisker checkout, replace
// that coordinate for every included module subproject with the in-tree SDK.
// This keeps module build.gradle.kts files identical in standalone and
// workspace builds and prevents an older published SDK from reintroducing
// legacy renderer dependencies into the generated app.
subprojects {
    configurations.configureEach {
        resolutionStrategy.dependencySubstitution {
            if (rootProject.findProject(":whisker-module") != null) {
                substitute(module("rs.whisker:whisker-module-android"))
                    .using(project(":whisker-module"))
            }
        }
    }
}
