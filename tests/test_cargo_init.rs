#![allow(unused_imports)]
use std::fs::{self, File};
use std::io::prelude::*;
use std::env;
use tempdir::TempDir;

use support::{execs, paths, cargo_dir};
use support::paths::CargoPathExt;
use hamcrest::{assert_that, existing_file, existing_dir, is_not};

use cargo::util::{process, ProcessBuilder};

fn setup() {
}

fn my_process(s: &str) -> ProcessBuilder {
    let mut p = process(s).unwrap();
    p.cwd(&paths::root()).env("HOME", &paths::home());
    return p;
}

fn cargo_process(s: &str) -> ProcessBuilder {
    let mut p = process(&cargo_dir().join("cargo")).unwrap();
    p.arg(s).cwd(&paths::root()).env("HOME", &paths::home());
    return p;
}

test!(simple_lib {
    assert_that(cargo_process("init").arg("--vcs").arg("none")
                                    .env("USER", "foo"),
                execs().with_status(0));

    assert_that(&paths::root().join("Cargo.toml"), existing_file());
    assert_that(&paths::root().join("src/lib.rs"), existing_file());
    assert_that(&paths::root().join(".gitignore"), is_not(existing_file()));

    assert_that(cargo_process("build").cwd(&paths::root().join(".")),
                execs().with_status(0));
});

test!(simple_bin {
    let path = paths::root().join("foo");
    fs::create_dir(&path).ok();
    assert_that(cargo_process("init").arg("--bin").arg("--vcs").arg("none")
                                    .env("USER", "foo").cwd(&path),
                execs().with_status(0));

    assert_that(&paths::root().join("foo/Cargo.toml"), existing_file());
    assert_that(&paths::root().join("foo/src/main.rs"), existing_file());

    assert_that(cargo_process("build").cwd(&paths::root().join("foo")),
                execs().with_status(0));
    assert_that(&paths::root().join(&format!("foo/target/debug/foo{}",
                                             env::consts::EXE_SUFFIX)),
                existing_file());
});

fn bin_already_exists(explicit: bool) {
    let path = paths::root().join("foo");
    fs::create_dir(&path).ok();
    fs::create_dir(&path.join("src")).ok();
    
    let sourcefile_path = path.join("src/main.rs");
    
    File::create(&sourcefile_path).unwrap().write_all(br#"
        fn main() {
            println!("Hello, world 2!");
        }
    "#).unwrap();
    
    if explicit {
        assert_that(cargo_process("init").arg("--bin").arg("--vcs").arg("none")
                                        .env("USER", "foo").cwd(&path),
                    execs().with_status(0));
    } else {
        assert_that(cargo_process("init").arg("--vcs").arg("none")
                                        .env("USER", "foo").cwd(&path),
                    execs().with_status(0));
    }

    assert_that(&paths::root().join("foo/Cargo.toml"), existing_file());
    assert_that(&paths::root().join("foo/src/lib.rs"), is_not(existing_file()));
    
    // Check that our file is not overwritten
    let mut contents = String::new();
    File::open(&sourcefile_path).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"Hello, world 2!"#));

    assert_that(cargo_process("build").cwd(&paths::root().join("foo")),
                execs().with_status(0));
    assert_that(&paths::root().join(&format!("foo/target/debug/foo{}",
                                             env::consts::EXE_SUFFIX)),
                existing_file());
}

test!(bin_already_exists_explicit {
    bin_already_exists(true)
});

test!(bin_already_exists_implicit {
    bin_already_exists(false)
});

test!(simple_git {
    assert_that(cargo_process("init").arg("--vcs").arg("git")
                                    .env("USER", "foo"),
                execs().with_status(0));

    assert_that(&paths::root().join("Cargo.toml"), existing_file());
    assert_that(&paths::root().join("src/lib.rs"), existing_file());
    assert_that(&paths::root().join(".git"), existing_dir());
    assert_that(&paths::root().join(".gitignore"), existing_file());

    assert_that(cargo_process("build").cwd(&paths::root().join(".")),
                execs().with_status(0));
});

test!(with_argument {
    assert_that(cargo_process("init").arg("foo"),
                execs().with_status(1)
                       .with_stderr("\
Invalid arguments.

Usage:
    cargo init [options]
    cargo init -h | --help
"));
});


test!(unknown_flags {
    assert_that(cargo_process("init").arg("foo").arg("--flag"),
                execs().with_status(1)
                       .with_stderr("\
Unknown flag: '--flag'

Usage:
    cargo init [options]
    cargo init -h | --help
"));
});
