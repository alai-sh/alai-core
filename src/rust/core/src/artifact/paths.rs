// Copyright: (c) 2026, Jordan Borean (@jborean93) <jborean93@gmail.com>
// MIT License (see LICENSE or https://opensource.org/licenses/MIT)

//! Keeping an artifact inside where it's extracted: entry names and symlink targets checked by name, and every path
//! checked on disk as it's made.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

#[cfg(windows)]
use super::{Budget, read_hashed};
use super::{Error, OsContext};

/// The most symlinks followed resolving one path, as Linux's `MAXSYMLINKS`.
const MAX_LINKS: usize = 40;

/// The mode of every directory an artifact is extracted to, masked by the process umask as the kernel applies it.
#[cfg(unix)]
const DIR_MODE: u32 = 0o755;

/// Refuse a path that isn't plainly inside the collection: absolute, or with an empty, `.` or `..` part. On Windows,
/// also one with a part Windows can't hold as it's named.
pub(crate) fn check_path(entry: &str) -> Result<(), Error> {
    let parts = || entry.split('/');
    if entry.starts_with('/')
        || entry.contains('\\')
        || parts().any(|p| matches!(p, "" | "." | ".."))
        || (cfg!(windows) && parts().any(unsafe_on_windows))
    {
        return Err(Error::extract("path", entry));
    }
    Ok(())
}

/// Refuse a symlink whose target, by name, leads outside the collection. On Windows, `\` and `:` are separators and
/// drives too, so a target with either is refused, as is one with a part Windows can't hold.
pub(crate) fn check_link(entry: &str, target: &str) -> Result<(), Error> {
    let mut depth: isize = entry.matches('/').count() as isize;
    let mut outside = target.starts_with('/') || (cfg!(windows) && target.contains(['\\', ':']));
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => depth -= 1,
            _ => {
                depth += 1;
                outside |= cfg!(windows) && unsafe_on_windows(part);
            }
        }
        outside |= depth < 0;
    }
    if outside {
        Err(Error::Artifact("link", entry.to_owned(), target.to_owned()))
    } else {
        Ok(())
    }
}

/// Whether Windows would read a name as something else: a device such as `CON` or `aux.txt`, a name ending in `.` or
/// a space, which it drops, or one with a character it reserves, `:` naming a stream. The devices are those
/// Microsoft's "Naming Files, Paths, and Namespaces" lists, as Python's `os.path.isreserved` has them. `COM¹` to
/// `COM³` count, as ¹, ² and ³ are ISO 8859-1's only superscript digits.
fn unsafe_on_windows(part: &str) -> bool {
    const DEVICES: [&str; 8] = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM", "LPT"];
    if part.ends_with(['.', ' ']) || part.chars().any(|c| c < ' ' || "<>:\"|?*".contains(c)) {
        return true;
    }
    let stem = part
        .split('.')
        .next()
        .unwrap_or(part)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    DEVICES[..6].contains(&stem.as_str())
        || DEVICES[6..].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"))
        })
}

/// A new file at `path`, refusing one that's already there (a symlink included), with `mode` masked by the process
/// umask as the kernel applies it. Windows has no modes, so there `mode` is ignored.
pub(crate) fn create(path: &Path, mode: u32) -> Result<File, Error> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(mode);
    #[cfg(not(unix))]
    let _ = mode;
    options.open(path).at(path)
}

fn dir_builder() -> DirBuilder {
    #[allow(unused_mut)]
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    builder.mode(DIR_MODE);
    builder
}

/// The new directory `path`, failing if it's there already.
pub(crate) fn make_new_dir(path: &Path) -> Result<(), Error> {
    dir_builder().create(path).at(path)
}

/// The directory `path`, made if it isn't one already (an earlier entry's parent is made with it).
pub(crate) fn make_dir(path: &Path) -> Result<(), Error> {
    match dir_builder().create(path) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        result => result.at(path),
    }
}

/// Where an artifact is being extracted, and the symlinks made in it so far.
pub(crate) struct Target {
    dest: PathBuf,
    real: PathBuf,
    pub(crate) links: Vec<(String, PathBuf)>,
    /// Directories already checked whose path, under `dest`, has no symlink in it, so can't later lead anywhere else:
    /// `dest` itself, just made, to begin with.
    plain: HashSet<PathBuf>,
    /// On Windows, the links it wouldn't make, to copy instead, as the entry, its path and its target.
    #[cfg(windows)]
    copies: Vec<(String, PathBuf, String)>,
}

impl Target {
    pub(crate) fn new(dest: &Path) -> Result<Self, Error> {
        let dest = std::path::absolute(dest).at(dest)?;
        let real = realpath(&dest);
        Ok(Target {
            plain: HashSet::from([dest.clone()]),
            dest,
            real,
            links: Vec::new(),
            #[cfg(windows)]
            copies: Vec::new(),
        })
    }

    /// Where `entry` goes, its parent directories made and checked to resolve inside the collection on disk. Unless
    /// the parent's own parent is known to have no symlink, so at most the parent itself is made, and inside, it's
    /// checked before it's made too: through a chain of links leading out, a directory would be made outside.
    pub(crate) fn path(&mut self, entry: &str) -> Result<PathBuf, Error> {
        let path = self.dest.join(entry);
        let parent = path.parent().unwrap_or(&self.dest).to_owned();
        if !self.plain.contains(&parent) {
            if !parent.parent().is_some_and(|above| self.plain.contains(above)) {
                self.check_inside(&parent, entry)?;
            }
            dir_builder().recursive(true).create(&parent).at(&parent)?;
            if self.is_plain(&parent, entry)? {
                self.plain.insert(parent);
            }
        }
        Ok(path)
    }

    /// Whether the directory `parent`, just made for `entry`, has no symlink in its path under `dest`: one `lstat`
    /// when its own parent is known to have none, else resolved in full, and compared with where it would be under
    /// the resolved `dest`, which may itself be reached through a link.
    fn is_plain(&self, parent: &Path, entry: &str) -> Result<bool, Error> {
        if parent.parent().is_some_and(|above| self.plain.contains(above))
            && !fs::symlink_metadata(parent).at(parent)?.file_type().is_symlink()
        {
            return Ok(true);
        }
        let relative = parent.strip_prefix(&self.dest).unwrap_or(parent);
        Ok(self.check_inside(parent, entry)? == self.real.join(relative))
    }

    /// Fail unless `path` resolves inside the collection on disk; return where it resolves to.
    pub(crate) fn check_inside(&self, path: &Path, entry: &str) -> Result<PathBuf, Error> {
        let real = realpath(path);
        if !real.starts_with(&self.real) {
            return Err(Error::extract("escape", entry));
        }
        Ok(real)
    }

    /// Make the symlink `path` to `target`, a link to a directory if `dir`, as Windows needs to know. Where Windows
    /// won't make links, as without Developer Mode, it's copied by `copy_links` instead.
    pub(crate) fn symlink(&mut self, target: &str, path: &Path, entry: &str, dir: bool) -> Result<(), Error> {
        #[cfg(unix)]
        let _ = dir;
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, path);
        #[cfg(windows)]
        let made = {
            // ERROR_PRIVILEGE_NOT_HELD
            const NO_PRIVILEGE: i32 = 1314;
            let target = target.replace('/', "\\");
            let made = if dir {
                std::os::windows::fs::symlink_dir(&target, path)
            } else {
                std::os::windows::fs::symlink_file(&target, path)
            };
            if made.as_ref().is_err_and(|e| e.raw_os_error() == Some(NO_PRIVILEGE)) {
                self.copies.push((entry.to_owned(), path.to_owned(), target));
                return Ok(());
            }
            made
        };
        made.at(path)?;
        self.links.push((entry.to_owned(), path.to_owned()));
        Ok(())
    }

    #[cfg(unix)]
    pub(crate) fn copy_links(&mut self, _budget: &super::Budget) -> Result<(), Error> {
        Ok(())
    }

    /// Copy what each link Windows wouldn't make points to, checked inside the collection and spent from `budget`.
    /// One to a link not copied yet waits for it.
    #[cfg(windows)]
    pub(crate) fn copy_links(&mut self, budget: &Budget) -> Result<(), Error> {
        let mut pending = std::mem::take(&mut self.copies);
        while !pending.is_empty() {
            let source = |path: &Path, target: &str| path.parent().unwrap_or(path).join(target);
            let (ready, waiting): (Vec<_>, Vec<_>) = pending
                .into_iter()
                .partition(|(_, path, target)| fs::metadata(source(path, target)).is_ok());
            if ready.is_empty() {
                // Nothing left that leads anywhere.
                return Err(Error::extract("unreadable", &waiting[0].0));
            }
            for (entry, path, target) in ready {
                let source = source(&path, &target);
                let real = self.check_inside(&source, &entry)?;
                // A directory that holds the link would copy forever.
                if realpath(&path).starts_with(&real) {
                    return Err(Error::extract("unreadable", &entry));
                }
                self.copy(&source, &path, &entry, budget)?;
            }
            pending = waiting;
        }
        Ok(())
    }

    /// Copy the file or directory `source` to the new `path`, for a link Windows won't make.
    #[cfg(windows)]
    fn copy(&mut self, source: &Path, path: &Path, entry: &str, budget: &Budget) -> Result<(), Error> {
        if fs::metadata(source).at(source)?.is_dir() {
            make_new_dir(path)?;
            for item in fs::read_dir(source).at(source)? {
                let item = item.at(source)?;
                let from = item.path();
                self.check_inside(&from, entry)?;
                self.copy(&from, &path.join(item.file_name()), entry, budget)?;
            }
            return Ok(());
        }
        let mut file = File::open(source).at(source)?;
        let mut written = create(path, 0o644)?;
        let mut buffer = vec![0u8; super::BUFFER];
        let _ = read_hashed(&mut file, source, Some((&mut written, path)), &mut buffer, budget)?;
        Ok(())
    }
}

/// The absolute path `path` leads to, following every symlink, as Python's `os.path.realpath` does without
/// `strict`: a part that doesn't exist, or can't be read, is taken as it is.
fn realpath(path: &Path) -> PathBuf {
    let mut resolved = root_of(path);
    // Parts still to resolve, the next last.
    let mut pending: Vec<OsString> = path.components().rev().filter_map(part).collect();
    let mut followed = 0;
    while let Some(next) = pending.pop() {
        if next == ".." {
            resolved.pop();
            continue;
        }
        let candidate = resolved.join(&next);
        let target = match fs::symlink_metadata(&candidate) {
            Ok(meta) if meta.file_type().is_symlink() => fs::read_link(&candidate).ok(),
            _ => None,
        };
        match target {
            Some(target) if followed < MAX_LINKS => {
                followed += 1;
                if target.has_root() {
                    resolved = if target.is_absolute() {
                        root_of(&target)
                    } else {
                        root_of(&resolved)
                    };
                }
                pending.extend(target.components().rev().filter_map(part));
            }
            _ => resolved = candidate,
        }
    }
    resolved
}

/// The root of the absolute path `path`, as `/`, or on Windows its drive or share and root, such as `C:\`.
fn root_of(path: &Path) -> PathBuf {
    path.components()
        .take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
        .collect()
}

fn part(component: Component<'_>) -> Option<OsString> {
    match component {
        Component::Normal(name) => Some(name.to_owned()),
        Component::ParentDir => Some("..".into()),
        Component::RootDir | Component::CurDir | Component::Prefix(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_names() {
        for name in [
            "CON",
            "con.txt",
            "aux.tar.gz",
            "NUL ",
            "COM1",
            "lpt9.py",
            "COM¹",
            "conin$",
            "a.",
            "a ",
            "a:b",
            "a?",
        ] {
            assert!(unsafe_on_windows(name), "{name}");
        }
        for name in [
            "CONSOLE",
            "com",
            "COM10",
            "COM0",
            "LPT0",
            "COM⁴",
            "lpt",
            "a.b",
            "nul_plugin.py",
            "auxiliary.py",
            "é",
        ] {
            assert!(!unsafe_on_windows(name), "{name}");
        }
    }

    #[test]
    fn links_by_name() {
        assert!(check_link("a/b", "../c").is_ok());
        assert!(check_link("a/b", "../../c").is_err());
        assert!(check_link("a", "/etc").is_err());
        assert_eq!(check_link("a", "..\\..\\x").is_err(), cfg!(windows));
        assert_eq!(check_link("a", "c:x").is_err(), cfg!(windows));
        assert_eq!(check_path("dir/con.py").is_err(), cfg!(windows));
    }
}
