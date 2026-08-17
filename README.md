# Lark Package Format

[English](README.en.md) | **简体中文**

`.lark` 是 **Lightweight Android Resource Kit** 的缩写，即“轻量级 Android 资源包”。

它是一种面向 Android 应用交付的标准化资源包格式，用于把 APK、OBB、可选 XMPK、外部资源和复制规则统一打包，便于快速校验、分发和部署。

## 设计目标

- **轻量**：`.lark` 使用 ZIP 容器，文件条目以 Store 方式直接存储，不压缩，适合大体积 APK、OBB 和资源文件。
- **标准化**：包名、版本号、APK、OBB、XMPK 和资源复制规则都有明确命名和校验规则。
- **可部署**：通过 `main.json` 描述资源从包内路径复制到 Android 设备路径的规则。
- **可校验**：不需要真实解压即可检查 `.lark` 包结构、规则 JSON、APK 包名和资源覆盖关系。

## 文件命名

`.lark` 文件名必须包含 APK 包名和 APK `versionName`：

```text
<packageName>.<versionName>.lark
```

示例：

```text
com.Company.ProductName.1.0.1.lark
```

解包后的目录名只使用 APK 包名：

```text
com.Company.ProductName\
```

## 包结构

提交目录示例：

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

打包后的 `.lark` 根目录示例：

```text
com.Company.ProductName.apk
main.1.com.Company.ProductName.obb
ProductName.xmpk
main.json
Movies/demo.mp4
PakCache/demo.asset
```

目录中的规则文件可以命名为 `copy.json`、`main.json`、`index.json`、`manifest.json` 或 `<packageName>.json`；写入 `.lark` 后统一改名为 `main.json`。

目录中的 APK 文件名可以不是包名；写入 `.lark` 后统一改名为 `<packageName>.apk`。

## APK

- 必须有且仅有一个 `.apk` 文件。
- APK 包名以 Manifest 中读取到的 package name 为准。
- 提交目录名必须与 APK 包名一致。
- `main.json.launchPackage` 必须与 APK 包名一致。
- `.lark` 文件名必须为 `<packageName>.<versionName>.lark`。
- `.lark` 内的 APK 必须位于根目录，并命名为 `<packageName>.apk`。

## OBB

- `.obb` 可以不存在。
- 如果存在，必须有且仅有一个。
- OBB 必须位于包根目录。
- OBB 文件名必须符合：

```text
main.<versionCode>.<packageName>.obb
```

示例：

```text
main.1.com.Company.ProductName.obb
```

## XMPK

- `.xmpk` 可以不存在。
- 如果存在，必须有且仅有一个。
- XMPK 必须位于包根目录。
- XMPK 文件名必须为 APK 包名最后一段产品名加 `.xmpk`。
- 产品名只允许英文字母和下划线。

示例：

```text
DemoLBE.xmpk
```

## 规则文件 main.json

规则 JSON 必须位于包根目录，且只能存在一个。

允许的输入文件名：

- `copy.json`
- `main.json`
- `index.json`
- `manifest.json`
- `<packageName>.json`

写入 `.lark` 后统一为：

```text
main.json
```

字段说明：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `launchPackage` | 是 | 启动包名，必须与 APK 包名一致 |
| `waitSeconds` | 否 | 启动后等待秒数，默认值 `0` |
| `description` | 否 | 说明文字，可为空 |
| `rules` | 是 | 复制规则数组，可以为空数组 |

复制规则字段：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `source` | 是 | 包内相对路径匹配规则，不允许绝对路径或 `..` |
| `deviceDest` | 是 | Android 设备目标路径，必须以 `/sdcard/` 开头 |

示例：

```json
{
  "launchPackage": "com.Company.ProductName",
  "waitSeconds": 15,
  "description": "示例包：视频复制到包外目录，PakCache 复制到包内数据目录。",
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

如果不需要等待，可以省略 `waitSeconds`：

```json
{
  "launchPackage": "com.Company.ProductName",
  "description": null,
  "rules": []
}
```

## 资源复制语义

`.lark` 用 `rules` 描述资源从包内路径复制到 Android 设备路径。

| 类型 | 设备路径 | 卸载行为 |
| --- | --- | --- |
| 包外资源 | `/sdcard/...` 下的自定义目录，例如 `/sdcard/.Dubnium/Movies/` | 程序卸载时不会自动删除 |
| 包内资源 | `/sdcard/Android/data/<packageName>/...` | 程序卸载时会一起删除 |

## 资源覆盖规则

除以下文件外，包内所有普通文件都必须被 `rules` 中至少一条 `source` 覆盖：

- `main.json`
- APK
- OBB
- 可选 XMPK

例如目录中存在：

```text
Movies/demo.mp4
PakCache/demo.asset
Readme.txt
```

但规则只有：

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

则检查失败，因为 `PakCache/demo.asset` 和 `Readme.txt` 没有出现在复制规则里。

## 校验项

`.lark` 格式实现应至少校验以下项目：

- 输入必须是存在的包目录或 `.lark` 文件。
- 包目录名必须与 APK 包名一致。
- `.lark` 文件名必须为 `<packageName>.<versionName>.lark`。
- APK 必须有且仅有一个。
- OBB 可以没有；如果有，必须有且仅有一个。
- XMPK 可以没有；如果有，必须有且仅有一个。
- 规则 JSON 必须存在，且只能存在一个允许文件名。
- 规则 JSON 必须是合法标准 JSON。
- `launchPackage` 必须与 APK 包名一致。
- `waitSeconds` 可选，默认值为 `0`；如果填写，不能为负数。
- 每条复制规则必须包含 `source` 和 `deviceDest`。
- `source` 必须是相对路径匹配规则，不能是绝对路径，不能包含 `..`。
- `deviceDest` 必须以 `/sdcard/` 开头。
- 除 `main.json`、APK、OBB 和可选 XMPK 外，其他文件必须被至少一条 `source` 覆盖。
- `.lark` 中不能包含绝对路径、根路径或 `..` 路径穿越条目。
- `.lark` 中不能包含重复文件条目。

## JSON 注意事项

规则 JSON 必须是标准 JSON：

- 属性名必须使用英文双引号 `"`。
- 键值分隔必须使用英文冒号 `:`。
- 不能使用中文弯引号 `“ ”` 或中文冒号 `：`。

错误示例：

```json
{
  "launchPackage": "com.Company.ProductName",
  “ waitSeconds”：0,
  "rules": []
}
```

正确示例：

```json
{
  "launchPackage": "com.Company.ProductName",
  "waitSeconds": 0,
  "rules": []
}
```

## Rust 参考实现

本仓库提供 `lark-pack-tool`，其行为与 `Dubnium.LarkPackTool` 对齐，支持：

- 目录打包为 `.lark`
- `.lark` 解包为目录
- `--check` 非解压校验
- Store ZIP / Zip64 输出
- 规则 JSON 归一化为 `main.json`
- APK 文件名归一化为 `<packageName>.apk`
- 已存在输出的时间戳备份与失败恢复

### 构建

```powershell
cargo build --release
```

产物位于：

```text
target\release\lark-pack-tool.exe
```

### 使用

```powershell
# 目录打包为 .lark
.\target\release\lark-pack-tool.exe C:\home\apk\com.Company.ProductName

# 解包 .lark
.\target\release\lark-pack-tool.exe C:\home\apk\com.Company.ProductName.1.0.1.lark

# 仅校验，不生成或解压文件
.\target\release\lark-pack-tool.exe --check C:\home\apk\com.Company.ProductName

# 跳过额外资源的 rules 覆盖检查
.\target\release\lark-pack-tool.exe --ignore-uncovered --check C:\home\apk\com.Company.ProductName
```

### APK 读取

Rust 标准库没有 APK 或 Android Binary XML（AXML）读取器。实现使用 Apache-2.0
许可的纯 Rust crate `apk-info-axml`，从 APK ZIP 中读取并解析
`AndroidManifest.xml` 和可选的 `resources.arsc`，获取 `package` 与
`versionName`。外层 ZIP 读写使用 `zip` crate；`.lark` 写入时所有条目均为
Store，并在需要时自动使用 Zip64。

预编译版本可在本仓库的 [Releases](https://github.com/OpenLBE/lark-package-format/releases) 页面下载。
