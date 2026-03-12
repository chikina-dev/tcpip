#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status_code: u16,
    pub reason: &'static str,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut lines = text.split("\r\n");
        let request_line = lines.next()?;
        let mut parts = request_line.split_whitespace();

        Some(Self {
            method: parts.next()?.to_string(),
            path: parts.next()?.to_string(),
        })
    }
}

impl HttpResponse {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status_code: 200,
            reason: "OK",
            body: body.into(),
        }
    }

    pub fn not_found(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status_code: 404,
            reason: "Not Found",
            body: body.into(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let header = format!(
            "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.status_code,
            self.reason,
            self.body.len()
        );
        let mut bytes = header.into_bytes();
        bytes.extend_from_slice(&self.body);
        bytes
    }
}

pub fn build_get_request(path: &str, host: &str) -> Vec<u8> {
    format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").into_bytes()
}

pub fn http_message_complete(bytes: &[u8]) -> bool {
    let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let headers = &bytes[..header_end + 4];
    let header_text = match std::str::from_utf8(headers) {
        Ok(text) => text,
        Err(_) => return false,
    };

    let content_length = header_text
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);

    bytes.len() >= header_end + 4 + content_length
}
