# Building skymrp

## Prerequisites

### General

You need:

- [Git](https://git-scm.com/)
- [Rust](https://www.rust-lang.org/tools/install)
- [CMake](https://cmake.org/)
- [Boost](https://www.boost.org/users/download/)
- Your platform's standard C and C++ compilers

First clone the repository:

```bash
git clone --recursive https://github.com/open-mythroad/skymrp-emu.git
```

If you forgot to clone submodules:

```bash
cd skymrp-emu
git submodule update --init --recursive
```

Then install the platform-specific dependencies below.

### Linux

```bash
# Debian/Ubuntu
sudo apt-get update
sudo apt-get install -y build-essential cmake libboost-all-dev
```

### macOS

```bash
brew install boost
```

### Windows

```bash
# bash
curl -L -o boost_1_81_0.7z https://archives.boost.io/release/1.81.0/source/boost_1_81_0.7z
7z -ovendor x boost_1_81_0.7z
mv vendor/boost_1_81_0 vendor/boost
```

### Android

All the general prerequisites apply for Android, and you need three additional things:

1. Rust toolchain: `rustup target add aarch64-linux-android`
2. cargo-ndk: `cargo install cargo-ndk`
   - Important: make sure to use version 3.4.0 or later (can be checked with `cargo ndk -v`)
3. The Android SDK and NDK. There's two options:
    - Install Android Studio (recommended): https://developer.android.com/
    - Install "Command line tools only": https://developer.android.com/studio/index.html#command-line-tools-only
      - You might also need to install Gradle (check the version in `android/gradle/wrapper/gradle-wrapper.properties`)
4. Boost: Download and extract Boost into `vendor/boost` by following the Windows instructions above.

## Building

### Non-Android platforms


```bash
cargo build --release
```

### Android

#### With Android Studio

Open/import `android` project folder, click on build, then run

#### With Gradle on the command line

```bash
export ANDROID_NDK_HOME="path/to/ndk"
export ANDROID_SDK_HOME="path/to/sdk"

cd android
gradle assembleRelease
```
