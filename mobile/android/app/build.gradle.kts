plugins {
    id("com.android.application") version "8.7.3"
    id("org.jetbrains.kotlin.android") version "2.1.21"
    id("org.jetbrains.kotlin.plugin.compose") version "2.1.21"
    id("org.jetbrains.kotlin.plugin.serialization") version "2.1.21"
}

android {
    namespace = "io.ahara.rollinglocal.operator"
    compileSdk = 35

    defaultConfig {
        applicationId = "io.ahara.rollinglocal.operator"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        manifestPlaceholders["appAuthRedirectScheme"] = "rollinglocal"
        buildConfigField("String", "API_ORIGIN", "\"https://api.rolling-local.ahara.io\"")
    }

    buildTypes {
        debug {
            providers.gradleProperty("rollingLocalApiOrigin").orNull?.let {
                require(it.matches(Regex("https?://[a-zA-Z0-9.:-]+"))) { "Set an HTTP(S) API origin." }
                buildConfigField("String", "API_ORIGIN", "\"$it\"")
            }
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin { jvmToolchain(17) }

dependencies {
    implementation(platform("androidx.compose:compose-bom:2025.05.01"))
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.8.1")
    implementation("net.openid:appauth:0.11.1")
}
