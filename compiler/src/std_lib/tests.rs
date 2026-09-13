use super::*;
use crate::{check_program, module::NoModules};

#[test]
fn every_embedded_module_compiles_on_its_own() {
    // A library that ships with the compiler must never ship broken: each
    // module is checked here, imported by a program that does nothing else.
    for path in paths() {
        let qualifier = path.rsplit('/').next().expect("qualifier");
        let entry = format!("import \"{path}\"\nfunc main() {{ }}");
        if let Err(errors) = check_program("main.skuld", &entry, &mut NoModules) {
            panic!("{qualifier} does not compile:\n{}", errors.render());
        }
    }
}

#[test]
fn a_reserved_path_never_reaches_the_loader() {
    assert!(is_reserved("std"));
    assert!(is_reserved("std/utf8"));
    assert!(is_reserved("std/nothing/here"));
    // Only the exact first segment is reserved; these are ordinary paths.
    assert!(!is_reserved("stdlib"));
    assert!(!is_reserved("a/std"));
    assert!(!is_reserved("standard"));
}

#[test]
fn an_unknown_reserved_module_names_the_ones_that_exist() {
    let errors = check_program(
        "main.skuld",
        "import \"std/nope\"\nfunc main() { }",
        &mut NoModules,
    )
    .expect_err("unknown std module");
    let rendered = errors.render();
    assert!(rendered.contains("reserved prefix"), "{rendered}");
    assert!(rendered.contains("std/utf8"), "{rendered}");
}
