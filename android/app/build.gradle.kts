/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 *
 * Parts of this file are derived from SDL 2's Android project template, which
 * has a different license. Please see vendor/SDL/LICENSE.txt for details.
 */
import org.gradle.nativeplatform.platform.internal.DefaultNativePlatform

plugins {
    id("com.android.application") version("8.10.1")
    id("com.github.willir.rust.cargo-ndk-android") version("0.3.4")
    id("org.jetbrains.kotlin.android") version("2.0.21")
}

android {
    ndkVersion = "25.2.9519653"
    compileSdk = 31
    buildFeatures {
        buildConfig = true
    }
    defaultConfig {
        resValue("string", "app_name", "SkyMRP")
        buildConfigField("String", "APP_NAME", "\"SkyMRP\"")
        manifestPlaceholders["icon"] = "@drawable/icon"
        buildConfigField("int", "APP_ICON", "R.drawable.icon")
        versionName = "0.2.0"

        minSdk = 21 // first version with AArch64
        targetSdk = 31
        externalNativeBuild {
            ndkBuild {
                arguments("APP_PLATFORM=android-21")
                abiFilters("arm64-v8a")
            }
        }
    }
    // The target JVM version must be the same for Java and Kotlin.
    compileOptions {
        sourceCompatibility(JavaVersion.VERSION_11)
        targetCompatibility(JavaVersion.VERSION_11)
    }
    kotlinOptions {
        jvmTarget = "11"
    }
    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("debug")
            isMinifyEnabled = false
            isDebuggable = true // allow use of ADB to manage files, etc
        }
        debug {
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("**/*.so")
            }
            isDebuggable = true
            isJniDebuggable = true
        }
    }

    applicationVariants.all {
        val variantName = name.replaceFirstChar { char ->
            if (char.isLowerCase()) char.titlecase() else char.toString()
        }
        tasks.named("merge${variantName}Assets").configure {
            dependsOn("externalNativeBuild${variantName}")
        }
    }

    sourceSets {
        getByName("main") {
            java.srcDir("${rootDir.parentFile}/vendor/SDL/android-project/app/src/main/java")
        }
    }

    if (!project.hasProperty("EXCLUDE_NATIVE_LIBS")) {
        sourceSets {
            getByName("main") {
                jniLibs.srcDir("${projectDir}/jniLibs")
            }
        }
        externalNativeBuild {
            ndkBuild {
                path("jni/Android.mk")
            }
        }
    }

    lint {
        abortOnError = false
    }
    namespace = "org.skymrp.android"
}

cargoNdk {
    // Make sure this matches the android abiFilters above.
    targets = arrayListOf("arm64")
    module = ".."
    librariesNames = arrayListOf("libskymrp.so", "libSDL2.so", "libc++_shared.so")
    extraCargoEnv = mapOf(
        "ANDROID_NDK" to android.ndkDirectory.toString(),
        "ANDROID_NDK_HOME" to android.ndkDirectory.toString(),
    )

    if (DefaultNativePlatform.host().operatingSystem.isWindows) {
        val binPath =
            android.ndkDirectory.toPath().resolve("toolchains/llvm/prebuilt/windows-x86_64/bin")
        val clangPath = binPath.resolve("clang.exe")
        val clangXXPath = binPath.resolve("clang++.exe")

        if (!clangPath.toFile().exists()) {
            throw GradleException("NDK clang compiler not found at expected location: $clangPath")
        }
        if (!clangXXPath.toFile().exists()) {
            throw GradleException("NDK clang++ compiler not found at expected location: $clangXXPath")
        }

        extraCargoEnv.putAll(
            mapOf(
                "CC" to clangPath.toString(),
                "CXX" to clangXXPath.toString(),
                // The default generator on Windows (Visual Studio) does not respect
                // the CC and CXX environment variables. Using Ninja ensures that
                // the specified compilers are used
                "CMAKE_GENERATOR" to "Ninja",
            )
        )
    }

    extraCargoBuildArguments = arrayListOf(
        "--lib",
        "--no-default-features",
        "--features",
        "sdl2/bundled"
    )
}

dependencies {
    implementation(fileTree("libs") {
        include("*.jar")
    })
}
