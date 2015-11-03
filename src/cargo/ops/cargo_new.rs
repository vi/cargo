use std::env;
use std::fs::{self, File};
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

#[derive(Clone, Debug, PartialEq)]
pub struct NewOptions<'a> {
    pub version_control: Option<VersionControl>,
    pub bin: bool,
    pub path: &'a str,
    pub name: Option<&'a str>,
    pub sourcefile_relative_path : Option<String>,
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
    match opts.name {
        Some(name) => Ok(name),
        None => {
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

fn detect_source_path_and_type<'a : 'b, 'b>(project_path : &Path, 
                                            project_name: &str, 
                                            opts2: &'b mut NewOptions) -> CargoResult<()> {
    let path = project_path;
    let name = project_name;
    
    let mut found_source_files = 0;
    
    if paths::file_already_exists(&path.join("src/main.rs")) {
        opts2.bin = true;
        opts2.sourcefile_relative_path = Some(String::from("src/main.rs"));
        found_source_files += 1;
    }
    if paths::file_already_exists(&path.join("main.rs")) {
        opts2.bin = true;
        opts2.sourcefile_relative_path = Some(String::from("main.rs"));
        found_source_files += 1;
    }
    fn autodetect_bin_file(p: &Path) -> CargoResult<bool> {
        let mut content = String::new();
        try!(File::open(p).and_then(|mut x| x.read_to_string(&mut content)));
        Ok(content.contains("fn main"))
    }
    if paths::file_already_exists(&path.join(format!("{}.rs", name))) {
        if opts2.bin {
            // OK
        } else {
            opts2.bin = try!(autodetect_bin_file(&path.join(format!("{}.rs", name))));
            
        }
        opts2.sourcefile_relative_path = Some(format!("{}.rs", name));
        found_source_files += 1;
    }
    
    if paths::file_already_exists(&path.join(format!("src/{}.rs", name))) {
        if opts2.bin {
            // OK
        } else {
            opts2.bin = try!(autodetect_bin_file(&path.join(format!("src/{}.rs", name))));
            
        }
        opts2.sourcefile_relative_path = Some(format!("src/{}.rs", name));
        found_source_files += 1;
    }
    
    if (!opts2.bin) && paths::file_already_exists(&path.join("src/lib.rs")) {
        found_source_files += 1;
        opts2.sourcefile_relative_path = Some(String::from("src/lib.rs"));
    }
    if (!opts2.bin) && paths::file_already_exists(&path.join("lib.rs")) {
        found_source_files += 1;
        opts2.sourcefile_relative_path = Some(String::from("lib.rs"));
    }
    
    if found_source_files > 1 {
        return Err(human(r"There are multiple eligible source files for `cargo init`.
I don't know which Cargo.toml template to use."));
    }
    Ok(())
}

pub fn new(opts: NewOptions, config: &Config) -> CargoResult<()> {
    let path = config.cwd().join(opts.path);
    if fs::metadata(&path).is_ok() {
        return Err(human(format!("Destination `{}` already exists",
                                 path.display())))
    }
    
    let name = try!(get_name(&path, &opts, config));
    try!(check_name(name));
    
    mk(config, &path, name, &opts).chain_error(|| {
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
    
    let mut opts2 = opts.clone();
    
    if opts2.sourcefile_relative_path == None {
        try!(detect_source_path_and_type(&path, name, &mut opts2));
    }
    
    if opts2.version_control == None {
        let mut num_detected_vsces = 0;
        
        if fs::metadata(&path.join(".git")).is_ok() {
            opts2.version_control = Some(VersionControl::Git);
            num_detected_vsces += 1;
        }
        
        if fs::metadata(&path.join(".hg")).is_ok() {
            opts2.version_control = Some(VersionControl::Hg);
            num_detected_vsces += 1;
        }
        
        // if none exists, maybe create git, like in `cargo new`
        
        if num_detected_vsces > 1 {
            return Err(human("Both .git and .hg exist. I don't know what to choose."));
        }
    }
    
    mk(config, &path, name, &opts2).chain_error(|| {
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

fn mk(config: &Config, path: &Path, name: &str,
      opts: &NewOptions) -> CargoResult<()> {
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
    
    let (path_of_source_file, cargotoml_path_specifier) = match opts.sourcefile_relative_path {
        None => if opts.bin {
                    (path.join("src/main.rs"), String::new())
                } else {
                    (path.join("src/lib.rs"), String::new())
                },
        Some(ref src_rel_path) => {
            let specifier = if opts.bin {
                if src_rel_path == "src/main.rs" {
                    String::new()
                } else {
                    format!(r#"
[[bin]]
name = "{}"
path = {}
"#, name, toml::Value::String(src_rel_path.clone()))
                }
            } else {
                if src_rel_path == "src/lib.rs" {
                    String::new()
                } else {
                    format!(r#"
[lib]
name = "{}"
path = {}
"#, name, toml::Value::String(src_rel_path.clone()))
                }
            };
            (path.join(src_rel_path), specifier)
        },
    };

    try!(paths::write(&path.join("Cargo.toml"), format!(
r#"[package]
name = "{}"
version = "0.1.0"
authors = [{}]
{}"#, name, toml::Value::String(author), cargotoml_path_specifier).as_bytes()));

    if let Some(src_dir) = path_of_source_file.parent() {
        try!(fs::create_dir_all(src_dir));
    }

    if opts.bin {
        try!(paths::write_if_not_exists(&path_of_source_file, b"\
fn main() {
    println!(\"Hello, world!\");
}
"));
    } else {
        try!(paths::write_if_not_exists(&path_of_source_file, b"\
#[test]
fn it_works() {
}
"));
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
