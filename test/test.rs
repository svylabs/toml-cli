use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::process::Output;
use std::str;

use tempfile::TempDir;

macro_rules! tomltest {
    ($name:ident, $fun:expr) => {
        #[test]
        fn $name() {
            $fun(TestCaseState::new());
        }
    };
}

macro_rules! tomltest_get_err {
    ($name:ident, $args:expr, $pattern:expr) => {
        tomltest!($name, |mut t: TestCaseState| {
            t.write_file(INPUT);
            t.cmd.args(["get", &t.filename()]).args($args);
            assert!(t.expect_error().contains($pattern));
        });
    };
}

macro_rules! tomltest_get {
    ($name:ident, $args:expr, $expected:expr) => {
        tomltest!($name, |mut t: TestCaseState| {
            t.write_file(INPUT);
            t.cmd.args(["get", &t.filename()]).args($args);
            check_eq($expected, &t.expect_success());
        });
    };
}

macro_rules! tomltest_get1 {
    ($name:ident, $key:expr, $expected:expr) => {
        tomltest!($name, |mut t: TestCaseState| {
            t.write_file(INPUT);
            t.cmd.args(["get", &t.filename(), $key]);
            let expected = format!("{}\n", serde_json::to_string(&$expected).unwrap());
            check_eq(&expected, &t.expect_success());
        });
    };
}

tomltest!(help_if_no_args, |mut t: TestCaseState| {
    assert!(t.expect_error().contains("-h, --help"));
});

const INPUT: &str = r#"
key = "value"
int = 17
bool = true

# this is a TOML comment
bare-Key_1 = "bare"  # another TOML comment
"quoted key‽" = "quoted"
"" = "empty"
dotted.a = "dotted-a"
dotted . b = "dotted-b"

[foo]
x = "foo-x"
y.yy = "foo-yy"
"#;

tomltest_get1!(get_string, "key", "value");
tomltest_get1!(get_int, "int", 17);
tomltest_get1!(get_bool, "bool", true);
// TODO test remaining TOML value types: float, datetime, and aggregates:
//   array, table, inline table, array of tables.

// Test the various TOML key syntax: https://toml.io/en/v1.0.0#keys
tomltest_get1!(get_bare_key, "bare-Key_1", "bare");
tomltest_get1!(get_quoted_key, "\"quoted key‽\"", "quoted");
// tomltest_get1!(get_empty_key, "\"\"", "empty"); // TODO failing
tomltest_get1!(get_dotted_key, "dotted.a", "dotted-a");
tomltest_get1!(get_dotted_spaced_key, "dotted.b", "dotted-b");
tomltest_get1!(get_nested, "foo.x", "foo-x");
tomltest_get1!(get_nested_dotted, "foo.y.yy", "foo-yy");
// TODO test `get` inside arrays and arrays of tables

tomltest_get!(get_string_raw, ["--raw", "key"], "value\n");
// TODO test `get --raw` on non-strings

// TODO test `get --output-toml`

tomltest_get_err!(get_missing, ["nosuchkey"], "panicked"); // TODO should make error better

tomltest!(set_string_existing, |mut t: TestCaseState| {
    let contents = r#"[a]
b = "c"
[x]
y = "z""#;
    t.write_file(contents);
    t.cmd.args(["set", &t.filename(), "x.y", "new"]);
    let expected = r#"[a]
b = "c"
[x]
y = "new"
"#;
    check_eq(expected, &t.expect_success());
});

tomltest!(set_string, |mut t: TestCaseState| {
    let contents = r#"[a]
b = "c"
[x]
y = "z""#;
    t.write_file(contents);
    t.cmd.args(["set", &t.filename(), "x.z", "123"]);
    let expected = r#"[a]
b = "c"
[x]
y = "z"
z = "123"
"#;
    check_eq(expected, &t.expect_success());
});

struct TestCaseState {
    cmd: process::Command,
    #[allow(dead_code)] // We keep the TempDir around to prolong its lifetime
    dir: TempDir,
    filename: PathBuf,
}

impl TestCaseState {
    pub fn new() -> Self {
        let cmd = process::Command::new(get_exec_path());
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let filename = dir.path().join("test.toml");
        TestCaseState { cmd, dir, filename }
    }

    pub fn expect_success(&mut self) -> String {
        let out = self.cmd.output().unwrap();
        if !out.status.success() {
            self.fail(&out, "Command failed!");
        } else if !out.stderr.is_empty() {
            self.fail(&out, "Command printed to stderr despite success");
        }
        String::from_utf8(out.stdout).unwrap()
    }

    pub fn expect_error(&mut self) -> String {
        let out = self.cmd.output().unwrap();
        if out.status.success() {
            self.fail(&out, "Command succeeded; expected failure");
        } else if !out.stdout.is_empty() {
            self.fail(&out, "Command printed to stdout despite failure");
        }
        String::from_utf8(out.stderr).unwrap()
    }

    fn fail(&self, out: &Output, summary: &str) {
        panic!(
            "\n============\
             \n{}\
             \ncmdline: {:?}\
             \nstatus: {}\
             \nstderr: {}\
             \nstdout: {}\
             \n============\n",
            summary,
            self.cmd,
            out.status,
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout),
        )
    }

    pub fn write_file(&self, contents: &str) {
        fs::write(&self.filename, contents).expect("failed to write test fixture");
    }

    pub fn filename(&self) -> String {
        // TODO we don't really need a String here, do we?
        String::from(self.filename.as_os_str().to_str().unwrap())
    }
}

fn get_exec_path() -> PathBuf {
    // TODO is there no cleaner way to get this from Cargo?
    // Also should it really be "debug"?
    let target_dir: PathBuf = env::var_os("CARGO_TARGET_DIR")
        .unwrap_or_else(|| OsString::from("target"))
        .into();
    target_dir.join("debug").join("toml")
}

/// Like `assert_eq!`, but with more-readable output for debugging failed tests.
///
/// In particular, print the strings directly rather than with `{:?}`.
#[rustfmt::skip]
fn check_eq(expected: &str, actual: &str) {
    if expected != actual {
        panic!("
~~~ expected:
{}~~~ got:
{}~~~
", expected, actual);
    }
}
