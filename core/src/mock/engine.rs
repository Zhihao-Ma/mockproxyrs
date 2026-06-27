use crate::error::{MockproxyrsError, Result};
use rquickjs::{Context, Runtime};
use std::time::{Duration, Instant};

const SCRIPT_MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const SCRIPT_STACK_LIMIT_BYTES: usize = 1024 * 1024;
pub const SCRIPT_TIMEOUT: Duration = Duration::from_millis(1000);

/// JS 脚本引擎。
///
/// 封装 rquickjs Runtime，提供进程级单例、资源限制和语法校验。
#[derive(Clone)]
pub struct ScriptEngine {
    runtime: Runtime,
}

impl ScriptEngine {
    /// 创建新引擎，设置内存和栈限制。
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new().map_err(to_script_error)?;
        runtime.set_memory_limit(SCRIPT_MEMORY_LIMIT_BYTES);
        runtime.set_max_stack_size(SCRIPT_STACK_LIMIT_BYTES);
        Ok(Self { runtime })
    }

    /// 校验脚本语法（不执行用户脚本体）。
    ///
    /// 将脚本包在 `new Function(...)` 中解析，只检查语法，不执行逻辑。
    pub fn validate(&self, script: &str) -> Result<()> {
        let source = format!("new Function({})", json_string_literal(script));
        self.with_limited_context(|ctx| {
            ctx.eval::<(), _>(source)
                .map_err(|e| to_detailed_script_error(&ctx, e))?;
            Ok(())
        })
    }

    /// 创建带超时限制的 JS 上下文，并在此上下文中执行闭包。
    ///
    /// 每次调用都创建全新的 Context，保证请求间状态隔离。
    pub(crate) fn with_limited_context<F, T>(&self, f: F) -> Result<T>
    where
        F: for<'js> FnOnce(rquickjs::Ctx<'js>) -> Result<T>,
    {
        let started_at = Instant::now();
        self.runtime.set_interrupt_handler(Some(Box::new(move || {
            started_at.elapsed() >= SCRIPT_TIMEOUT
        })));

        let result = (|| {
            let context = Context::full(&self.runtime).map_err(to_script_error)?;
            context.with(f)
        })();

        self.runtime.set_interrupt_handler(None);
        result
    }
}

fn json_string_literal(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string literal cannot fail")
}

/// 将 rquickjs 错误转换为统一的 MockproxyrsError::Script。
pub(crate) fn to_script_error(error: rquickjs::Error) -> MockproxyrsError {
    MockproxyrsError::Script(error.to_string())
}

/// 捕获 JS 异常的详细信息（message + stack），方便调试。
/// 自动修正 IIFE 包装导致的 +1 行号偏移。
pub(crate) fn to_detailed_script_error(
    ctx: &rquickjs::Ctx<'_>,
    error: rquickjs::Error,
) -> MockproxyrsError {
    let caught = ctx.catch();
    if let Some(obj) = caught.as_object() {
        let message: Option<String> = obj.get("message").ok();
        let stack: Option<String> = obj.get("stack").ok();
        if let Some(stack) = stack {
            return MockproxyrsError::Script(adjust_line_numbers(&stack));
        }
        if let Some(msg) = message {
            return MockproxyrsError::Script(adjust_line_numbers(&msg));
        }
    }
    MockproxyrsError::Script(adjust_line_numbers(&error.to_string()))
}

/// 修正 `<input>:N:C` 中的行号 N → N-1（抵消 IIFE 包装的一行偏移）。
fn adjust_line_numbers(text: &str) -> String {
    let re = regex::Regex::new(r"<input>:(\d+):(\d+)").expect("static regex");
    re.replace_all(text, |caps: &regex::Captures| {
        let line: usize = caps[1].parse().unwrap_or(1);
        let col: usize = caps[2].parse().unwrap_or(1);
        format!("<input>:{}:{}", line.saturating_sub(3), col)
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_accepts_valid_script() {
        let engine = ScriptEngine::new().unwrap();
        let result = engine.validate("return { code: 0 };");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_rejects_syntax_error() {
        let engine = ScriptEngine::new().unwrap();
        let result = engine.validate("return { code: ;");
        assert!(result.is_err());
        assert!(!result.unwrap_err().to_string().is_empty());
    }
}
