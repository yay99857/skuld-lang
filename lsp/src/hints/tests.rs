use super::*;

fn hints_of(source: &str) -> Vec<(usize, String)> {
    let typed = skuld_compiler::check(source).expect("the fixture checks");
    type_hints(source, &typed)
        .into_iter()
        .map(|hint| (hint.offset, hint.label))
        .collect()
}

/// The label of each hint, which is what a reader sees.
fn labels(source: &str) -> Vec<String> {
    hints_of(source)
        .into_iter()
        .map(|(_, label)| label)
        .collect()
}

#[test]
fn reports_the_inferred_type_of_a_binding() {
    let source = "func main() {\n    let count = 1\n    print(count)\n}\n";
    let hints = hints_of(source);
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].1, ": int");
    // The hint sits immediately after the name, where the annotation would be.
    assert_eq!(&source[..hints[0].0], "func main() {\n    let count");
}

#[test]
fn says_nothing_where_the_type_is_already_written() {
    assert_eq!(
        labels("func main() {\n    let count: int = 1\n    print(count)\n}\n"),
        Vec::<String>::new()
    );
    // Spacing before the colon is the user's choice, and does not make the
    // annotation disappear.
    assert_eq!(
        labels("func main() {\n    let count : int = 1\n    print(count)\n}\n"),
        Vec::<String>::new()
    );
}

#[test]
fn covers_a_var_as_well_as_a_let() {
    assert_eq!(
        labels(
            "func main() {\n    var total = 1.5\n    total = total + 1.0\n    print(total)\n}\n"
        ),
        [": float"]
    );
}

#[test]
fn covers_the_binders_that_never_take_an_annotation() {
    // `for`, `if let` and a match arm's payload all bind a name the user
    // cannot annotate, which is exactly where the type is least obvious.
    let source = "enum Shape {\n    Circle(int),\n    Dot\n}\n\nfunc main() {\n    for index in 0..3 {\n        print(index)\n    }\n    let maybe: Option<string> = \"a\"\n    if let text = maybe {\n        print(text)\n    }\n    let shape = Shape.Circle(2)\n    match shape {\n        Shape.Circle(radius): print(radius)\n        Shape.Dot: print(0)\n    }\n}\n";
    assert_eq!(labels(source), [": int", ": string", ": Shape", ": int"]);
}

#[test]
fn names_a_declared_type_the_way_it_was_declared() {
    let source = "class User {\n    name: string\n}\n\nfunc main() {\n    let user = new User(name: \"a\")\n    print(user.name)\n}\n";
    assert_eq!(labels(source), [": User"]);
}

#[test]
fn a_binding_inside_a_lambda_is_hinted_too() {
    // The hints come from the resolver's table rather than a walk over
    // statements, so a body nested in an expression is not a special case.
    let source = "func main() {\n    var values = [3, 1, 2]\n    values.sort((a: int, b: int) -> int {\n        let first = a\n        return first - b\n    })\n    print(values[0])\n}\n";
    assert_eq!(labels(source), [": []int", ": int"]);
}
