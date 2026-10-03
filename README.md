# skymrp: high-level emulator for MRP apps

**skymrp** is a high-level emulator (HLE) for Mythroad/MRP apps. It runs on
modern desktop operating systems, and is written in Rust.

This emulator does not emulate a complete Mythroad phone runtime. Instead, it
takes the place of the platform and provides its own implementations of the APIs
that MRP apps call into. The only code the
[emulated CPU](https://github.com/open-mythroad/dynarmic) executes is the native
ARM code from MRP apps.

The goal of this project is to run classic MRP apps and games on modern
systems:

- Currently: C/ARM-based MRP packages that use commonly implemented Mythroad
  APIs.
- Longer term: broader compatibility with real-world MRP software and more
  complete platform API coverage.
- Planned, but not implemented yet: full `mr` virtual machine mode for packages
  whose main logic is implemented as `start.mr` bytecode for the Mythroad
  Lua-like runtime.

## Important Disclaimer

This project is not affiliated with or endorsed by Sky-mobi, any original
Mythroad, MRP, handset, carrier, or application vendor in any way. Mythroad,
MRP, Sky-mobi, and related names, trademarks, software, documentation, and
other intellectual property are the property of Sky-mobi and/or their
respective owners.

Only use skymrp with software you have obtained legally.

## Development Status

This project is currently mostly developed by arctan95 as a personal project.
It is still early-stage software, and no promises can be made about app
compatibility. Development currently focuses on MRP apps whose executable logic
is implemented in native C/ARM code. Many C-based MRP apps can run, but
app-specific issues should be expected.

In general, supported functionality is defined by the apps being tested: API
coverage grows as missing behaviour is needed for real software. Consequently,
completeness varies between areas such as filesystem access, graphics, text
rendering, audio, timers, input, libc, and networking.

## Usage

First obtain skymrp, either a
[binary release](https://github.com/open-mythroad/skymrp-emu/releases) or by building
it yourself (see the next section).

You'll then need an MRP app that you can run. Pass the MRP file on the command
line:

```sh
skymrp path/to/app.mrp
```

When running from the source tree:

```sh
cargo run -- path/to/app.mrp
```

You can see the command-line usage by passing the `--help` flag:

```sh
skymrp --help
```

## Building

See [BUILD.md](./BUILD.md) for build instructions.

## Contributing

This project is still in early development. Contributions that improve MRP app
compatibility, platform API coverage, documentation, and build portability are
welcome.

For non-trivial changes, please discuss the direction first. Keep commits small
and focused, avoid bundling unrelated changes together, and run the formatting
and lint checks before submitting changes:

```sh
cargo fmt
cargo clippy
```

Please be careful about copyright and reverse engineering. Do not contribute
code copied from proprietary or leaked sources. When documenting or implementing
platform behaviour, prefer legally available documentation, clean-room testing,
and compatible open-source references.

## License

skymrp © 2026 arctan95 and other contributors.

The source code of skymrp itself is licensed under the Mozilla Public License,
version 2.0.

Due to license compatibility concerns, binaries are under the GNU General
Public License version 3 or later.

## Thanks

This project builds on work from many projects and communities. Thank you to:

- The [mrpoid2018](https://github.com/Yichou/mrpoid2018) project, which is an
  important reference for Mythroad behaviour and MRP compatibility research.
- The [touchHLE](https://github.com/touchHLE/touchHLE) project, whose HLE design
  and documentation style are a major reference point for this project.
- The authors and contributors of
  [Dynarmic](https://github.com/lioncash/dynarmic),
  [Sonivox](https://github.com/EmbeddedSynth/sonivox), and
  [SDL](https://www.libsdl.org/).
- Everyone who documented Mythroad, MRP file formats, handset behaviour, and old
  mobile app platforms.
