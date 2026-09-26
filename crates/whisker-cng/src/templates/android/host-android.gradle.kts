    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    val whiskerKeystore = System.getenv("WHISKER_ANDROID_KEYSTORE")
    if (whiskerKeystore != null) {
        signingConfigs {
            create("whiskerRelease") {
                storeFile = file(whiskerKeystore)
                storePassword = System.getenv("WHISKER_ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("WHISKER_ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("WHISKER_ANDROID_KEY_PASSWORD")
            }
        }
    }
