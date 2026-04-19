// Copyright 2024 mazao
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! 错误类型定义
//!
//! 提供统一的错误类型 [`MockproxyrsError`] 和结果类型别名 [`Result`]。

use thiserror::Error;

/// Mockproxyrs 统一错误类型
///
/// 涵盖所有可能的错误场景，包括 IO、数据库、网络、配置等。
#[derive(Debug, Error)]
pub enum MockproxyrsError {
    /// IO 错误（文件读写、网络连接等）
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// 数据库错误（SQLite 操作失败）
    #[error("Database error: {0}")]
    Database(String),

    /// 配置错误（配置解析或验证失败）
    #[error("Config error: {0}")]
    Config(String),

    /// 代理错误（请求转发失败）
    #[error("Proxy error: {0}")]
    Proxy(String),

    /// 解析错误（URL、JSON 等解析失败）
    #[error("Parse error: {0}")]
    Parse(String),

    /// 资源未找到
    #[error("Not found: {0}")]
    NotFound(String),

    /// 资源已存在
    #[error("Already exists: {0}")]
    AlreadyExists(String),

    /// 服务未运行
    #[error("Service not running: {0}")]
    ServiceNotRunning(String),

    /// 服务已在运行
    #[error("Service already running: {0}")]
    ServiceAlreadyRunning(String),

    /// 无效地址
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    /// HTTP 错误
    #[error("HTTP error: {0}")]
    Http(String),

    /// TLS/SSL 错误
    #[error("TLS error: {0}")]
    Tls(String),

    /// 通道错误（事件发送失败）
    #[error("Channel error: {0}")]
    Channel(String),

    /// 未知错误
    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// Mockproxyrs 结果类型别名
pub type Result<T> = std::result::Result<T, MockproxyrsError>;
