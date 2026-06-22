# 文档解析 Sidecar

这是个人知识库桌面应用的本地文档解析 sidecar。Rust 负责文件路径校验、权限边界和数据库写入，sidecar 只接收 Rust 传入的单个本地文件路径，返回结构化 JSON。

## 本地协议

stdin 输入：

```json
{
  "filePath": "E:\\Knowledge\\Redis.md",
  "relativePath": "Redis.md",
  "maxInputBytes": 52428800
}
```

stdout 成功输出：

```json
{
  "ok": true,
  "result": {
    "title": "Redis.md",
    "body": "正文文本",
    "summary": "摘要文本",
    "sourceLocator": "Redis.md",
    "tableInsights": []
  }
}
```

stdout 失败输出：

```json
{
  "ok": false,
  "error": {
    "code": "PARSER_UNSUPPORTED_FILE",
    "message": "当前文档解析仅支持 PDF、DOCX、XLSX、Markdown 和 TXT 文件"
  }
}
```

## 本地开发

```powershell
Set-Location .\sidecars\parser
..\..\.venv\Scripts\python.exe -m pip install -r .\requirements-dev.txt
..\..\.venv\Scripts\python.exe -m pytest
```

## 边界

- 只解析本地文件，不访问网络
- 不下载模型，不读取环境密钥
- 不写入数据库，数据库写入只发生在 Rust 核心
- 文件路径必须先由 Rust 校验在当前知识库目录内
- 单个输入文件默认限制为 50 MB
- DOCX/XLSX 内部 XML 单个 entry 默认限制为 10 MB，累计解压 XML 默认限制为 30 MB
