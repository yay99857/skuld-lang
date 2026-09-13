use super::*;

fn roundtrip(text: &str) -> String {
    parse(text).expect("parses").to_text()
}

#[test]
fn reads_the_scalar_forms() {
    assert_eq!(parse("null").unwrap(), Json::Null);
    assert_eq!(parse("true").unwrap(), Json::Bool(true));
    assert_eq!(parse("false").unwrap(), Json::Bool(false));
    assert_eq!(parse("  42 ").unwrap(), Json::Number(42.0));
    assert_eq!(parse("-3.5e2").unwrap(), Json::Number(-350.0));
    assert_eq!(parse(r#""hi""#).unwrap(), Json::String("hi".into()));
}

#[test]
fn an_integer_is_written_without_a_fractional_part() {
    // A client reading `id` as an integer must not receive `1.0`.
    assert_eq!(Json::Number(1.0).to_text(), "1");
    assert_eq!(Json::Number(-7.0).to_text(), "-7");
    assert_eq!(Json::Number(0.5).to_text(), "0.5");
}

#[test]
fn reads_nested_structures() {
    let value = parse(r#"{"a":[1,{"b":null}],"c":true}"#).unwrap();
    assert_eq!(value.path(&["a"]).unwrap().as_array().unwrap().len(), 2);
    assert_eq!(value.path(&["c"]).unwrap(), &Json::Bool(true));
    assert!(value.path(&["a", "b"]).is_none());
}

#[test]
fn follows_a_key_path() {
    let value = parse(r#"{"params":{"textDocument":{"uri":"file:///x.skuld"}}}"#).unwrap();
    assert_eq!(
        value
            .path(&["params", "textDocument", "uri"])
            .and_then(Json::as_str),
        Some("file:///x.skuld")
    );
    assert!(value.path(&["params", "missing", "uri"]).is_none());
}

#[test]
fn reads_every_escape() {
    let value = parse(r#""a\"b\\c\/d\be\ff\ng\rh\ti""#).unwrap();
    assert_eq!(value.as_str().unwrap(), "a\"b\\c/d\u{08}e\u{0c}f\ng\rh\ti");
}

#[test]
fn joins_a_surrogate_pair() {
    // A client sends astral characters as a pair; decoding each half on its
    // own would produce two replacement characters instead of one emoji.
    assert_eq!(parse(r#""😀""#).unwrap().as_str(), Some("\u{1f600}"));
    assert_eq!(parse(r#""é""#).unwrap().as_str(), Some("\u{e9}"));
}

#[test]
fn a_lone_surrogate_becomes_the_replacement_character() {
    // Not a panic and not a failed message: one bad name must not cost the
    // whole request.
    assert_eq!(parse(r#""\ud83d""#).unwrap().as_str(), Some("\u{fffd}"));
    assert_eq!(parse(r#""\udc00x""#).unwrap().as_str(), Some("\u{fffd}x"));
}

#[test]
fn writes_control_characters_as_escapes() {
    // An unescaped control character makes the whole message invalid JSON.
    let text = Json::string("a\u{1}b\nc").to_text();
    assert_eq!(text, r#""a\u0001b\nc""#);
    assert_eq!(parse(&text).unwrap().as_str(), Some("a\u{1}b\nc"));
}

#[test]
fn survives_a_roundtrip() {
    let text = r#"{"id":1,"jsonrpc":"2.0","result":{"items":[],"ok":true}}"#;
    assert_eq!(roundtrip(text), text);
}

#[test]
fn multibyte_text_is_not_split() {
    let original = "ol\u{e1} \u{2014} \u{65e5}\u{672c}\u{8a9e}";
    let text = Json::string(original).to_text();
    assert_eq!(parse(&text).unwrap().as_str(), Some(original));
}

#[test]
fn rejects_malformed_input() {
    for bad in [
        "",
        "{",
        "[1,]",
        "{\"a\"}",
        "{\"a\":1,}",
        "nul",
        "\"unterminated",
        "1 2",
        "{\"a\":1} trailing",
    ] {
        assert!(parse(bad).is_err(), "`{bad}` should not parse");
    }
}

#[test]
fn rejects_an_unescaped_control_character() {
    assert!(parse("\"a\nb\"").is_err());
}

#[test]
fn refuses_input_nested_past_the_depth_limit() {
    // Deep recursion would abort the process; an error keeps the server alive.
    let deep = format!("{}1{}", "[".repeat(500), "]".repeat(500));
    assert!(parse(&deep).is_err());
}
