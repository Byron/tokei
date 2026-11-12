use std::fs;

use tempfile::TempDir;
use tokei::{CodeStats, Config, LanguageType, Languages};

const INLINE: &str = "fn production() {}\n\
#[cfg(test)]\n\
mod tests {\n\
    // A test comment.\n\
\n\
    #[test]\n\
    fn nested() {\n\
        if true { assert!(true); }\n\
    }\n\
}\n\
#[test]\n\
fn standalone() {}\n\
fn after() {}\n";

fn counts(stats: &CodeStats) -> (usize, usize, usize) {
    let stats = stats.summarise();
    (stats.code, stats.comments, stats.blanks)
}

fn unsplit() -> Config {
    Config {
        rust_tests: Some(false),
        ..Config::default()
    }
}

#[test]
fn inline_tests_are_separate_by_default_and_opt_out_preserves_totals() {
    let stats = LanguageType::Rust.parse_from_str(INLINE, &Config::default());
    let tests = &stats.blobs[&LanguageType::RustTest];
    assert_eq!((stats.code, stats.comments, stats.blanks), (2, 0, 0));
    assert_eq!(counts(tests), (9, 1, 1));
    assert_eq!(
        stats,
        LanguageType::Rust.parse_from_str(
            INLINE,
            &Config {
                rust_tests: Some(true),
                ..Config::default()
            }
        )
    );

    let original = LanguageType::Rust.parse_from_str(INLINE, &unsplit());
    assert!(!original.blobs.contains_key(&LanguageType::RustTest));
    assert_eq!(counts(&stats), counts(&original));
}

#[test]
fn braces_and_test_markers_inside_literals_and_comments_do_not_change_scopes() {
    let production = r####"fn production() {
    let marker = r###"#[test] } {"###;
    // #[cfg(test)] }
}
"####;
    let test = r####"#[test]
fn literals() {
    let closing = '}';
    let opening = '{';
    let quote = '\'';
    let string = "escaped \" } {";
    let raw = r###"a quote " and braces } {
#[cfg(test)]
"###;
    /* } /* { */ } */
    if true { assert!(true); }
}
"####;
    let after = "fn after() {}\n";
    let source = format!("{production}{test}{after}");
    let stats = LanguageType::Rust.parse_from_str(&source, &Config::default());

    assert_eq!(
        counts(&stats.blobs[&LanguageType::RustTest]),
        counts(&LanguageType::Rust.parse_from_str(test, &unsplit()))
    );
    let non_tests = LanguageType::Rust.parse_from_str(format!("{production}{after}"), &unsplit());
    assert_eq!(
        (stats.code, stats.comments, stats.blanks),
        counts(&non_tests)
    );
    assert_eq!(
        counts(&stats),
        counts(&LanguageType::Rust.parse_from_str(source, &unsplit()))
    );
}

#[test]
fn attributes_before_and_after_test_markers_belong_to_the_test_item() {
    let test_items = "#[allow(dead_code)]\n\
#[cfg(test)]\n\
#[allow(unused_imports)]\n\
mod helpers { use super::*; }\n\
#[allow(dead_code)]\n\
#[test]\n\
#[ignore]\n\
fn ignored() {}\n\
#[tokio::test]\n\
async fn asynchronous() {}\n\
#[tokio::test(flavor = \"multi_thread\", worker_threads = 2)]\n\
async fn multithreaded() {}\n";
    let source = format!("fn before() {{}}\n{test_items}fn after() {{}}\n");
    let stats = LanguageType::Rust.parse_from_str(source, &Config::default());
    assert_eq!(stats.code, 2);
    assert_eq!(
        counts(&stats.blobs[&LanguageType::RustTest]),
        counts(&LanguageType::Rust.parse_from_str(test_items, &unsplit()))
    );
}

#[test]
fn non_test_cfg_attributes_and_other_languages_are_unchanged() {
    let source = "#[cfg(not(test))]\nfn production() {}\n\
#[cfg(any(test, feature = \"normal\"))]\nfn shared() {}\n";
    let stats = LanguageType::Rust.parse_from_str(source, &Config::default());
    assert_eq!(stats, LanguageType::Rust.parse_from_str(source, &unsplit()));
    assert!(!stats.blobs.contains_key(&LanguageType::RustTest));

    for language in [LanguageType::C, LanguageType::Python] {
        assert_eq!(
            language.parse_from_str(INLINE, &Config::default()),
            language.parse_from_str(INLINE, &unsplit())
        );
    }
}

#[test]
fn test_directory_counts_keep_rust_filter_and_physical_file_totals() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("tests")).unwrap();
    fs::create_dir(dir.path().join("integration-tests")).unwrap();
    fs::write(
        dir.path().join("lib.rs"),
        "fn production() {}\n#[test]\nfn unit() {}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("tests/integration.rs"),
        "fn helper() {}\n#[test]\nfn integration() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("tests/script.py"), "print('hello')\n").unwrap();
    let python = LanguageType::Python
        .parse(dir.path().join("tests/script.py"), &Config::default())
        .unwrap();
    assert_eq!(python.stats.code, 1);
    assert!(!python.stats.blobs.contains_key(&LanguageType::RustTest));
    fs::write(
        dir.path().join("integration-tests/main.rs"),
        "fn main() {}\n",
    )
    .unwrap();
    fs::write(dir.path().join("tests.rs"), "fn main() {}\n").unwrap();

    let mut languages = Languages::new();
    let config = Config {
        types: Some(vec![LanguageType::Rust]),
        ..Config::default()
    };
    languages.get_statistics(&[dir.path()], &[], &config);
    assert_eq!(languages.len(), 1);
    let rust = &languages[&LanguageType::Rust];
    assert_eq!(rust.reports.len(), 4);
    assert_eq!(rust.code, 3);
    let tests = &rust.children[&LanguageType::RustTest];
    assert_eq!(tests.len(), 2);
    assert_eq!(
        tests
            .iter()
            .map(|report| counts(&report.stats).0)
            .sum::<usize>(),
        5
    );
    assert_eq!(languages.total().code, 8);
    assert_eq!(
        languages
            .total()
            .children
            .values()
            .map(Vec::len)
            .sum::<usize>(),
        4
    );

    let encoded = serde_json::to_string(&languages).unwrap();
    assert_eq!(
        serde_json::from_str::<Languages>(&encoded).unwrap(),
        languages
    );

    let mut original = Languages::new();
    original.get_statistics(
        &[dir.path()],
        &[],
        &Config {
            rust_tests: Some(false),
            ..config
        },
    );
    let rust = &original[&LanguageType::Rust];
    assert_eq!(rust.code, 8);
    assert!(!rust.children.contains_key(&LanguageType::RustTest));
}

#[cfg(feature = "cli")]
mod cli {
    use super::*;
    use std::process::Command;

    fn run(dir: &TempDir, args: &[&str]) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_tokei"))
            .current_dir(dir.path())
            .env("NO_COLOR", "1")
            .args(args)
            .arg("sample.rs")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn cli_reports_tests_and_explicit_flag_overrides_config() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("sample.rs"),
            "fn production() {}\n#[test]\nfn unit() {}\n",
        )
        .unwrap();
        fs::write(dir.path().join("tokei.toml"), "rust_tests = true\n").unwrap();

        assert!(run(&dir, &[]).contains("Rust tests"));
        let json: serde_json::Value = serde_json::from_str(&run(&dir, &["--output=json"])).unwrap();
        assert_eq!(json["Rust"]["code"], 1);
        assert_eq!(
            json["Rust"]["children"]["Rust tests"][0]["stats"]["code"],
            2
        );
        assert_eq!(json["Total"]["code"], 3);

        let disabled: serde_json::Value =
            serde_json::from_str(&run(&dir, &["--output=json", "--rust-tests=false"])).unwrap();
        assert_eq!(disabled["Rust"]["code"], 3);
        assert!(disabled["Rust"]["children"].get("Rust tests").is_none());

        fs::write(dir.path().join("tokei.toml"), "rust_tests = false\n").unwrap();
        assert!(!run(&dir, &[]).contains("Rust tests"));
        assert!(run(&dir, &["--rust-tests"]).contains("Rust tests"));
    }

    #[test]
    fn streaming_and_compact_keep_test_counts() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("sample.rs"),
            "fn production() {}\n#[test]\nfn unit() {}\n",
        )
        .unwrap();
        fs::write(dir.path().join("tokei.toml"), "rust_tests = true\n").unwrap();

        let simple = run(&dir, &["--streaming=simple"]);
        let tests = simple
            .lines()
            .find(|line| line.trim_start().starts_with("Rust tests "))
            .unwrap();
        assert_eq!(
            tests.split_whitespace().rev().take(4).collect::<Vec<_>>(),
            ["0", "0", "2", "2"]
        );

        let streamed: serde_json::Value =
            serde_json::from_str(run(&dir, &["--streaming=json"]).trim()).unwrap();
        assert_eq!(streamed["language"], "Rust");
        assert_eq!(streamed["stats"]["stats"]["blobs"]["Rust tests"]["code"], 2);

        let compact = run(&dir, &["--compact"]);
        assert!(compact.contains("Rust tests"));
        let rust = compact
            .lines()
            .find(|line| line.trim_start().starts_with("Rust "))
            .unwrap();
        assert_eq!(rust.split_whitespace().nth(2), Some("1"));
        let total = compact
            .lines()
            .find(|line| line.trim_start().starts_with("Total "))
            .unwrap();
        assert_eq!(total.split_whitespace().nth(2), Some("3"));
    }

    #[test]
    fn relative_inputs_resolve_test_directories_and_parent_components() {
        let dir = TempDir::new().unwrap();
        let tests_dir = dir.path().join("tests");
        let nested_dir = tests_dir.join("nested");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "fn production() {}\n").unwrap();
        for cwd in [&tests_dir, &nested_dir] {
            fs::write(cwd.join("helper.rs"), "fn helper() {}\nfn fixture() {}\n").unwrap();
            fs::write(cwd.join("tokei.toml"), "rust_tests = true\n").unwrap();
        }

        for (cwd, input, code, is_test) in [
            (&tests_dir, "helper.rs", 2, true),
            (&nested_dir, "helper.rs", 2, true),
            (&tests_dir, ".", 4, true),
            (&tests_dir, "../tests/helper.rs", 2, true),
            (&tests_dir, "../src/lib.rs", 1, false),
            (&nested_dir, "../../src/lib.rs", 1, false),
        ] {
            let output = Command::new(env!("CARGO_BIN_EXE_tokei"))
                .current_dir(cwd)
                .args(["--output=json", "--types=Rust", input])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                json["Total"]["code"],
                code,
                "{} from {}",
                input,
                cwd.display()
            );
            assert_eq!(json["Rust"]["code"], if is_test { 0 } else { code });
            assert_eq!(
                json["Rust"]["children"].get("Rust tests").is_some(),
                is_test
            );
        }
    }

    #[test]
    fn file_rows_include_documentation_inside_tests_in_their_totals() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("sample.rs"),
            "fn production() {}\n#[cfg(test)]\nmod tests {\n    /// A test helper.\n    fn helper() {}\n}\n",
        )
        .unwrap();
        fs::write(dir.path().join("tokei.toml"), "rust_tests = true\n").unwrap();

        let output = run(&dir, &["--files"]);
        let test_rows: Vec<_> = output
            .lines()
            .filter(|line| line.contains("Rust tests"))
            .collect();
        assert_eq!(test_rows.len(), 2, "{output}");
        for row in test_rows {
            assert_eq!(
                row.split_whitespace().rev().take(4).collect::<Vec<_>>(),
                ["0", "1", "4", "5"],
                "{row}"
            );
        }
        let subtotals: Vec<_> = output
            .lines()
            .filter(|line| line.contains("(Total)"))
            .collect();
        assert_eq!(subtotals.len(), 2, "{output}");
        for row in subtotals {
            assert_eq!(
                row.split_whitespace().rev().take(4).collect::<Vec<_>>(),
                ["0", "1", "5", "6"],
                "{row}"
            );
        }
    }
}
