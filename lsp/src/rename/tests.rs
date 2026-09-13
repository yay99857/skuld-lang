use super::*;
use skuld_compiler::{check_program, module::NoModules};

fn checked(source: &str) -> TypedProgram {
    check_program("main.skuld", source, &mut NoModules).unwrap_or_else(|errors| {
        panic!("the fixture must check:\n{}", errors.render());
    })
}

/// The symbol named at the first occurrence of `needle`, asked one byte in so
/// the test never depends on the cursor resting at a word's start.
fn at(source: &str, needle: &str, typed: &TypedProgram) -> Result<(SymbolId, Word), Refusal> {
    let offset = source.find(needle).expect("the occurrence") + 1;
    nameable(source, offset, typed)
}

const PROGRAM: &str = "\
class User {
    name: string

    greet() -> string {
        return this.name
    }
}

enum Shape {
    Dot
    Box(int)
}

func area(width: int, height: int) -> int {
    return width * height
}

func main() {
    let user = new User(name: \"Ada\")
    print(user.greet())
    print(area(2, 3))
    print(area(4, 5))
}
";

#[test]
fn a_declaration_and_all_of_its_uses_are_one_list() {
    let typed = checked(PROGRAM);
    let (symbol, word) = at(PROGRAM, "area(width", &typed).expect("a function");
    assert_eq!(word.text, "area");
    let found = occurrences(&typed, symbol);
    // The declaration and both calls, in source order.
    let starts: Vec<usize> = found
        .iter()
        .map(|occurrence| occurrence.span.start)
        .collect();
    let expected: Vec<usize> = PROGRAM
        .match_indices("area")
        .map(|(offset, _)| offset)
        .collect();
    assert_eq!(starts, expected);
    for occurrence in &found {
        assert_eq!(&PROGRAM[occurrence.span.start..occurrence.span.end], "area");
    }
}

#[test]
fn a_local_binding_is_found_from_a_use_as_well_as_from_its_declaration() {
    let typed = checked(PROGRAM);
    let from_use = at(PROGRAM, "user.greet", &typed).expect("a binding");
    let from_declaration = at(PROGRAM, "user = new", &typed).expect("a binding");
    assert_eq!(from_use.0, from_declaration.0);
    assert_eq!(occurrences(&typed, from_use.0).len(), 2);
}

#[test]
fn a_parameter_is_renameable_and_scoped_to_its_function() {
    let typed = checked(PROGRAM);
    let (symbol, _) = at(PROGRAM, "width * height", &typed).expect("a parameter");
    assert_eq!(occurrences(&typed, symbol).len(), 2);
}

#[test]
fn names_the_server_will_not_move_are_refused_by_what_they_are() {
    let typed = checked(PROGRAM);
    assert_eq!(
        at(PROGRAM, "print(user", &typed).unwrap_err(),
        Refusal::Prelude
    );
    // A type name in an annotation, and the enum name as a value qualifier.
    assert_eq!(at(PROGRAM, "User {", &typed).unwrap_err(), Refusal::Type);
    assert_eq!(at(PROGRAM, "Shape {", &typed).unwrap_err(), Refusal::Type);
    // A method reached through its receiver.
    assert_eq!(
        at(PROGRAM, "greet())", &typed).unwrap_err(),
        Refusal::Member
    );
    // Whitespace names nothing at all.
    assert_eq!(nameable(PROGRAM, 0, &typed).unwrap_err(), Refusal::NotAName);
}

#[test]
fn only_a_single_identifier_is_a_new_name() {
    assert!(is_identifier("total"));
    assert!(is_identifier("_x9"));
    assert!(!is_identifier(""));
    assert!(!is_identifier("func"));
    assert!(!is_identifier("two words"));
    assert!(!is_identifier("9lives"));
    assert!(!is_identifier("total "));
    assert!(!is_identifier("a+b"));
}

#[test]
fn a_shape_records_where_each_name_resolves() {
    let typed = checked(PROGRAM);
    let path = |file: FileId| Some(format!("f{}", file.0));
    let shape = shape(&typed, &path);
    let call = PROGRAM.find("area(2, 3)").expect("the call");
    let declaration = PROGRAM.find("area(width").expect("the declaration");
    assert_eq!(
        shape.get(&("f0".to_string(), call)),
        Some(&Site::Declared("f0".to_string(), declaration))
    );
    // A prelude binding is identified by what it is, having nowhere to be
    // declared.
    let printed = PROGRAM.find("print(user").expect("the call");
    assert_eq!(
        shape.get(&("f0".to_string(), printed)),
        Some(&Site::Prelude("print".to_string()))
    );
}

#[test]
fn shifting_a_shape_moves_offsets_after_an_edit_and_leaves_the_rest() {
    let mut original = BTreeMap::new();
    original.insert(("a".to_string(), 10), Site::Declared("a".to_string(), 4));
    original.insert(("a".to_string(), 2), Site::Prelude("print".to_string()));
    original.insert(("b".to_string(), 10), Site::Declared("a".to_string(), 4));
    // One edit at offset 4 in `a`, lengthening the name by two bytes.
    let shift = |file: &str, offset: usize| {
        if file == "a" && offset > 4 {
            offset + 2
        } else {
            offset
        }
    };
    let moved = shifted(&original, &shift);
    assert_eq!(
        moved.get(&("a".to_string(), 12)),
        Some(&Site::Declared("a".to_string(), 4))
    );
    assert!(moved.contains_key(&("a".to_string(), 2)));
    // A file with no edit keeps its offsets, but the site it points at moved.
    assert_eq!(
        moved.get(&("b".to_string(), 10)),
        Some(&Site::Declared("a".to_string(), 4))
    );
}
