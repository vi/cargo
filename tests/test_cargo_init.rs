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

fn cargo_process(s: &str) -> ProcessBuilder {
    let mut p = process(&cargo_dir().join("cargo"));
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

fn bin_already_exists(explicit: bool, rellocation: &str) {
    let path = paths::root().join("foo");
    fs::create_dir(&path).ok();
    fs::create_dir(&path.join("src")).ok();
    
    let sourcefile_path = path.join(rellocation);
    
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
    bin_already_exists(true, "src/main.rs")
});

test!(bin_already_exists_implicit {
    bin_already_exists(false, "src/main.rs")
});

test!(bin_already_exists_explicit_nosrc {
    bin_already_exists(true, "main.rs")
});

test!(bin_already_exists_implicit_nosrc {
    bin_already_exists(false, "main.rs")
});

test!(bin_already_exists_implicit_namenosrc {
    bin_already_exists(false, "foo.rs")
});

test!(bin_already_exists_implicit_namesrc {
    bin_already_exists(false, "src/foo.rs")
});

test!(confused_by_multiple_lib_files {
    let path = paths::root().join("foo");
    fs::create_dir(&path).ok();
    fs::create_dir(&path.join("src")).ok();
    
    let sourcefile_path1 = path.join("src/lib.rs");
    
    File::create(&sourcefile_path1).unwrap().write_all(br#"
        fn qqq () {
            println!("Hello, world 2!");
        }
    "#).unwrap();
    
    let sourcefile_path2 = path.join("lib.rs");
    
    File::create(&sourcefile_path2).unwrap().write_all(br#"
        fn qqq () {
            println!("Hello, world 3!");
        }
    "#).unwrap();
    
    assert_that(cargo_process("init").arg("--vcs").arg("none")
                                    .env("USER", "foo").cwd(&path),
                execs().with_status(101));
    
    assert_that(&paths::root().join("foo/Cargo.toml"), is_not(existing_file()));
});


test!(multibin_project_name_clash {
    let path = paths::root().join("foo");
    fs::create_dir(&path).ok();
    
    let sourcefile_path1 = path.join("foo.rs");
    
    File::create(&sourcefile_path1).unwrap().write_all(br#"
        fn main () {
            println!("Hello, world 2!");
        }
    "#).unwrap();
    
    let sourcefile_path2 = path.join("main.rs");
    
    File::create(&sourcefile_path2).unwrap().write_all(br#"
        fn main () {
            println!("Hello, world 3!");
        }
    "#).unwrap();
    
    assert_that(cargo_process("init").arg("--vcs").arg("none")
                                    .env("USER", "foo").cwd(&path),
                execs().with_status(101));
                
    assert_that(&paths::root().join("foo/Cargo.toml"), is_not(existing_file()));
    //assert_that(cargo_process("build").cwd(&paths::root().join("foo")),
    //            execs().with_status(0));
    
});

fn lib_already_exists(rellocation: &str) {
    let path = paths::root().join("foo");
    fs::create_dir(&path).ok();
    fs::create_dir(&path.join("src")).ok();
    
    let sourcefile_path = path.join(rellocation);
    
    File::create(&sourcefile_path).unwrap().write_all(br#"
        pub fn qqq() {}
    "#).unwrap();
    
    assert_that(cargo_process("init").arg("--vcs").arg("none")
                                    .env("USER", "foo").cwd(&path),
                execs().with_status(0));

    assert_that(&paths::root().join("foo/Cargo.toml"), existing_file());
    assert_that(&paths::root().join("foo/src/main.rs"), is_not(existing_file()));
    
    // Check that our file is not overwritten
    let mut contents = String::new();
    File::open(&sourcefile_path).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"pub fn qqq() {}"#));

    assert_that(cargo_process("build").cwd(&paths::root().join("foo")),
                execs().with_status(0));
}

test!(lib_already_exists_src {
    lib_already_exists("src/lib.rs")
});

test!(lib_already_exists_nosrc {
    lib_already_exists("lib.rs")
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

test!(git_autodetect {
    fs::create_dir(&paths::root().join(".git")).ok();
    
    assert_that(cargo_process("init")
                                    .env("USER", "foo"),
                execs().with_status(0));
    

    assert_that(&paths::root().join("Cargo.toml"), existing_file());
    assert_that(&paths::root().join("src/lib.rs"), existing_file());
    assert_that(&paths::root().join(".git"), existing_dir());
    assert_that(&paths::root().join(".gitignore"), existing_file());

    assert_that(cargo_process("build").cwd(&paths::root().join(".")),
                execs().with_status(0));
});


test!(mercurial_autodetect {
    fs::create_dir(&paths::root().join(".hg")).ok();
    
    assert_that(cargo_process("init")
                                    .env("USER", "foo"),
                execs().with_status(0));
    

    assert_that(&paths::root().join("Cargo.toml"), existing_file());
    assert_that(&paths::root().join("src/lib.rs"), existing_file());
    assert_that(&paths::root().join(".git"), is_not(existing_dir()));
    assert_that(&paths::root().join(".hgignore"), existing_file());

    assert_that(cargo_process("build").cwd(&paths::root().join(".")),
                execs().with_status(0));
});

test!(gitignore_appended_not_replaced {
    fs::create_dir(&paths::root().join(".git")).ok();
    
    File::create(&paths::root().join(".gitignore")).unwrap().write_all(b"qqqqqq\n").unwrap();
    
    assert_that(cargo_process("init")
                                    .env("USER", "foo"),
                execs().with_status(0));
    

    assert_that(&paths::root().join("Cargo.toml"), existing_file());
    assert_that(&paths::root().join("src/lib.rs"), existing_file());
    assert_that(&paths::root().join(".git"), existing_dir());
    assert_that(&paths::root().join(".gitignore"), existing_file());
    
    let mut contents = String::new();
    File::open(&paths::root().join(".gitignore")).unwrap().read_to_string(&mut contents).unwrap();
    assert!(contents.contains(r#"qqqqq"#));

    assert_that(cargo_process("build").cwd(&paths::root().join(".")),
                execs().with_status(0));
});

test!(with_argument {
    assert_that(cargo_process("init").arg("foo").arg("--vcs").arg("none"),
                execs().with_status(0));
    assert_that(&paths::root().join("foo/Cargo.toml"), existing_file());
});


test!(unknown_flags {
    assert_that(cargo_process("init").arg("foo").arg("--flag"),
                execs().with_status(1)
                       .with_stderr("\
Unknown flag: '--flag'

Usage:
    cargo init [options] [<path>]
    cargo init -h | --help
"));
});

#[cfg(not(windows))]
test!(no_filename {
    assert_that(cargo_process("init").arg("/"),
                execs().with_status(101)
                       .with_stderr("\
cannot auto-detect project name from path \"/\" ; use --name to override
"));
});
