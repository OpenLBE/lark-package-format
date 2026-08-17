# Lark Package Format

**English** | [简体中文](README.md)

`.lark` stands for **Lightweight Android Resource Kit**.

It is a standardized package format for Android application delivery. It bundles an APK, an optional OBB, an optional XMPK, external resources, and copy rules for fast validation, distribution, and deployment.

## Design Goals

- **Lightweight**: `.lark` uses a ZIP container with every entry stored without compression, making it suitable for large APK, OBB, and resource files.
- **Standardized**: package names, versions, APKs, OBBs, XMPKs, and resource copy rules have explicit naming and validation requirements.
- **Deployable**: `main.json` describes how resources are copied from the package to an Android device.
- **Verifiable**: implementations can validate the package structure, rules JSON, APK package name, and resource coverage without extracting the archive.

## File Naming

A `.lark` filename must contain the APK package name and APK `versionName`:

```text
<packageName>.<versionName>.lark
```

Example:

```text
com.Company.ProductName.1.0.1.lark
```

The extracted directory uses only the APK package name:

```text
com.Company.ProductName\
```

## Package Structure

Example input directory:

```text
com.Company.ProductName\
  GameBuild.apk
  main.1.com.Company.ProductName.obb
  ProductName.xmpk
  copy.json
  Movies\
    demo.mp4
  PakCache\
    demo.asset
```

Example `.lark` root after packing:

```text
com.Company.ProductName.apk
main.1.com.Company.ProductName.obb
ProductName.xmpk
main.json
Movies/demo.mp4
PakCache/demo.asset
```

The input rules file may be named `copy.json`, `main.json`, `index.json`, `manifest.json`, or `<packageName>.json`. It is always stored as `main.json` in the `.lark` archive.

The input APK filename does not need to match the package name. It is always stored as `<packageName>.apk`.

## APK

- The package must contain exactly one `.apk` file.
- The package name is read from the APK Manifest.
- The input directory name must match the APK package name.
- `main.json.launchPackage` must match the APK package name.
- The `.lark` filename must be `<packageName>.<versionName>.lark`.
- Inside `.lark`, the APK must be at the archive root and named `<packageName>.apk`.

## OBB

- An `.obb` file is optional.
- If present, exactly one is allowed.
- It must be at the package root.
- Its filename must follow:

```text
main.<versionCode>.<packageName>.obb
```

Example:

```text
main.1.com.Company.ProductName.obb
```

## XMPK

- An `.xmpk` file is optional.
- If present, exactly one is allowed.
- It must be at the package root.
- Its filename must be the last product-name segment of the APK package name followed by `.xmpk`.
- The product name may contain only English letters and underscores.

Example:

```text
DemoLBE.xmpk
```

## Rules File: main.json

Exactly one rules JSON file must exist at the package root.

Allowed input filenames:

- `copy.json`
- `main.json`
- `index.json`
- `manifest.json`
- `<packageName>.json`

It is always stored as:

```text
main.json
```

Top-level fields:

| Field | Required | Description |
| --- | --- | --- |
| `launchPackage` | Yes | Package to launch; must match the APK package name |
| `waitSeconds` | No | Seconds to wait after launch; defaults to `0` |
| `description` | No | Optional description; may be null |
| `rules` | Yes | Copy-rule array; may be empty |

Copy-rule fields:

| Field | Required | Description |
| --- | --- | --- |
| `source` | Yes | Relative in-package path pattern; absolute paths and `..` are forbidden |
| `deviceDest` | Yes | Android device destination; must start with `/sdcard/` |

Example:

```json
{
  "launchPackage": "com.Company.ProductName",
  "waitSeconds": 15,
  "description": "Example package: copy Movies outside the app directory and PakCache into it.",
  "rules": [
    {
      "source": "Movies/**/*",
      "deviceDest": "/sdcard/.Dubnium/Movies/"
    },
    {
      "source": "PakCache/**/*",
      "deviceDest": "/sdcard/Android/data/com.Company.ProductName/files/PakCache/"
    }
  ]
}
```

If no wait is required, `waitSeconds` may be omitted:

```json
{
  "launchPackage": "com.Company.ProductName",
  "description": null,
  "rules": []
}
```

## Resource Copy Semantics

`.lark` uses `rules` to describe how resources are copied from the archive to an Android device.

| Type | Device path | Uninstall behavior |
| --- | --- | --- |
| External resource | Custom path under `/sdcard/...`, such as `/sdcard/.Dubnium/Movies/` | Not removed automatically when the app is uninstalled |
| App resource | `/sdcard/Android/data/<packageName>/...` | Removed with the app |

## Resource Coverage

Every regular file must be covered by at least one `rules[].source` pattern, except:

- `main.json`
- APK
- OBB
- Optional XMPK

For example, suppose the package contains:

```text
Movies/demo.mp4
PakCache/demo.asset
Readme.txt
```

but the only rule is:

```json
{
  "launchPackage": "com.Company.ProductName",
  "rules": [
    {
      "source": "Movies/**/*",
      "deviceDest": "/sdcard/.Dubnium/Movies/"
    }
  ]
}
```

Validation fails because `PakCache/demo.asset` and `Readme.txt` are not covered.

## Validation Requirements

A `.lark` implementation should validate at least the following:

- The input is an existing package directory or `.lark` file.
- The package directory name matches the APK package name.
- The `.lark` filename is `<packageName>.<versionName>.lark`.
- Exactly one APK exists.
- At most one OBB exists.
- At most one XMPK exists.
- Exactly one allowed rules JSON file exists.
- The rules file contains valid standard JSON.
- `launchPackage` matches the APK package name.
- `waitSeconds` defaults to `0` and must not be negative.
- Every copy rule contains `source` and `deviceDest`.
- `source` is relative and contains neither an absolute path nor `..`.
- `deviceDest` starts with `/sdcard/`.
- Every file except `main.json`, APK, OBB, and optional XMPK is covered by at least one `source`.
- Archive entries contain no absolute, rooted, or parent-traversal paths.
- Archive entries contain no duplicate file paths.

## JSON Notes

The rules file must use standard JSON:

- Property names use ASCII double quotes (`"`).
- Key/value separators use ASCII colons (`:`).
- Smart quotes (`“ ”`) and full-width colons (`：`) are invalid.

Invalid:

```json
{
  "launchPackage": "com.Company.ProductName",
  “ waitSeconds”：0,
  "rules": []
}
```

Valid:

```json
{
  "launchPackage": "com.Company.ProductName",
  "waitSeconds": 0,
  "rules": []
}
```

## Rust Reference Implementation

This repository provides `lark-pack-tool`, compatible with `Dubnium.LarkPackTool`, with support for:

- Packing a directory into `.lark`
- Extracting `.lark` into a directory
- Validation without extraction via `--check`
- Store ZIP and Zip64 output
- Rules filename normalization to `main.json`
- APK filename normalization to `<packageName>.apk`
- Timestamped backups and failure recovery for existing outputs

### Build

```powershell
cargo build --release
```

Output:

```text
target\release\lark-pack-tool.exe
```

### Usage

```powershell
# Pack a directory into .lark
.\target\release\lark-pack-tool.exe C:\home\apk\com.Company.ProductName

# Extract a .lark package
.\target\release\lark-pack-tool.exe C:\home\apk\com.Company.ProductName.1.0.1.lark

# Validate without creating or extracting files
.\target\release\lark-pack-tool.exe --check C:\home\apk\com.Company.ProductName

# Skip rules coverage validation for extra files
.\target\release\lark-pack-tool.exe --ignore-uncovered --check C:\home\apk\com.Company.ProductName
```

### APK Reading

The Rust standard library does not include an APK or Android Binary XML (AXML) reader. This implementation uses the pure-Rust, Apache-2.0-licensed `apk-info-axml` crate to read `AndroidManifest.xml` and optional `resources.arsc` entries from the APK ZIP and obtain `package` and `versionName`.

Outer ZIP I/O uses the `zip` crate. Every `.lark` entry is written with the Store method, with Zip64 enabled automatically when required.

Prebuilt binaries are available from the repository [Releases](https://github.com/OpenLBE/lark-package-format/releases) page.
