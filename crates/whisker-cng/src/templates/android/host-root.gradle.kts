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
