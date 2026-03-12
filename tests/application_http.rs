use tcpip_userland::application::http::{HttpRequest, HttpResponse, http_message_complete};

#[test]
fn parses_get_request() {
    let request = HttpRequest::parse(b"GET /hello HTTP/1.1\r\nHost: example\r\n\r\n").unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/hello");
}

#[test]
fn detects_complete_http_message() {
    let response = HttpResponse::ok("hello").encode();
    assert!(http_message_complete(&response));
    assert!(!http_message_complete(
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhe"
    ));
}
