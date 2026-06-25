use std::collections::HashMap;

/// 注入 JS 脚本的请求上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub method: String,
    pub url: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub query: HashMap<String, String>,
}

impl RequestContext {
    pub fn new(
        method: String,
        url: String,
        headers: HashMap<String, String>,
        body: String,
    ) -> Self {
        let path = url.split('?').next().unwrap_or(&url).to_string();
        let query = parse_query(&url);
        Self {
            method,
            url,
            path,
            headers,
            body,
            query,
        }
    }
}

fn parse_query(url: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let Some(query) = url.split_once('?').map(|(_, q)| q) else {
        return result;
    };

    for pair in query.split('&').filter(|s| !s.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        result.insert(percent_decode(key), percent_decode(value));
    }

    result
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = from_hex(bytes[i + 1]);
                let lo = from_hex(bytes[i + 2]);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_context_parses_path_and_query() {
        let ctx = RequestContext::new(
            "GET".to_string(),
            "/api/users?id=1&page=2&name=alice+bob&encoded=%E4%B8%AD".to_string(),
            HashMap::new(),
            String::new(),
        );

        assert_eq!(ctx.path, "/api/users");
        assert_eq!(ctx.query.get("id"), Some(&"1".to_string()));
        assert_eq!(ctx.query.get("page"), Some(&"2".to_string()));
        assert_eq!(ctx.query.get("name"), Some(&"alice bob".to_string()));
        assert_eq!(ctx.query.get("encoded"), Some(&"中".to_string()));
    }

    #[test]
    fn test_request_context_handles_missing_query() {
        let ctx = RequestContext::new(
            "POST".to_string(),
            "/api/users".to_string(),
            HashMap::new(),
            "{}".to_string(),
        );

        assert_eq!(ctx.path, "/api/users");
        assert!(ctx.query.is_empty());
    }
}
