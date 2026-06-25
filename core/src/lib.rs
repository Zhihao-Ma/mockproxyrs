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

//! Mockproxyrs 核心库
//!
//! 提供平台无关的反向代理和 Mock 功能。

pub mod domain;
pub mod error;
pub mod event;
pub mod mock;
pub mod proxy;
pub mod repository;

// 重导出常用类型
pub use domain::{MockRule, MockService, ResponseEvent, ServiceStatus};
pub use error::{MockproxyrsError, Result};
pub use event::EventEmitter;
pub use mock::{execute_script, RequestContext, ScriptEngine, ScriptMockResponse};
pub use repository::MockRepository;
