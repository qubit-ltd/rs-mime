# Qubit MIME

Rust 服务使用的 MIME 类型检测工具。

本 crate 通过文件名 glob 规则和内容魔数规则检测 MIME 类型，数据模型与
Java `common-mime` 模块使用的 freedesktop 风格 MIME 数据库保持一致。
