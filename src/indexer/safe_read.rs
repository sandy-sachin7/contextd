#[cfg(not(any(unix, windows)))]
use anyhow::anyhow;
use anyhow::{bail, Context, Result};
use std::fs::File;
use std::path::{Component, Path, PathBuf};

/// Open a regular file below one of the configured roots without following links.
pub fn open_file_under_roots(path: &Path, roots: &[PathBuf]) -> Result<File> {
    for root in roots {
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            continue;
        }

        let file = open_beneath(root, relative)?;
        let metadata = file.metadata().context("read opened file metadata")?;
        if !metadata.is_file() {
            bail!("opened path is not a regular file");
        }
        return Ok(file);
    }
    bail!("path is not beneath a configured watch root")
}

#[cfg(unix)]
fn open_beneath(root: &Path, relative: &Path) -> Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;

    fn cstring(value: &std::ffi::OsStr) -> Result<CString> {
        CString::new(value.as_bytes()).context("path component contains NUL")
    }

    fn open_at(dir: &OwnedFd, name: &std::ffi::OsStr, flags: libc::c_int) -> Result<OwnedFd> {
        let name = cstring(name)?;
        // SAFETY: `dir` is an open directory descriptor and `name` is NUL-terminated.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags, 0) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error())
                .context("open path component without following links");
        }
        // SAFETY: openat returned a new descriptor owned by this function.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    let root_name = cstring(root.as_os_str())?;
    // SAFETY: `root_name` is a valid NUL-terminated path.
    let root_fd = unsafe {
        libc::open(
            root_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if root_fd < 0 {
        return Err(std::io::Error::last_os_error())
            .context("open watch root without following links");
    }
    // SAFETY: open returned a new descriptor owned by this function.
    let mut directory = unsafe { OwnedFd::from_raw_fd(root_fd) };

    let components: Vec<_> = relative.components().collect();
    let Some((last, parents)) = components.split_last() else {
        bail!("relative path has no components");
    };
    for component in parents {
        let Component::Normal(name) = component else {
            bail!("invalid path component beneath watch root");
        };
        directory = open_at(
            &directory,
            name,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )?;
    }

    let Component::Normal(name) = last else {
        bail!("invalid final path component");
    };
    let file_fd = open_at(
        &directory,
        name,
        libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
    )?;
    Ok(File::from(file_fd))
}

#[cfg(windows)]
fn open_beneath(root: &Path, relative: &Path) -> Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFinalPathNameByHandleW, FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED,
    };

    let expected = normalize_windows_path(&root.join(relative));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(root.join(relative))
        .context("open path without following final reparse point")?;
    let attributes = file.metadata()?.file_attributes();
    if attributes & 0x400 != 0 {
        bail!("opened path is a reparse point");
    }

    let handle = std::os::windows::io::AsRawHandle::as_raw_handle(&file);
    let mut buffer = vec![0u16; 32768];
    // SAFETY: The handle belongs to `file`; the buffer has the declared capacity.
    let count = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED,
        )
    };
    if count == 0 || count as usize >= buffer.len() {
        return Err(std::io::Error::last_os_error()).context("resolve opened file handle path");
    }
    buffer.truncate(count as usize);
    let actual = normalize_windows_path(Path::new(&String::from_utf16(&buffer)?));
    if !windows_handle_matches(&actual, &expected, root) {
        bail!("opened file handle does not match the selected path beneath its watch root");
    }
    Ok(file)
}

/// Compare a resolved handle path against the selected path and its watch root.
/// `GetFinalPathNameByHandleW` returns `\\?\-prefixed paths, so the root needs the
/// same normalization as the handle and expected paths before `starts_with`.
#[cfg(windows)]
fn windows_handle_matches(actual: &Path, expected: &Path, root: &Path) -> bool {
    let normalized_actual = normalize_windows_path(actual);
    let normalized_root = normalize_windows_path(root);
    normalized_actual.starts_with(&normalized_root)
        && same_windows_path(&normalized_actual, &normalize_windows_path(expected))
}

#[cfg(windows)]
fn normalize_windows_path(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(unc_path) = value.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{unc_path}"));
    }
    let value = value.strip_prefix(r"\\?\").unwrap_or(&value);
    PathBuf::from(value)
}

#[cfg(windows)]
fn same_windows_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(not(any(unix, windows)))]
fn open_beneath(_root: &Path, _relative: &Path) -> Result<File> {
    Err(anyhow!(
        "no-follow file reads are unsupported on this platform"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn rejects_outside_symlink_swapped_after_path_selection() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let selected_path = root.path().join("document.txt");
        let target = outside.path().join("secret.txt");
        std::fs::write(&selected_path, "inside").unwrap();
        std::fs::write(&target, "outside secret").unwrap();

        // The caller selected this regular lexical path before the replacement.
        let selected_path = std::fs::canonicalize(selected_path).unwrap();
        std::fs::remove_file(&selected_path).unwrap();
        symlink(&target, &selected_path).unwrap();

        assert!(open_file_under_roots(&selected_path, &[root.path().to_path_buf()]).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_handle_match_normalizes_verbatim_prefixes() {
        let root = Path::new(r"\\?\C:\repo");
        let actual = Path::new(r"\\?\C:\repo\doc.txt");
        let expected = Path::new(r"C:\repo\doc.txt");
        assert!(windows_handle_matches(actual, expected, root));

        let outside = Path::new(r"\\?\C:\elsewhere\secret.txt");
        assert!(!windows_handle_matches(
            outside,
            Path::new(r"C:\elsewhere\secret.txt"),
            root
        ));
    }
}
