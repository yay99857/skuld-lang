use super::*;

#[test]
fn finds_lines_and_columns() {
    let positions = Positions::new("func main() {\n    print(1)\n}\n");
    assert_eq!(
        positions.position(0),
        Position {
            line: 0,
            character: 0
        }
    );
    assert_eq!(
        positions.position(5),
        Position {
            line: 0,
            character: 5
        }
    );
    assert_eq!(
        positions.position(14),
        Position {
            line: 1,
            character: 0
        }
    );
    assert_eq!(
        positions.position(18),
        Position {
            line: 1,
            character: 4
        }
    );
}

#[test]
fn counts_a_column_in_utf16_units() {
    // `é` is two bytes and one UTF-16 unit; the emoji is four bytes and TWO
    // UTF-16 units. Counting scalars, as the compiler's own renderer does,
    // would put the position one unit early for every astral character.
    let text = "let s = \"é😀\"\nx";
    let positions = Positions::new(text);
    let emoji = text.find('😀').unwrap();
    assert_eq!(
        positions.position(emoji),
        Position {
            line: 0,
            character: 10
        }
    );
    let after_emoji = emoji + '😀'.len_utf8();
    assert_eq!(
        positions.position(after_emoji),
        Position {
            line: 0,
            character: 12
        }
    );
    assert_eq!(text[..after_emoji].chars().count(), 11);
}

#[test]
fn clamps_an_offset_past_the_end() {
    let positions = Positions::new("abc");
    assert_eq!(
        positions.position(999),
        Position {
            line: 0,
            character: 3
        }
    );
}

#[test]
fn rounds_an_offset_inside_a_character_down() {
    // A span that starts mid-character would panic a naive slice.
    let positions = Positions::new("é");
    assert_eq!(
        positions.position(1),
        Position {
            line: 0,
            character: 0
        }
    );
}

#[test]
fn an_empty_text_has_one_position() {
    let positions = Positions::new("");
    assert_eq!(
        positions.position(0),
        Position {
            line: 0,
            character: 0
        }
    );
}

#[test]
fn a_trailing_newline_opens_a_final_line() {
    let positions = Positions::new("a\n");
    assert_eq!(
        positions.position(2),
        Position {
            line: 1,
            character: 0
        }
    );
}

#[test]
fn reads_a_file_uri() {
    assert_eq!(
        uri_to_path("file:///home/jm/x.skuld").as_deref(),
        Some("/home/jm/x.skuld")
    );
    assert_eq!(
        uri_to_path("file:///home/jm/a%20b/%C3%A9.skuld").as_deref(),
        Some("/home/jm/a b/é.skuld")
    );
}

#[test]
fn refuses_a_uri_that_is_not_a_local_file() {
    // Following these would read from somewhere the client never opened.
    assert_eq!(uri_to_path("http://example.com/x.skuld"), None);
    assert_eq!(uri_to_path("file://remote-host/x.skuld"), None);
    assert_eq!(uri_to_path("untitled:Untitled-1"), None);
}

#[test]
fn a_path_roundtrips_through_a_uri() {
    for path in [
        "/home/jm/x.skuld",
        "/home/jm/a b/é.skuld",
        "/tmp/with#hash/and?question.skuld",
    ] {
        assert_eq!(uri_to_path(&path_to_uri(path)).as_deref(), Some(path));
    }
}

#[test]
fn encodes_the_characters_a_uri_cannot_carry_raw() {
    assert_eq!(path_to_uri("/a b.skuld"), "file:///a%20b.skuld");
    assert_eq!(path_to_uri("/é.skuld"), "file:///%C3%A9.skuld");
}
