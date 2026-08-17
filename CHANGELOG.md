# Changelog

All notable changes to this project are documented in this file.

## [1.0.1] - 2026-08-17

### Fixed

- Allow underscores in XMPK product names, including names such as
  `WZRY_VR`.
- Update the Chinese and English format documentation to reflect the
  expanded XMPK naming rule.

## [1.0.0] - 2026-08-15

### Added

- Add the Rust reference implementation of the Lightweight Android Resource
  Kit package format.
- Support packing, checking, and unpacking `.lark` packages.
- Read APK package names and version names from binary Android manifests.
- Add bidirectional compatibility coverage for the C# implementation.
- Publish a Windows x64 executable with an accompanying SHA-256 checksum.

## [0.1.0] - 2026-08-08

### Added

- Publish the initial Windows x64 Native AOT build of
  `Dubnium.LarkPackTool`.

[1.0.1]: https://github.com/OpenLBE/lark-package-format/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/OpenLBE/lark-package-format/compare/v0.1.0...v1.0.0
[0.1.0]: https://github.com/OpenLBE/lark-package-format/releases/tag/v0.1.0
