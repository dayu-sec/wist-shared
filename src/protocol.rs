//! 跨进程**错误投影**（设计 `foundation/error-handling-system.md` §6）。
//!
//! 内部 `StructError<R>` 不直接序列化到协议；对外一律走这层稳定投影
//! `{ "error": { code, message, …可选 } }`。`code` 是稳定字符串码（由 reason identity
//! 映射而来，§6.3），**不是** Rust enum / Debug 文本。
//!
//! 安全约束（§6.1 / §9）：`detail` / source error / backtrace **不**进这层；完整因果链只进本地日志。
//! 可选字段按需增长即可 —— 简单错误仍只出 `code` + `message`（`skip_serializing_if`）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 严重度（§6.1）。不设则不出现在正文里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Fatal,
}

/// 稳定错误投影（§6.1 的最小字段集）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolError {
    /// 稳定字符串码，供机器分支（前端 / gwlinkd / agentd 按它决定处置）。
    pub code: String,
    /// 可暴露短文本，不含路径 / token / 命令原文 / 完整 stderr。
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, String>>,
}

impl ProtocolError {
    /// 只带必填字段（`code` + `message`）。
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: None,
            severity: None,
            correlation_id: None,
            fields: None,
        }
    }
}

/// 对外错误信封：错误一律嵌在 `error` 下，与成功载荷分名字空间。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolErrorEnvelope {
    pub error: ProtocolError,
}

impl ProtocolErrorEnvelope {
    pub fn new(error: ProtocolError) -> Self {
        Self { error }
    }
}
