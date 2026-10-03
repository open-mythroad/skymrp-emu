# skymrp：Rust 编写的高性能 MRP 模拟器

**skymrp** 是一个用 Rust 编写的 Mythroad/MRP 高层模拟器（HLE），可在现代桌面系统上运行。

skymrp 并不模拟完整的早期功能机平台，而是实现 MRP 应用运行时需要的 Mythroad 平台接口。
应用中的 ARM 原生代码会交给
[Dynarmic](https://github.com/open-mythroad/dynarmic) 模拟执行。

这个项目的目标，是让经典 MRP 应用和游戏能在现代系统上重新运行：

- 当前：主要面向用 C 编写、核心逻辑编译为 ARM 原生代码，并调用常见 Mythroad 接口的 MRP 应用和游戏。
- 长期：提升真实 MRP 软件的兼容性，逐步补齐更多平台接口。
- 计划中，但尚未实现：完整 `mr` 虚拟机模式，用于运行主逻辑由 Mythroad
  类 Lua 运行时的 `start.mr` 字节码实现的 MRP 应用和游戏。

## 重要免责声明

本项目与杭州斯凯、原始 Mythroad/MRP 平台、手机厂商、运营商或任何应用厂商均无从属、
授权、认可或背书关系。Mythroad、MRP、杭州斯凯以及相关名称、商标、软件、文档和其他知识产权，
均归杭州斯凯和/或各自权利方所有。

使用 skymrp 运行 MRP 应用前，请确保相关软件来源合法。

## 开发状态

本项目目前主要由 arctan95 作为个人项目维护，仍处于早期开发阶段，暂不承诺应用兼容性。
当前开发重点也是这类 C 版 MRP 软件。许多相关应用和游戏已经可以运行，
但遇到兼容性问题仍属正常情况。

整体上，功能支持范围会随着实际测试的软件逐步推进：真实应用需要哪些尚未实现的行为，
对应接口就会逐步补上。因此，文件系统、图形、文字渲染、音频、定时器、输入、libc、
网络等模块的完成度并不完全一致。

## 使用方式

首先获取 skymrp。你可以从
[发布页面](https://github.com/open-mythroad/skymrp-emu/releases) 下载二进制文件，也可以按照下一节从源码构建。

然后准备一个可运行的 MRP 应用或游戏，并在命令行中传入对应的 `.mrp` 文件：

```sh
skymrp path/to/app.mrp
```

如果要直接从源码目录运行：

```sh
cargo run -- path/to/app.mrp
```

可以通过 `--help` 查看命令行参数：

```sh
skymrp --help
```

## 构建

请参阅 [BUILD.md](./BUILD.md) 了解如何构建项目。

## 贡献

本项目仍处于早期开发阶段。欢迎提交能改善 MRP 应用兼容性、平台接口覆盖、文档和构建可移植性的贡献。

如果改动较大，请先讨论方向。请保持 commit 小而聚焦，避免把无关改动混在一起。
提交前请运行格式化和静态检查：

```sh
cargo fmt
cargo clippy
```

请注意版权和逆向工程边界。不要提交从专有源码或泄露源码中复制的代码。
记录或实现平台行为时，请优先参考合法可得的文档、clean-room 测试结果，以及许可证兼容的开源项目。

## 许可证

skymrp © 2026 arctan95 and other contributors.

skymrp 本身的源代码基于 Mozilla Public License version 2.0 授权。

出于许可证兼容性考虑，二进制文件基于 GNU General Public License version 3 or later 授权。

## 致谢

本项目离不开许多项目和社区的积累。感谢：

- [mrpoid2018](https://github.com/Yichou/mrpoid2018) 项目，它是研究 Mythroad
  行为和 MRP 兼容性的重要参考。
- [touchHLE](https://github.com/touchHLE/touchHLE) 项目，它的 HLE 设计和文档风格给了本项目很多参考。
- [Dynarmic](https://github.com/lioncash/dynarmic)、
  [Sonivox](https://github.com/EmbeddedSynth/sonivox) 和
  [SDL](https://www.libsdl.org/) 的作者与贡献者。
- 所有整理和分享 Mythroad、MRP 文件格式、旧手机平台特性以及早期移动应用平台资料的人。
