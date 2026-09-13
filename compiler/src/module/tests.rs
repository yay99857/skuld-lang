use super::*;
use std::collections::BTreeMap;

/// A loader backed by a map, so the graph rules can be tested without a
/// filesystem — which is the point of the loader being a caller's concern.
struct Fake {
    modules: BTreeMap<&'static str, Vec<(&'static str, &'static str)>>,
}

impl Fake {
    fn new(modules: &[(&'static str, &[(&'static str, &'static str)])]) -> Self {
        Self {
            modules: modules
                .iter()
                .map(|(path, files)| (*path, files.to_vec()))
                .collect(),
        }
    }
}

impl ModuleLoader for Fake {
    fn load(&mut self, path: &str) -> Result<Vec<(String, String)>, String> {
        match self.modules.get(path) {
            Some(files) => Ok(files
                .iter()
                .map(|(name, source)| ((*name).to_owned(), (*source).to_owned()))
                .collect()),
            None => Err("no such module".into()),
        }
    }
}

fn codes(errors: &Errors) -> Vec<DiagnosticCode> {
    errors
        .diagnostics
        .iter()
        .map(|entry| entry.diagnostic.code)
        .collect()
}

#[test]
fn a_module_is_loaded_once_however_many_files_import_it() {
    let mut loader = Fake::new(&[
        (
            "shared",
            &[("shared/one.skuld", "pub func a() -> int { return 1 }")],
        ),
        (
            "left",
            &[(
                "left/l.skuld",
                "import \"shared\"\npub func b() -> int { return shared.a() }",
            )],
        ),
    ]);
    let program = load(
        "main.skuld",
        "import \"shared\"\nimport \"left\"\nfunc main() { }",
        &mut loader,
    )
    .expect("a valid program");
    let paths: Vec<_> = program
        .modules
        .iter()
        .map(|module| module.path.as_str())
        .collect();
    assert_eq!(paths, vec!["", "shared", "left"]);
    assert_eq!(program.files.len(), 3);
    // The root module is the entry file alone.
    assert_eq!(program.modules[0].files, vec![FileId(0)]);
}

#[test]
fn a_qualifier_is_the_last_path_segment() {
    let mut loader = Fake::new(&[(
        "net/socket",
        &[(
            "net/socket/s.skuld",
            "pub func connect() -> int { return 1 }",
        )],
    )]);
    let program = load(
        "main.skuld",
        "import \"net/socket\"\nfunc main() { }",
        &mut loader,
    )
    .expect("a valid program");
    assert_eq!(program.modules[1].qualifier, "socket");
}

#[test]
fn every_file_of_a_module_is_part_of_it() {
    let mut loader = Fake::new(&[(
        "pair",
        &[
            ("pair/a.skuld", "pub func a() -> int { return 1 }"),
            ("pair/b.skuld", "pub func b() -> int { return a() }"),
        ],
    )]);
    let program = load(
        "main.skuld",
        "import \"pair\"\nfunc main() { }",
        &mut loader,
    )
    .expect("a valid program");
    assert_eq!(program.modules[1].files, vec![FileId(1), FileId(2)]);
}

#[test]
fn a_cycle_between_modules_is_rejected() {
    let mut loader = Fake::new(&[
        (
            "a",
            &[(
                "a/a.skuld",
                "import \"b\"\npub func a() -> int { return 1 }",
            )],
        ),
        (
            "b",
            &[(
                "b/b.skuld",
                "import \"a\"\npub func b() -> int { return 1 }",
            )],
        ),
    ]);
    let errors =
        load("main.skuld", "import \"a\"\nfunc main() { }", &mut loader).expect_err("a cycle");
    assert_eq!(codes(&errors), vec![DiagnosticCode::ImportCycle]);
}

#[test]
fn a_module_importing_itself_is_a_cycle() {
    let mut loader = Fake::new(&[(
        "self",
        &[(
            "self/s.skuld",
            "import \"self\"\npub func s() -> int { return 1 }",
        )],
    )]);
    let errors = load(
        "main.skuld",
        "import \"self\"\nfunc main() { }",
        &mut loader,
    )
    .expect_err("a cycle");
    assert_eq!(codes(&errors), vec![DiagnosticCode::ImportCycle]);
}

#[test]
fn a_diamond_is_not_a_cycle() {
    let mut loader = Fake::new(&[
        (
            "base",
            &[("base/base.skuld", "pub func v() -> int { return 1 }")],
        ),
        (
            "left",
            &[(
                "left/l.skuld",
                "import \"base\"\npub func l() -> int { return base.v() }",
            )],
        ),
        (
            "right",
            &[(
                "right/r.skuld",
                "import \"base\"\npub func r() -> int { return base.v() }",
            )],
        ),
    ]);
    load(
        "main.skuld",
        "import \"left\"\nimport \"right\"\nfunc main() { }",
        &mut loader,
    )
    .expect("a diamond is fine");
}

#[test]
fn a_module_the_loader_cannot_find_is_reported_where_it_is_imported() {
    let mut loader = Fake::new(&[]);
    let errors = load(
        "main.skuld",
        "import \"absent\"\nfunc main() { }",
        &mut loader,
    )
    .expect_err("a missing module");
    assert_eq!(codes(&errors), vec![DiagnosticCode::UnknownModule]);
    assert_eq!(errors.diagnostics[0].file, FileId(0));
}

#[test]
fn an_empty_module_is_not_a_module() {
    let mut loader = Fake::new(&[("empty", &[])]);
    let errors = load(
        "main.skuld",
        "import \"empty\"\nfunc main() { }",
        &mut loader,
    )
    .expect_err("nothing to import");
    assert_eq!(codes(&errors), vec![DiagnosticCode::UnknownModule]);
}

#[test]
fn two_paths_cannot_share_one_qualifier_in_a_file() {
    let mut loader = Fake::new(&[
        (
            "json",
            &[("json/j.skuld", "pub func a() -> int { return 1 }")],
        ),
        (
            "other/json",
            &[("other/json/j.skuld", "pub func b() -> int { return 1 }")],
        ),
    ]);
    let errors = load(
        "main.skuld",
        "import \"json\"\nimport \"other/json\"\nfunc main() { }",
        &mut loader,
    )
    .expect_err("one qualifier, two modules");
    assert_eq!(codes(&errors), vec![DiagnosticCode::DuplicateDeclaration]);
}

#[test]
fn a_file_that_does_not_parse_keeps_its_own_diagnostics() {
    let mut loader = Fake::new(&[("broken", &[("broken/b.skuld", "func (")])]);
    let errors = load(
        "main.skuld",
        "import \"broken\"\nfunc main() { }",
        &mut loader,
    )
    .expect_err("a parse error");
    // Reported against the module's file, not the entry that imported it.
    assert_eq!(errors.diagnostics[0].file, FileId(1));
    assert_eq!(errors.sources[1].name, "broken/b.skuld");
}

#[test]
fn a_directory_named_std_cannot_shadow_the_standard_library() {
    // The loader offers its own `std/utf8`; the reservation means it is never
    // asked, so the embedded module is what the program sees.
    let mut loader = Fake::new(&[(
        "std/utf8",
        &[(
            "std/utf8/decoy.skuld",
            "pub func decoy() -> int { return 1 }",
        )],
    )]);
    let errors = load(
        "main.skuld",
        "import \"std/utf8\"\nfunc main() { print(utf8.decoy()) }",
        &mut loader,
    )
    .map(|program| {
        // Loading succeeds either way; what differs is whose files arrived.
        program
            .files
            .iter()
            .map(|file| file.name.clone())
            .collect::<Vec<_>>()
    })
    .expect("the embedded module loads");
    assert!(
        errors.contains(&"std/utf8/decode.skuld".to_owned()),
        "{errors:?}"
    );
    assert!(
        !errors.iter().any(|name| name.contains("decoy")),
        "{errors:?}"
    );
}
