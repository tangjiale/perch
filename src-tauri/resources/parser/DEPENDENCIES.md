# 文档解析运行时

- Apache Tika App 3.2.3，Apache License 2.0。
  来源：https://repo.maven.apache.org/maven2/org/apache/tika/tika-app/3.2.3/tika-app-3.2.3.jar
  SHA-256：`80c20c085e2c0976bbd55969e5bf90dda2b7155db31068639fbc871d0369e7e7`。
  2026-09-08 从 Maven Central 下载核对，并与上游 SHA-1
  `4849b7f696f7f47691ea4c7a81b985e8f8fb4338` 交叉检查一致。
  JAR 内含 `META-INF/LICENSE`、`META-INF/NOTICE` 及依赖许可。
- Eclipse Temurin JRE 21.0.12.1+1，macOS aarch64 / Windows x64。
  来源：https://github.com/adoptium/temurin21-binaries/releases/tag/jdk-21.0.12.1%2B1
  macOS archive SHA-256：`dec50fc6f9fcd4fe3ae8cabf5a5fa68f6afc48841f7698e468e9aa5d54beed84`。
  Windows archive SHA-256：`d35f31e712f0fcf6ac5a093edc90204fbff22f720ba3950bd09d331d5e621636`。
  两者于 2026-09-08 核对 GitHub 官方 Release asset digest。
  许可和第三方声明位于 macOS `jre/Contents/Home/legal`、Windows `jre/legal`，
  保留上游目录及链接，一并随应用分发。

直接启动随包 Java，不需要用户安装 Java 或 Office。Java 最大堆 512 MB，
解析并发 1，单文件 60 秒超时，正文输出最多 16 MB；禁用外部解析器及 OCR。
扫描 PDF 没有正文时显示 needs_ocr，不能将其当成已索引的文档。

运行 `node scripts/prepare-parser.mjs` 按当前构建主机准备运行时，下载地址、
版本和 SHA-256 固定在 `scripts/parser-runtime.json`。脚本校验下载包后解压，
支持 macOS ARM64 和 Windows x64；不支持的平台直接失败。已有 Tika 文件会
重新校验 SHA-256，已有 JRE 校验 release 版本、平台、架构和 Java/许可目录；发布工作流在每个平台下载前清理 CI 工作目录中其他平台的生成运行时，
不匹配时拒绝覆盖；需要更换时在全新检出目录构建。临时下载保留在系统临时目录。

`node scripts/prepare-parser.mjs --check` 仅验证已准备资源。`jre` 和 `tika-app.jar`
不提交 Git，由 CI 在各自平台准备并执行真实 PPTX/XLSX/DOCX/PDF 解析测试。
Windows 直接启动 `jre/bin/java.exe` 并隐藏控制台；macOS 使用
`jre/Contents/Home/bin/java`。Intel Mac 尚未支持，不能发布相应安装包。
