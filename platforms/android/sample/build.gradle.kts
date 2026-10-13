import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "dev.openrich.sample"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.openrich.sample"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1"
        ndk {
            abiFilters += providers.gradleProperty("openrichAbi").orNull ?: "arm64-v8a"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin {
        compilerOptions {
            jvmTarget = JvmTarget.JVM_17
        }
    }
    buildFeatures {
        compose = true
    }
    packaging {
        // Keep the native library uncompressed and 16 KB aligned in the APK.
        jniLibs {
            useLegacyPackaging = false
        }
    }
}

dependencies {
    implementation(project(":openrich-compose"))
    implementation(libs.androidx.core.ktx)
    // Newer lifecycle than activity-compose asks for (as in rs-organizer).
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.material3)
}
