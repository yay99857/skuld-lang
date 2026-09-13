use super::*;
use std::io::BufReader;

fn read_all(input: &str) -> Result<Json, ReadError> {
    read(&mut BufReader::new(input.as_bytes()))
}

#[test]
fn reads_a_framed_message() {
    let message = read_all("Content-Length: 16\r\n\r\n{\"jsonrpc\":\"2\"}\n").unwrap();
    assert_eq!(message.get("jsonrpc").and_then(Json::as_str), Some("2"));
}

#[test]
fn ignores_other_headers_and_header_case() {
    let body = r#"{"id":3}"#;
    let input = format!(
        "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n\
         content-length: {}\r\n\r\n{body}",
        body.len()
    );
    assert_eq!(
        read_all(&input).unwrap().get("id").unwrap().as_i64(),
        Some(3)
    );
}

#[test]
fn counts_the_body_in_bytes_not_characters() {
    // `é` is two bytes and one character. A length counted in characters would
    // truncate the body and desynchronise every message after it.
    let body = r#"{"m":"é"}"#;
    assert_eq!(body.chars().count(), 9);
    assert_eq!(body.len(), 10);
    let input = format!("Content-Length: {}\r\n\r\n{body}", body.len());
    assert_eq!(
        read_all(&input).unwrap().get("m").and_then(Json::as_str),
        Some("é")
    );
}

#[test]
fn a_closed_stream_is_not_an_error() {
    assert!(matches!(read_all(""), Err(ReadError::Closed)));
}

#[test]
fn reports_a_missing_length() {
    let error = read_all("Content-Type: x\r\n\r\n{}").unwrap_err();
    assert!(matches!(error, ReadError::Malformed(_)));
    assert!(error.to_string().contains("Content-Length"));
}

#[test]
fn reports_a_body_shorter_than_its_header_claims() {
    assert!(matches!(
        read_all("Content-Length: 100\r\n\r\n{}"),
        Err(ReadError::Malformed(_))
    ));
}

#[test]
fn reports_a_malformed_header_and_bad_json() {
    assert!(matches!(
        read_all("nonsense\r\n\r\n{}"),
        Err(ReadError::Malformed(_))
    ));
    assert!(matches!(
        read_all("Content-Length: 3\r\n\r\n{ ["),
        Err(ReadError::Malformed(_))
    ));
}

#[test]
fn writes_a_frame_whose_length_matches_its_body() {
    let mut out = Vec::new();
    write(&mut out, &Json::object([("id", Json::number(1))])).unwrap();
    let text = String::from_utf8(out).unwrap();
    let (header, body) = text.split_once("\r\n\r\n").expect("a blank line");
    assert_eq!(body, r#"{"id":1}"#);
    assert_eq!(header, format!("Content-Length: {}", body.len()));
}

#[test]
fn a_written_frame_reads_back() {
    let original = Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("method", Json::string("olá/método")),
    ]);
    let mut out = Vec::new();
    write(&mut out, &original).unwrap();
    assert_eq!(read(&mut BufReader::new(&out[..])).unwrap(), original);
}
