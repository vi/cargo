use std::env;
use std::fs::{self};
use std::io::prelude::*;
use std::path::Path;

use rustc_serialize::{Decodable, Decoder};

use git2::Config as GitConfig;

use term::color::BLACK;

use util::{GitRepo, HgRepo, CargoResult, human, ChainError, internal};
use util::{Config, paths};

use toml;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VersionControl { Git, Hg, NoVcs }

pub struct NewOptions<'a> {
    pub version_control: Option<VersionControl>,
    pub bin: bool,
    pub path: &'a str,
    pub name: Option<&'a str>,
}

struct SourceFileInformation {
    relative_path: String,
    bin: bool,
}

struct MkOptions<'a> {
    version_control: Option<VersionControl>,
    path: &'a Path,
    name: &'a str,
    source_files: Vec<SourceFileInformation>,
}

impl Decodable for VersionControl {
    fn decode<D: Decoder>(d: &mut D) -> Result<VersionControl, D::Error> {
        Ok(match &try!(d.read_str())[..] {
            "git" => VersionControl::Git,
            "hg" => VersionControl::Hg,
            "none" => VersionControl::NoVcs,
            n => {
                let err = format!("could not decode '{}' as version control", n);
                return Err(d.error(&err));
            }
        })
    }
}

struct CargoNewConfig {
    name: Option<String>,
    email: Option<String>,
    version_control: Option<VersionControl>,
}

fn get_name<'a>(path: &'a Path, opts: &'a NewOptions, config: &Config) -> CargoResult<&'a str> {
    if let Some(name) = opts.name {
        return Ok(name);
    }
    
    let dir_name = try!(path.file_name().and_then(|s| s.to_str()).chain_error(|| {
        human(&format!("cannot create a project with a non-unicode name: {:?}",
                       path.file_name().unwrap()))
    }));
    
    if opts.bin {
        Ok(dir_name)
    } else {
        let new_name = strip_rust_affixes(dir_name);
        if new_name != dir_name {
            let message = format!(
                "note: package will be named `{}`; use --name to override",
                new_name);
            try!(config.shell().say(&message, BLACK));
        }
        Ok(new_name)
    }
}

fn check_name(name: &str) -> CargoResult<()> {
    for c in name.chars() {
        if c.is_alphanumeric() { continue }
        if c == '_' || c == '-' { continue }
        return Err(human(&format!("Invalid character `{}` in crate name: `{}`",
                                  c, name)));
    }
    Ok(())
}

fn detect_source_paths_and_types<'a : 'b, 'b>(project_path : &'a Path, 
                                            project_name: &'a str, 
                                            detected_files: &'b mut Vec<SourceFileInformation>) -> CargoResult<()> {
    let path = project_path;
    let name = project_name;
    
    enum H {
        Bin,
        Lib,
        Detect,
    }
    
    struct Test {
        proposed_path: String,
        handling: H,
    }
        
    let tests = vec![
        Test { proposed_path: format!("src/main.rs"),     handling: H::Bin },
        Test { proposed_path: format!("main.rs"),         handling: H::Bin },
        Test { proposed_path: format!("src/{}.rs", name), handling: H::Detect },
        Test { proposed_path: format!("{}.rs", name),     handling: H::Detect },
        Test { proposed_path: format!("src/lib.rs"),      handling: H::Lib },
        Test { proposed_path: format!("lib.rs"),          handling: H::Lib },
    ];
    
    for i in tests {
        let pp = i.proposed_path;
        if ! paths::file_already_exists(&path.join(pp.clone())) {
            continue;
        }
        let sfi = match i.handling {
            H::Bin => 
                SourceFileInformation { relative_path: pp, bin: true },
            H::Lib => 
                SourceFileInformation { relative_path: pp, bin: false },
            H::Detect => {
                let content = try!(paths::read(&path.join(pp.clone())));
                let isbin = content.contains("fn main");
                SourceFileInformation { relative_path: pp, bin: isbin }
            },
        };
        detected_files.push(sfi);
    }
    
    Ok(())
}

fn plan_new_source_file(bin: bool) -> SourceFileInformation {
    if bin {
        SourceFileInformation { 
             relative_path: String::from("src/main.rs"),
             bin: true,
        }
    } else {
        SourceFileInformation {
             relative_path: String::from("src/lib.rs"),
             bin: false,
        }
    }
}

pub fn new(opts: NewOptions, config: &Config) -> CargoResult<()> {
    let path = config.cwd().join(opts.path);
    if fs::metadata(&path).is_ok() {
        return Err(human(format!("Destination `{}` already exists",
                                 path.display())))
    }
    
    let name = try!(get_name(&path, &opts, config));
    try!(check_name(name));

    let mkopts = MkOptions {
        version_control: opts.version_control,
        path: &path,
        name: name,
        source_files: vec![plan_new_source_file(opts.bin)],
    };
    
    mk(config, &mkopts).chain_error(|| {
        human(format!("Failed to create project `{}` at `{}`",
                      name, path.display()))
    })
}

pub fn init(opts: NewOptions, config: &Config) -> CargoResult<()> {
    assert_eq!(opts.path, ".");
    let path = config.cwd().join(opts.path);
    
    let cargotoml_path = path.join("Cargo.toml");
    if fs::metadata(&cargotoml_path).is_ok() {
        return Err(human(format!("Destination `{}` already exists",
                                 cargotoml_path.display())))
    }
    
    let name = try!(get_name(&path, &opts, config));
    try!(check_name(name));
    
    let mut src_paths_types = vec![];
    
    try!(detect_source_paths_and_types(&path, name, &mut src_paths_types));
    
    if src_paths_types.len() == 0 {
        src_paths_types.push(plan_new_source_file(opts.bin));
    } else {
        // --bin option may be ignored if lib.rs or src/lib.rs present
        // Maybe when doing `cargo init --bin` inside a library project stub,
        // user may mean "initialize for library, but also add binary target"
    }
    
    let mut version_control = opts.version_control;
    
    if version_control == None {
        let mut num_detected_vsces = 0;
        
        if fs::metadata(&path.join(".git")).is_ok() {
            version_control = Some(VersionControl::Git);
            num_detected_vsces += 1;
        }
        
        if fs::metadata(&path.join(".hg")).is_ok() {
            version_control = Some(VersionControl::Hg);
            num_detected_vsces += 1;
        }
        
        // if none exists, maybe create git, like in `cargo new`
        
        if num_detected_vsces > 1 {
            return Err(human("Both .git and .hg exist. I don't know what to choose."));
        }
    }
    
    let mkopts = MkOptions {
        version_control: version_control,
        path: &path,
        name: name,
        source_files: src_paths_types,
    };
    
    mk(config, &mkopts).chain_error(|| {
        human(format!("Failed to create project `{}` at `{}`",
                      name, path.display()))
    })
}

fn strip_rust_affixes(name: &str) -> &str {
    for &prefix in &["rust-", "rust_", "rs-", "rs_"] {
        if name.starts_with(prefix) {
            return &name[prefix.len()..];
        }
    }
    for &suffix in &["-rust", "_rust", "-rs", "_rs"] {
        if name.ends_with(suffix) {
            return &name[..name.len()-suffix.len()];
        }
    }
    name
}

fn existing_vcs_repo(path: &Path) -> bool {
    GitRepo::discover(path).is_ok() || HgRepo::discover(path).is_ok()
}

fn mk(config: &Config, opts: &MkOptions) -> CargoResult<()> {
    let path = opts.path;
    let name = opts.name;
    let cfg = try!(global_config(config));
    let mut ignore = "target\n".to_string();
    let in_existing_vcs_repo = existing_vcs_repo(path.parent().unwrap());
    ignore.push_str("Cargo.lock\n");

    let vcs = match (opts.version_control, cfg.version_control, in_existing_vcs_repo) {
        (None, None, false) => VersionControl::Git,
        (None, Some(option), false) => option,
        (Some(option), _, _) => option,
        (_, _, true) => VersionControl::NoVcs,
    };

    match vcs {
        VersionControl::Git => {
            if ! fs::metadata(&path.join(".git")).is_ok() {
                try!(GitRepo::init(path));
            }
            try!(paths::append(&path.join(".gitignore"), ignore.as_bytes()));
        },
        VersionControl::Hg => {
            if ! paths::directory_already_exists(&path.join(".hg")) {
                try!(HgRepo::init(path));
            }
            try!(paths::append(&path.join(".hgignore"), ignore.as_bytes()));
        },
        VersionControl::NoVcs => {
            try!(fs::create_dir_all(path));
        },
    };

    let (author_name, email) = try!(discover_author());
    // Hoo boy, sure glad we've got exhaustivenes checking behind us.
    let author = match (cfg.name, cfg.email, author_name, email) {
        (Some(name), Some(email), _, _) |
        (Some(name), None, _, Some(email)) |
        (None, Some(email), name, _) |
        (None, None, name, Some(email)) => format!("{} <{}>", name, email),
        (Some(name), None, _, None) |
        (None, None, name, None) => name,
    };
    
    let mut cargotoml_path_specifier = String::new();
    
    let mut there_was_already_a_lib = false;
    let mut previous_lib_relpath = String::new();
    
    // Calculare what [lib] and [[bin]]s do we need to append to Cargo.toml
    
    let explicit_better_than_implicit = opts.source_files.len() > 1;
    
    let mut bin_counter = 1;
    
    for i in &opts.source_files {
        if i.bin {
            if i.relative_path != "src/main.rs" || explicit_better_than_implicit {
                let name_appendix =
                    if bin_counter == 1 { format!(r"") }
                    else { format!(r"{}", bin_counter)  };
                // the user is expected to rename "myproject2" into something
                // more meaningful after initialization.
                        
                cargotoml_path_specifier.push_str(&format!(r#"
[[bin]]
name = "{}{}"
path = {}
"#, name, name_appendix, toml::Value::String(i.relative_path.clone())));
                bin_counter += 1;
            }
        } else {
            if i.relative_path != "src/lib.rs" || explicit_better_than_implicit {
                cargotoml_path_specifier.push_str(&format!(r#"
[lib]
name = "{}"
path = {}
"#, name, toml::Value::String(i.relative_path.clone())));
            }
            if there_was_already_a_lib {
                return Err(human(format!(
                    "I confused by multiple library source files. There are both {} and {}...",
                    previous_lib_relpath, i.relative_path)));
            } else {
                there_was_already_a_lib = true;
                previous_lib_relpath = i.relative_path.clone();
            }
        }
    }

    // Create Cargo.toml file with necessary [lib] and [[bin]] sections, if needed

    try!(paths::write(&path.join("Cargo.toml"), format!(
r#"[package]
name = "{}"
version = "0.1.0"
authors = [{}]
{}"#, name, toml::Value::String(author), cargotoml_path_specifier).as_bytes()));

    // Create all specified source files 
    // (with respective parent directories) 
    // if they are don't exist

    for i in &opts.source_files {
        let path_of_source_file = path.join(i.relative_path.clone());
        
        if let Some(src_dir) = path_of_source_file.parent() {
            try!(fs::create_dir_all(src_dir));
        }
    
        let default_file_content : &[u8] = if i.bin {
            b"\
fn main() {
    println!(\"Hello, world!\");
}
"
        } else {
            b"\
#[test]
fn it_works() {
}
"
        };
    
        if ! paths::file_already_exists(&path_of_source_file) {
            return paths::write(&path_of_source_file, default_file_content)
        }
    }

    Ok(())
}

fn discover_author() -> CargoResult<(String, Option<String>)> {
    let git_config = GitConfig::open_default().ok();
    let git_config = git_config.as_ref();
    let name = git_config.and_then(|g| g.get_string("user.name").ok())
                         .map(|s| s.to_string())
                         .or_else(|| env::var("USER").ok())      // unix
                         .or_else(|| env::var("USERNAME").ok()); // windows
    let name = match name {
        Some(name) => name,
        None => {
            let username_var = if cfg!(windows) {"USERNAME"} else {"USER"};
            return Err(human(format!("could not determine the current \
                                      user, please set ${}", username_var)))
        }
    };
    let email = git_config.and_then(|g| g.get_string("user.email").ok())
                          .or_else(|| env::var("EMAIL").ok());

    let name = name.trim().to_string();
    let email = email.map(|s| s.trim().to_string());

    Ok((name, email))
}

fn global_config(config: &Config) -> CargoResult<CargoNewConfig> {
    let name = try!(config.get_string("cargo-new.name")).map(|s| s.0);
    let email = try!(config.get_string("cargo-new.email")).map(|s| s.0);
    let vcs = try!(config.get_string("cargo-new.vcs"));

    let vcs = match vcs.as_ref().map(|p| (&p.0[..], &p.1)) {
        Some(("git", _)) => Some(VersionControl::Git),
        Some(("hg", _)) => Some(VersionControl::Hg),
        Some(("none", _)) => Some(VersionControl::NoVcs),
        Some((s, p)) => {
            return Err(internal(format!("invalid configuration for key \
                                         `cargo-new.vcs`, unknown vcs `{}` \
                                         (found in {:?})", s, p)))
        }
        None => None
    };
    Ok(CargoNewConfig {
        name: name,
        email: email,
        version_control: vcs,
    })
}

#[cfg(test)]
mod tests {
    use super::strip_rust_affixes;

    #[test]
    fn affixes_stripped() {
        assert_eq!(strip_rust_affixes("rust-foo"), "foo");
        assert_eq!(strip_rust_affixes("foo-rs"), "foo");
        assert_eq!(strip_rust_affixes("rs_foo"), "foo");
        // Only one affix is stripped
        assert_eq!(strip_rust_affixes("rs-foo-rs"), "foo-rs");
        assert_eq!(strip_rust_affixes("foo-rs-rs"), "foo-rs");
        // It shouldn't touch the middle
        assert_eq!(strip_rust_affixes("some-rust-crate"), "some-rust-crate");
    }
}
