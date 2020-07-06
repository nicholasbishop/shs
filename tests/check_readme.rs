/// Test that the contents of the example in the README.md match
/// examples/hello.rs.
#[test]
fn check_readme_example() {
    let readme = include_str!("../README.md");
    let example = include_str!("../examples/hello.rs");

    let code = format!("```rust\n{}```", example);
    assert!(readme.contains(&code));
}
