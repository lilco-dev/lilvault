use crate::{
    MAX_FILE_BYTES,
    error::{self, Error},
};
use rustix::fs::{self, AtFlags, CWD, Dir, FileType, Mode, OFlags, RenameFlags, ResolveFlags};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const RESOLVE: ResolveFlags = ResolveFlags::BENEATH
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_MAGICLINKS)
    .union(ResolveFlags::NO_XDEV);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct Vault {
    root: File,
    path: PathBuf,
    writes: Mutex<()>,
}

#[derive(Clone)]
pub enum Condition {
    Create,
    Match(String),
}

impl Condition {
    fn check(&self, revision: &Option<String>) -> Result<(), Error> {
        match (self, revision) {
            (Self::Create, None) => Ok(()),
            (Self::Match(expected), Some(actual)) if expected == actual => Ok(()),
            _ => Err(Error::PreconditionFailed),
        }
    }
}

#[derive(Serialize)]
pub struct Note {
    pub content: String,
    pub revision: String,
}

#[derive(Serialize)]
pub struct Entry {
    pub path: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub size: u64,
}

pub fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn valid_revision(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn visible(component: &str) -> bool {
    !component.is_empty()
        && !component.starts_with('.')
        && !component.contains(['%', '\\'])
        && !component.chars().any(char::is_control)
}

pub fn validate_path(path: &str, directory: bool) -> Result<(), Error> {
    if directory && path.is_empty() {
        return Ok(());
    }
    if path.len() > 1024 || !path.split('/').all(visible) || (!directory && !path.ends_with(".md"))
    {
        return Err(Error::BadPath);
    }
    Ok(())
}

fn open_beneath(dir: &File, path: &str, directory: bool) -> Result<File, Error> {
    let flags = OFlags::RDONLY
        | OFlags::CLOEXEC
        | OFlags::NONBLOCK
        | if directory {
            OFlags::DIRECTORY
        } else {
            OFlags::empty()
        };
    fs::openat2(dir, path, flags, Mode::empty(), RESOLVE)
        .map(File::from)
        .map_err(|e| error::io(e.into()))
}

fn read_at(dir: &File, name: &str) -> Result<Note, Error> {
    let file = open_beneath(dir, name, false)?;
    let meta = file.metadata().map_err(error::io)?;
    // Hard links must not alias notes or files outside a configured vault.
    if !meta.is_file() || meta.nlink() != 1 {
        return Err(Error::BadPath);
    }
    if meta.len() > MAX_FILE_BYTES as u64 {
        return Err(Error::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(error::io)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Error::TooLarge);
    }
    let revision = revision(&bytes);
    let content = String::from_utf8(bytes).map_err(|_| Error::BadRequest)?;
    Ok(Note { content, revision })
}

fn current_revision(dir: &File, name: &str) -> Result<Option<String>, Error> {
    match read_at(dir, name) {
        Ok(note) => Ok(Some(note.revision)),
        Err(Error::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

// Always remove abandoned temporary files, including revision-check failures.
struct Pending<'a> {
    dir: &'a File,
    name: String,
}
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        let _ = fs::unlinkat(self.dir, self.name.as_str(), AtFlags::empty());
    }
}

impl Vault {
    pub fn open(path: &Path) -> Result<Self, Error> {
        if !path.is_absolute() {
            return Err(Error::BadPath);
        }
        let root = fs::openat2(
            CWD,
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        )
        .map(File::from)
        .map_err(|e| error::io(e.into()))?;
        // Probe openat2 flags now rather than falling back to unsafe path operations.
        open_beneath(&root, ".", true)?;
        Ok(Self {
            root,
            path: path.to_path_buf(),
            writes: Mutex::new(()),
        })
    }

    pub(crate) fn identity(&self) -> std::io::Result<(u64, u64)> {
        let metadata = self.root.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    pub fn available(&self) -> bool {
        let Ok(opened) = self.root.metadata() else {
            return false;
        };
        let Ok(current) = std::fs::symlink_metadata(&self.path) else {
            return false;
        };
        current.is_dir() && current.dev() == opened.dev() && current.ino() == opened.ino()
    }

    fn check_available(&self) -> Result<(), Error> {
        if self.available() {
            Ok(())
        } else {
            Err(Error::Unavailable)
        }
    }

    pub fn read(&self, path: &str) -> Result<Note, Error> {
        validate_path(path, false)?;
        self.check_available()?;
        read_at(&self.root, path)
    }

    pub fn list(&self, path: &str) -> Result<Vec<Entry>, Error> {
        validate_path(path, true)?;
        self.check_available()?;
        let dir = open_beneath(&self.root, if path.is_empty() { "." } else { path }, true)?;
        let mut entries = Dir::read_from(&dir).map_err(|e| error::io(e.into()))?;
        let mut result = Vec::new();
        let mut scanned = 0;
        while let Some(entry) = entries.read() {
            scanned += 1;
            if scanned > 10_000 {
                return Err(Error::TooLarge);
            }
            let entry = entry.map_err(|e| error::io(e.into()))?;
            let Ok(name) = entry.file_name().to_str() else {
                continue;
            };
            if !visible(name) {
                continue;
            }
            let stat = match fs::statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => stat,
                Err(rustix::io::Errno::NOENT) => continue, // external sync removed it
                Err(error) => return Err(error::io(error.into())),
            };
            let kind = match FileType::from_raw_mode(stat.st_mode) {
                FileType::Directory => "directory",
                FileType::RegularFile if name.ends_with(".md") && stat.st_nlink == 1 => "file",
                _ => continue,
            };
            let child = if path.is_empty() {
                name.to_owned()
            } else {
                format!("{path}/{name}")
            };
            result.push(Entry {
                path: child,
                kind,
                size: if kind == "file" {
                    stat.st_size.max(0) as u64
                } else {
                    0
                },
            });
        }
        result.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(result)
    }

    pub fn write(&self, path: &str, content: &str, condition: &Condition) -> Result<String, Error> {
        self.write_inner(path, content, condition, || {})
    }

    fn write_inner(
        &self,
        path: &str,
        content: &str,
        condition: &Condition,
        before_commit: impl FnOnce(),
    ) -> Result<String, Error> {
        validate_path(path, false)?;
        if content.len() > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
        // A per-vault lock is intentionally simpler than a growing per-path lock table.
        let _guard = self.writes.lock().map_err(|_| Error::Unavailable)?;
        self.check_available()?;
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        let dir = open_beneath(
            &self.root,
            if parent.is_empty() { "." } else { parent },
            true,
        )?;
        condition.check(&current_revision(&dir, name)?)?;
        let mut temp = None;
        for _ in 0..32 {
            let name = format!(
                ".lilvault-{}-{}.tmp",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            );
            match fs::openat(
                &dir,
                name.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => {
                    temp = Some((Pending { dir: &dir, name }, File::from(fd)));
                    break;
                }
                Err(rustix::io::Errno::EXIST) => continue,
                Err(error) => return Err(error::io(error.into())),
            }
        }
        let (pending, mut file) = temp.ok_or(Error::Unavailable)?;
        file.write_all(content.as_bytes()).map_err(error::io)?;
        file.sync_all().map_err(error::io)?;
        before_commit();
        // Headless bypasses this lock. Narrow, but cannot eliminate, the external-writer race.
        condition.check(&current_revision(&dir, name)?)?;
        self.check_available()?;
        match condition {
            Condition::Create => fs::renameat_with(
                &dir,
                pending.name.as_str(),
                &dir,
                name,
                RenameFlags::NOREPLACE,
            ),
            Condition::Match(_) => fs::renameat(&dir, pending.name.as_str(), &dir, name),
        }
        .map_err(|error| {
            if error == rustix::io::Errno::EXIST {
                Error::PreconditionFailed
            } else {
                error::io(error.into())
            }
        })?;
        dir.sync_all().map_err(error::io)?;
        Ok(revision(content.as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observes_external_write_before_commit_and_cleans_temp() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("note.md"), "original").unwrap();
        let vault = Vault::open(root.path()).unwrap();
        let condition = Condition::Match(revision(b"original"));
        let result = vault.write_inner("note.md", "gateway", &condition, || {
            std::fs::write(root.path().join("note.md"), "external").unwrap();
        });
        assert!(matches!(result, Err(Error::PreconditionFailed)));
        assert_eq!(vault.read("note.md").unwrap().content, "external");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn external_creation_is_not_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let vault = Vault::open(root.path()).unwrap();
        let result = vault.write_inner("note.md", "gateway", &Condition::Create, || {
            std::fs::write(root.path().join("note.md"), "external").unwrap();
        });
        assert!(matches!(result, Err(Error::PreconditionFailed)));
        assert_eq!(
            std::fs::read_to_string(root.path().join("note.md")).unwrap(),
            "external"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn directory_symlink_swaps_cannot_redirect_reads_or_writes() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent");
        let parked = root.path().join("parked");
        std::fs::create_dir(&parent).unwrap();
        std::fs::write(parent.join("note.md"), "inside").unwrap();
        std::fs::write(outside.path().join("note.md"), "outside").unwrap();
        let vault = Vault::open(root.path()).unwrap();
        assert_eq!(vault.read("parent/note.md").unwrap().content, "inside");
        let outside_path = outside.path().to_owned();
        let swapper = std::thread::spawn(move || {
            for _ in 0..400 {
                std::fs::rename(&parent, &parked).unwrap();
                symlink(&outside_path, &parent).unwrap();
                std::thread::yield_now();
                std::fs::remove_file(&parent).unwrap();
                std::fs::rename(&parked, &parent).unwrap();
            }
        });
        for _ in 0..200 {
            if let Ok(note) = vault.read("parent/note.md") {
                assert_ne!(note.content, "outside");
                let _ = vault.write(
                    "parent/note.md",
                    "gateway",
                    &Condition::Match(note.revision),
                );
            }
        }
        swapper.join().unwrap();
        assert_eq!(
            std::fs::read_to_string(outside.path().join("note.md")).unwrap(),
            "outside"
        );
        assert!(
            !std::fs::read_dir(root.path().join("parent"))
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".lilvault-"))
        );
    }
}
