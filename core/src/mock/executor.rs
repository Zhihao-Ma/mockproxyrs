use crate::error::{MockproxyrsError, Result};
use crate::mock::context::RequestContext;
use crate::mock::engine::{to_detailed_script_error, to_script_error, ScriptEngine};
use rquickjs::{Function, Object, Value, prelude::Rest};
use std::collections::HashMap;

const DEFAULT_CONTENT_TYPE: &str = "application/json;charset=UTF-8";

/// 脚本执行产生的 Mock 响应。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptMockResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

/// 异步执行用户脚本，生成 Mock 响应。
///
/// 内部通过 `spawn_blocking` 在阻塞线程中执行 JS，避免阻塞 tokio reactor。
pub async fn execute_script(
    engine: ScriptEngine,
    request: RequestContext,
    script: String,
) -> Result<ScriptMockResponse> {
    tokio::task::spawn_blocking(move || execute_script_blocking(&engine, request, &script))
        .await
        .map_err(|e| MockproxyrsError::Script(format!("script task join error: {}", e)))?
}

pub(crate) fn execute_script_blocking(
    engine: &ScriptEngine,
    request: RequestContext,
    script: &str,
) -> Result<ScriptMockResponse> {
    let source = format!("(function(){{\n{}\n}})()", script);
    engine.with_limited_context(|ctx| {
        inject_request(ctx.clone(), &request)?;
        inject_console(ctx.clone())?;
        let value = ctx
            .eval(source)
            .map_err(|e| to_detailed_script_error(&ctx, e))?;
        normalize_response(ctx, value)
    })
}

fn inject_request<'js>(ctx: rquickjs::Ctx<'js>, request: &RequestContext) -> Result<()> {
    let request_obj = Object::new(ctx.clone()).map_err(to_script_error)?;
    request_obj
        .set("method", request.method.clone())
        .map_err(to_script_error)?;
    request_obj
        .set("url", request.url.clone())
        .map_err(to_script_error)?;
    request_obj
        .set("path", request.path.clone())
        .map_err(to_script_error)?;
    request_obj
        .set("body", request.body.clone())
        .map_err(to_script_error)?;

    let headers = Object::new(ctx.clone()).map_err(to_script_error)?;
    for (key, value) in &request.headers {
        headers
            .set(key.as_str(), value.clone())
            .map_err(to_script_error)?;
    }
    request_obj
        .set("headers", headers)
        .map_err(to_script_error)?;

    let query = Object::new(ctx.clone()).map_err(to_script_error)?;
    for (key, value) in &request.query {
        query
            .set(key.as_str(), value.clone())
            .map_err(to_script_error)?;
    }
    request_obj.set("query", query).map_err(to_script_error)?;

    ctx.globals()
        .set("request", request_obj)
        .map_err(to_script_error)?;
    Ok(())
}

fn inject_console<'js>(ctx: rquickjs::Ctx<'js>) -> Result<()> {
    let console = Object::new(ctx.clone()).map_err(to_script_error)?;

    let log = Function::new(ctx.clone(), |args: Rest<Value>| {
        log::info!("[script] {}", format_console_args(args.0));
        Ok::<_, rquickjs::Error>(())
    })
    .map_err(to_script_error)?
    .with_name("log")
    .map_err(to_script_error)?;

    let error = Function::new(ctx.clone(), |args: Rest<Value>| {
        log::error!("[script] {}", format_console_args(args.0));
        Ok::<_, rquickjs::Error>(())
    })
    .map_err(to_script_error)?
    .with_name("error")
    .map_err(to_script_error)?;

    console.set("log", log).map_err(to_script_error)?;
    console.set("error", error).map_err(to_script_error)?;
    ctx.globals()
        .set("console", console)
        .map_err(to_script_error)?;
    Ok(())
}

fn format_console_args(args: Vec<Value<'_>>) -> String {
    args.into_iter()
        .map(|value| {
            if let Some(s) = value.as_string() {
                s.to_string()
                    .unwrap_or_else(|_| "[invalid string]".to_string())
            } else if value.is_null() {
                "null".to_string()
            } else if value.is_undefined() {
                "undefined".to_string()
            } else if let Some(n) = value.as_number() {
                n.to_string()
            } else if let Some(obj) = value.as_object() {
                // Use Debug format as a fallback for objects
                format!("{:?}", obj)
            } else {
                format!("{:?}", value)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_response<'js>(
    ctx: rquickjs::Ctx<'js>,
    value: Value<'js>,
) -> Result<ScriptMockResponse> {
    if value.is_undefined() {
        return Err(MockproxyrsError::Script(
            "script returned undefined; use `return` to produce a response".to_string(),
        ));
    }

    if let Some(object) = value.as_object()
        && object.contains_key("status").map_err(to_script_error)?
    {
        return parse_full_response(object);
    }

    Ok(ScriptMockResponse {
        status: 200,
        headers: default_headers(),
        body: stringify_value(ctx, value)?,
    })
}

fn parse_full_response(object: &Object<'_>) -> Result<ScriptMockResponse> {
    let status_number: i32 = object
        .get("status")
        .map_err(|_| MockproxyrsError::Script("response.status must be a number".to_string()))?;
    if !(100..=599).contains(&status_number) {
        return Err(MockproxyrsError::Script(
            "response.status must be between 100 and 599".to_string(),
        ));
    }

    let headers = if object.contains_key("headers").map_err(to_script_error)? {
        let headers_obj: Object = object.get("headers").map_err(|_| {
            MockproxyrsError::Script("response.headers must be an object".to_string())
        })?;
        object_to_string_map(&headers_obj)?
    } else {
        default_headers()
    };

    let body = if object.contains_key("body").map_err(to_script_error)? {
        object
            .get("body")
            .map_err(|_| MockproxyrsError::Script("response.body must be a string".to_string()))?
    } else {
        String::new()
    };

    Ok(ScriptMockResponse {
        status: status_number as u16,
        headers,
        body,
    })
}

fn object_to_string_map(object: &Object<'_>) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for pair in object.props::<String, Value>() {
        let (key, value) = pair.map_err(to_script_error)?;
        let value = if let Some(s) = value.as_string() {
            s.to_string().map_err(to_script_error)?
        } else if value.is_null() {
            "null".to_string()
        } else if value.is_undefined() {
            "undefined".to_string()
        } else if let Some(n) = value.as_number() {
            n.to_string()
        } else {
            format!("{:?}", value)
        };
        map.insert(key.to_lowercase(), value);
    }
    Ok(map)
}

fn stringify_value<'js>(ctx: rquickjs::Ctx<'js>, value: Value<'js>) -> Result<String> {
    if let Some(s) = value.as_string() {
        return s.to_string().map_err(to_script_error);
    }

    let json = ctx
        .json_stringify(value)
        .map_err(to_script_error)?
        .ok_or_else(|| {
            MockproxyrsError::Script("response body cannot be stringified".to_string())
        })?;
    json.to_string().map_err(to_script_error)
}

fn default_headers() -> HashMap<String, String> {
    HashMap::from([("content-type".to_string(), DEFAULT_CONTENT_TYPE.to_string())])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_body(body: &str) -> RequestContext {
        RequestContext::new(
            "POST".to_string(),
            "/api/users?id=42".to_string(),
            HashMap::from([("content-type".to_string(), "application/json".to_string())]),
            body.to_string(),
        )
    }

    #[tokio::test]
    async fn test_execute_full_response() {
        let engine = ScriptEngine::new().unwrap();
        let response = execute_script(
            engine,
            ctx_with_body("{}"),
            r#"return { status: 201, headers: { "x-test": "ok" }, body: "created" };"#.to_string(),
        )
        .await
        .unwrap();

        assert_eq!(response.status, 201);
        assert_eq!(response.headers.get("x-test"), Some(&"ok".to_string()));
        assert_eq!(response.body, "created");
    }

    #[tokio::test]
    async fn test_execute_reads_request_context() {
        let engine = ScriptEngine::new().unwrap();
        let response = execute_script(
            engine,
            ctx_with_body(r#"{"name":"alice"}"#),
            r#"
            const data = JSON.parse(request.body);
            return {
                method: request.method,
                path: request.path,
                id: request.query.id,
                contentType: request.headers["content-type"],
                name: data.name
            };
            "#
            .to_string(),
        )
        .await
        .unwrap();

        assert_eq!(response.status, 200);
        assert!(response.body.contains(r#""method":"POST""#));
        assert!(response.body.contains(r#""path":"/api/users""#));
        assert!(response.body.contains(r#""id":"42""#));
        assert!(response.body.contains(r#""name":"alice""#));
    }

    #[tokio::test]
    async fn test_execute_shorthand_string() {
        let engine = ScriptEngine::new().unwrap();
        let response = execute_script(engine, ctx_with_body(""), r#"return "hello";"#.to_string())
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("content-type").unwrap(),
            "application/json;charset=UTF-8"
        );
        assert_eq!(response.body, "hello");
    }

    #[tokio::test]
    async fn test_execute_shorthand_object() {
        let engine = ScriptEngine::new().unwrap();
        let response = execute_script(
            engine,
            ctx_with_body(""),
            r#"return { code: 0 };"#.to_string(),
        )
        .await
        .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.body, r#"{"code":0}"#);
    }

    #[tokio::test]
    async fn test_execute_rejects_undefined() {
        let engine = ScriptEngine::new().unwrap();
        let result = execute_script(engine, ctx_with_body(""), "const x = 1;".to_string()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("undefined"));
    }

    #[tokio::test]
    async fn test_execute_rejects_bad_status() {
        let engine = ScriptEngine::new().unwrap();
        let result = execute_script(
            engine,
            ctx_with_body(""),
            r#"return { status: 99, body: "bad" };"#.to_string(),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_console_methods_do_not_fail() {
        let engine = ScriptEngine::new().unwrap();
        let response = execute_script(
            engine,
            ctx_with_body(""),
            r#"
            console.log("hello", { code: 0 });
            console.error("warn", 1);
            return "ok";
            "#
            .to_string(),
        )
        .await
        .unwrap();
        assert_eq!(response.body, "ok");
    }

    #[tokio::test]
    async fn test_execute_times_out_infinite_loop() {
        let engine = ScriptEngine::new().unwrap();
        let started = std::time::Instant::now();
        let result = execute_script(engine, ctx_with_body(""), "while (true) {}".to_string()).await;
        assert!(result.is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
