use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
enum FileLocation {
    Path(PathBuf),
}

#[derive(Debug)]
pub enum FsError {
    AccessDenied,
    AlreadyExist,
    DirectoryNotEmpty,
    DoesNotExist,
    InvalidParentDir,
    NonexistentParentDir,
    ReadonlyParentDir,
}

#[derive(Debug)]
enum FsNode {
    File {
        location: FileLocation,
        writeable: bool,
    },
    Directory {
        children: HashMap<String, FsNode>,
        writeable: Option<PathBuf>,
    },
}
impl FsNode {
    fn from_host_dir(host_path: &Path, writeable: bool) -> Self {
        let mut children = HashMap::new();
        for entry in std::fs::read_dir(host_path).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            let host_path = entry.path();
            let name = entry.file_name().into_string().unwrap();

            // There is no support for symlinks within the virtual filesystem,
            // we treat a symlink as if it were a copy of the file it points to.
            let kind = if kind.is_symlink() {
                std::fs::metadata(&host_path).unwrap().file_type()
            } else {
                kind
            };

            if kind.is_file() {
                children.insert(
                    name,
                    FsNode::File {
                        location: FileLocation::Path(host_path),
                        writeable,
                    },
                );
            } else if kind.is_dir() {
                children.insert(name, FsNode::from_host_dir(&host_path, writeable));
            } else {
                panic!("{:?} is not a symlink, file or directory", host_path);
            }
        }
        FsNode::Directory {
            children,
            writeable: match writeable {
                true => Some(host_path.to_owned()),
                false => None,
            },
        }
    }

    fn dir() -> Self {
        FsNode::Directory {
            children: HashMap::new(),
            writeable: None,
        }
    }
    fn with_child(mut self, name: &str, child: FsNode) -> Self {
        let FsNode::Directory {
            ref mut children,
            writeable: _,
        } = self
        else {
            panic!();
        };
        assert!(children.insert(String::from(name), child).is_none());
        self
    }
    fn host_file(location: PathBuf) -> Self {
        FsNode::File {
            location: FileLocation::Path(location),
            writeable: false,
        }
    }
}

/// Like [Path] but for the virtual filesystem.
#[repr(transparent)]
#[derive(Debug)]
pub struct GuestPath(str);
impl GuestPath {
    pub fn new<S: AsRef<str> + ?Sized>(s: &S) -> &GuestPath {
        unsafe { &*(s.as_ref() as *const str as *const GuestPath) }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Join a path component.
    pub fn join<P: AsRef<str>>(&self, path: P) -> GuestPathBuf {
        GuestPathBuf::from(format!("{}/{}", self.as_str(), path.as_ref()))
    }

    /// Splits the path into a parent path and a file name.
    pub fn parent_and_file_name(&self) -> Option<(&GuestPath, &str)> {
        let path = self.as_str();
        let separator_idx = path.rfind(|c| c == '/' || c == '\\')?;
        let (parent_name, file_name) = path.split_at(separator_idx);
        Some((GuestPath::new(parent_name), &file_name[1..]))
    }

    /// Get the final component of the path.
    pub fn file_name(&self) -> Option<&str> {
        let (_, file_name) = self.parent_and_file_name()?;
        Some(file_name)
    }

    /// Get the parent directory of the path.
    pub fn parent(&self) -> Option<&GuestPath> {
        let (parent_name, _) = self.parent_and_file_name()?;
        Some(parent_name)
    }
}
impl AsRef<GuestPath> for GuestPath {
    fn as_ref(&self) -> &Self {
        self
    }
}
impl AsRef<str> for GuestPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
impl AsRef<GuestPath> for str {
    fn as_ref(&self) -> &GuestPath {
        unsafe { &*(self as *const str as *const GuestPath) }
    }
}
impl ToOwned for GuestPath {
    type Owned = GuestPathBuf;

    fn to_owned(&self) -> GuestPathBuf {
        GuestPathBuf::from(self)
    }
}

/// Like [File] but for the guest filesystem.
#[derive(Debug)]
pub enum GuestFile {
    File(File),
    Directory,
}

impl GuestFile {
    fn from_host_file(file: File) -> GuestFile {
        GuestFile::File(file)
    }

    fn from_directory() -> GuestFile {
        GuestFile::Directory
    }

    pub fn sync_all(&self) -> std::io::Result<()> {
        match self {
            GuestFile::File(file) => file.sync_all(),
            GuestFile::Directory => panic!("Attempt to sync a directory as a guest file"),
        }
    }
}

impl Read for GuestFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            GuestFile::File(file) => file.read(buf),
            GuestFile::Directory => panic!("Attempt to read from a directory as a guest file"),
        }
    }
}

impl Write for GuestFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            GuestFile::File(file) => file.write(buf),
            GuestFile::Directory => panic!("Attempt to write to a directory as a guest file"),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            GuestFile::File(file) => file.flush(),
            GuestFile::Directory => panic!("Attempt to flush a directory as a guest file"),
        }
    }
}

impl Seek for GuestFile {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match self {
            GuestFile::File(file) => file.seek(pos),
            GuestFile::Directory => panic!("Attempt to seek in a directory as a guest file"),
        }
    }
}

/// Like [PathBuf] but for the virtual filesystem.
#[derive(Debug, Clone)]
pub struct GuestPathBuf(String);
impl From<String> for GuestPathBuf {
    fn from(string: String) -> GuestPathBuf {
        GuestPathBuf(string)
    }
}
impl From<&GuestPath> for GuestPathBuf {
    fn from(guest_path: &GuestPath) -> GuestPathBuf {
        guest_path.as_str().to_string().into()
    }
}
impl From<GuestPathBuf> for String {
    fn from(guest_path: GuestPathBuf) -> String {
        guest_path.0
    }
}
impl std::ops::Deref for GuestPathBuf {
    type Target = GuestPath;

    fn deref(&self) -> &GuestPath {
        let s: &str = &self.0;
        s.as_ref()
    }
}
impl AsRef<GuestPath> for GuestPathBuf {
    fn as_ref(&self) -> &GuestPath {
        self
    }
}
impl std::borrow::Borrow<GuestPath> for GuestPathBuf {
    fn borrow(&self) -> &GuestPath {
        self
    }
}

fn apply_path_component<'a>(components: &mut Vec<&'a str>, component: &'a str) {
    match component {
        "" => (),
        "." => (),
        ".." => {
            components.pop();
        }
        _ => components.push(component),
    }
}

/// Resolve a path so that it is absolute and has no `.`, `..` or empty
/// components. The result is a series of zero or more path components forming
/// an absolute path (e.g. `["foo", "bar"]` means `/foo/bar`).
///
/// `relative_to` is the starting point for resolving a relative path, e.g. the
/// current directory. It must be an absolute path. It is optional if `path`
/// is absolute.
fn resolve_path<'a>(path: &'a GuestPath, relative_to: Option<&'a GuestPath>) -> Vec<&'a str> {
    let mut components = Vec::new();

    if !path.as_str().starts_with('/') && !path.as_str().starts_with('\\') {
        let relative_to = relative_to.unwrap().as_str();
        assert!(relative_to.starts_with('/') || relative_to.starts_with('\\'));
        for component in relative_to.split(['/', '\\']) {
            apply_path_component(&mut components, component);
        }
    }

    for component in path.as_str().split(['/', '\\']) {
        apply_path_component(&mut components, component);
    }

    components
}

/// Like [std::fs::OpenOptions] but for the guest filesystem.
/// TODO: `create_new`.
#[derive(Debug)]
pub struct GuestOpenOptions {
    read: bool,
    write: bool,
    append: bool,
    create: bool,
    truncate: bool,
}
impl GuestOpenOptions {
    pub fn new() -> GuestOpenOptions {
        GuestOpenOptions {
            read: false,
            write: false,
            append: false,
            create: false,
            truncate: false,
        }
    }
    pub fn read(&mut self) -> &mut Self {
        self.read = true;
        self
    }
    pub fn write(&mut self) -> &mut Self {
        self.write = true;
        self
    }
    pub fn append(&mut self) -> &mut Self {
        self.append = true;
        self
    }
    pub fn create(&mut self) -> &mut Self {
        self.create = true;
        self
    }
    pub fn truncate(&mut self) -> &mut Self {
        self.truncate = true;
        self
    }
}

/// Handles host I/O errors by panicking.
fn handle_open_err<T>(open_result: std::io::Result<T>, host_path: &Path) -> T {
    match open_result {
        Ok(ok) => ok,
        Err(e) => panic!("Unexpected I/O failure when trying to access real path {:?}: {}. This might indicate that files needed by skyrmp are missing, or were moved while it was running.", host_path, e),
    }
}

/// The type that owns the guest filesystem and provides accessors for it.
#[derive(Debug)]
pub struct Fs {
    root: FsNode,
    working_directory: GuestPathBuf,
    home_directory: GuestPathBuf,
}
impl Fs {
    pub fn new(mrp_host_path: &Path) -> (Fs, GuestPathBuf) {
        let mythroad_host_path = mrp_host_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        if !mythroad_host_path.is_dir() {
            panic!("Parent directory does not exist: {:?}", mythroad_host_path);
        }

        let working_directory = GuestPathBuf::from("/mythroad".to_string());
        let home_directory = GuestPathBuf::from("/mythroad".to_string());

        let mrp_guest_path = GuestPathBuf::from(format!(
            "/mythroad/{}",
            mrp_host_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
        ));

        let root =
            FsNode::dir().with_child("mythroad", FsNode::from_host_dir(&mythroad_host_path, true));

        log_dbg!("Initial filesystem layout: {:#?}", root);

        (
            Fs {
                root,
                working_directory,
                home_directory,
            },
            mrp_guest_path,
        )
    }

    /// Get the absolute path of the guest app's (sandboxed) home directory.
    pub fn home_directory(&self) -> &GuestPath {
        &self.home_directory
    }

    /// Get the node at a given path, if it exists.
    fn lookup_node(&self, path: &GuestPath) -> Option<&FsNode> {
        let mut node = &self.root;
        for component in resolve_path(path, Some(&self.working_directory)) {
            let FsNode::Directory {
                children,
                writeable: _,
            } = node
            else {
                return None;
            };
            node = children.get(component)?
        }
        Some(node)
    }

    /// Get the parent of the node at a given path, if it exists, and return it
    /// together with the final path component. This is an alternative to
    /// [Self::lookup_node] useful when writing to a file, where it might not
    /// exist yet (but its parent directory does).
    fn lookup_parent_node(&mut self, path: &GuestPath) -> Option<(&mut FsNode, String)> {
        let components = resolve_path(path, Some(&self.working_directory));
        let (&final_component, parent_components) = components.split_last()?;

        let mut parent = &mut self.root;
        for &component in parent_components {
            let FsNode::Directory {
                children,
                writeable: _,
            } = parent
            else {
                return None;
            };
            parent = children.get_mut(component)?
        }

        Some((parent, final_component.to_string()))
    }

    pub fn exists(&self, path: &GuestPath) -> bool {
        self.lookup_node(path).is_some()
    }

    /// Returns access information about the file/directory at the path
    /// (exists, read, write, execute)
    pub fn access(&self, path: &GuestPath) -> (bool, bool, bool, bool) {
        match self.lookup_node(path) {
            None => (false, false, false, false),
            Some(node) => match node {
                FsNode::File {
                    location: _,
                    writeable,
                } => (true, true, *writeable, false),
                FsNode::Directory {
                    children: _,
                    writeable,
                } => (true, true, writeable.is_some(), true),
            },
        }
    }

    /// Like [std::path::Path::is_file] but for the guest filesystem.
    pub fn is_file(&self, path: &GuestPath) -> bool {
        matches!(self.lookup_node(path), Some(FsNode::File { .. }))
    }

    /// Like [std::path::Path::is_dir] but for the guest filesystem.
    pub fn is_dir(&self, path: &GuestPath) -> bool {
        matches!(self.lookup_node(path), Some(FsNode::Directory { .. }))
    }

    pub fn size(&self, path: &GuestPath) -> Result<u64, ()> {
        // TODO: error handling
        let node = self.lookup_node(path).ok_or(())?;
        match node {
            FsNode::File { location, .. } => match location {
                FileLocation::Path(path) => {
                    fs::metadata(path).map(|meta| meta.len()).map_err(|_| ())
                }
            },
            _ => unimplemented!(),
        }
    }

    /// Like [std::fs::read] but for the guest filesystem.
    pub fn read<P: AsRef<GuestPath>>(&self, path: P) -> Result<Vec<u8>, ()> {
        let mut file = self.open(path.as_ref())?;
        let mut result = Vec::new();
        file.read_to_end(&mut result).map_err(|_| ())?;
        Ok(result)
    }

    /// Like [std::fs::File::open] but for the guest filesystem.
    #[allow(dead_code)]
    pub fn open<P: AsRef<GuestPath>>(&self, path: P) -> Result<GuestFile, ()> {
        let node = self.lookup_node(path.as_ref()).ok_or(())?;
        match node {
            FsNode::File { location, .. } => match location {
                FileLocation::Path(host_path) => {
                    let host_file = handle_open_err(File::open(host_path), host_path);
                    Ok(GuestFile::from_host_file(host_file))
                }
                _ => unimplemented!(),
            },
            FsNode::Directory { .. } => Err(()),
        }
    }

    pub fn rename<P: AsRef<GuestPath> + Copy>(&mut self, from: P, to: P) -> Result<(), ()> {
        let from_node = self.lookup_node(from.as_ref()).ok_or(())?;
        let from_host_path = match from_node {
            FsNode::File {
                location: from_location,
                writeable: from_writeable,
            } => {
                let FileLocation::Path(from_host_path) = from_location;
                assert!(from_writeable); // TODO: return errno
                                         // TODO: avoid copy?
                from_host_path.clone()
            }
            _ => unimplemented!(),
        };

        if self.lookup_node(to.as_ref()).is_none() {
            // In case target guest node do not exist, we need to create one
            let mut options = GuestOpenOptions::new();
            options.write().create().truncate();
            self.open_with_options(to, options)?;
        }

        let to_node = self.lookup_node(to.as_ref()).unwrap();
        let FsNode::File {
            location: to_location,
            writeable: to_writeable,
        } = to_node
        else {
            // TODO: return EISDIR
            return Err(());
        };
        let FileLocation::Path(to_host_path) = to_location;
        assert!(to_writeable); // TODO: return errno
        let res = fs::rename(from_host_path, to_host_path);
        if res.is_ok() {
            // Remove reference to the old from node
            let (parent_from, component) = self.lookup_parent_node(from.as_ref()).unwrap();
            let FsNode::Directory { children, .. } = parent_from else {
                panic!()
            };
            children.remove(&component).unwrap();
        }
        res.map_err(|_| ())
    }

    /// Like [File::options] but for the guest filesystem.
    pub fn open_with_options<P: AsRef<GuestPath>>(
        &mut self,
        path: P,
        options: GuestOpenOptions,
    ) -> Result<GuestFile, ()> {
        let GuestOpenOptions {
            read,
            write,
            append,
            create,
            truncate,
        } = options;
        assert!((!truncate && !create) || write || append);

        let path = path.as_ref();

        let (parent_node, new_filename) = self.lookup_parent_node(path).ok_or(())?;
        let FsNode::Directory {
            children,
            writeable: dir_host_path,
        } = parent_node
        else {
            return Err(());
        };

        // Open an existing file if possible

        if let Some(existing_file) = children.get(&new_filename) {
            match existing_file {
                FsNode::File {
                    ref location,
                    writeable,
                } => {
                    if !writeable && (append || write) {
                        log!("Warning: attempt to write to read-only file {:?}", path);
                        return Err(());
                    }
                    match location {
                        FileLocation::Path(host_path) => {
                            let file = handle_open_err(
                                File::options()
                                    .read(read)
                                    .write(write)
                                    .append(append)
                                    .create(false)
                                    .truncate(truncate)
                                    .open(host_path),
                                host_path,
                            );
                            return Ok(GuestFile::from_host_file(file));
                        }
                    }
                }
                FsNode::Directory { .. } => {
                    if write {
                        return Err(());
                    } else {
                        return Ok(GuestFile::from_directory());
                    }
                }
            }
        };

        // Create a new file otherwise

        if !create {
            return Err(());
        }

        let Some(dir_host_path) = dir_host_path else {
            log!(
                "Warning: attempt to create file at path {:?}, but directory is read-only",
                path
            );
            return Err(());
        };

        for c in new_filename.chars() {
            if std::path::is_separator(c) {
                panic!("Attempt to create file at path {:?}, but filename contains path separator character {:?}!", path, c);
            }
        }

        let host_path = dir_host_path.join(&new_filename);

        let file = handle_open_err(
            File::options()
                .read(read)
                .write(write)
                .append(append)
                .create(create)
                .truncate(truncate)
                .open(&host_path),
            &host_path,
        );
        log_dbg!(
            "Created file at path {:?} (host path: {:?})",
            path,
            host_path
        );
        children.insert(
            new_filename,
            FsNode::File {
                location: FileLocation::Path(host_path),
                writeable: true,
            },
        );
        Ok(GuestFile::from_host_file(file))
    }

    /// Removes a file or a directory. If the node is a directory, it must be
    /// empty.
    pub fn remove<P: AsRef<GuestPath>>(&mut self, path: P) -> Result<(), FsError> {
        let path = path.as_ref();

        let (parent_node, node_name) = self
            .lookup_parent_node(path)
            .ok_or(FsError::NonexistentParentDir)?;

        // Parent directory is not a directory
        let FsNode::Directory {
            children,
            writeable: dir_writeable,
        } = parent_node
        else {
            return Err(FsError::InvalidParentDir);
        };

        if !dir_writeable.is_some() {
            log!("Warning: attempt to delete file or directroy at path {:?}, but parent directory is read-only", path);
            return Err(FsError::ReadonlyParentDir);
        };

        let Some(node) = children.get(&node_name) else {
            // There is no file/directory with this name
            return Err(FsError::DoesNotExist);
        };

        match node {
            FsNode::File {
                location,
                writeable,
            } => {
                // Read-only files can't be removed. (This is probably not
                // correct, but it is safer for now.)
                if !writeable {
                    return Err(FsError::AccessDenied);
                }

                let host_path = match location {
                    FileLocation::Path(host_path) => host_path,
                };

                handle_open_err(std::fs::remove_file(host_path), host_path);
                log_dbg!(
                    "Deleted file at path {:?} (host path: {:?})",
                    path,
                    host_path
                );
            }
            FsNode::Directory {
                children,
                writeable,
            } => {
                // Directory is not empty
                if !children.is_empty() {
                    return Err(FsError::DirectoryNotEmpty);
                }
                // Read-only directories can't be removed. (This is probably not
                // correct, but it is safer for now.)
                let Some(host_path) = writeable else {
                    return Err(FsError::AccessDenied);
                };

                handle_open_err(std::fs::remove_dir(host_path), host_path);
                log_dbg!(
                    "Deleted directory at path {:?} (host path: {:?})",
                    path,
                    host_path
                );
            }
        }

        children.remove(&node_name).unwrap();

        Ok(())
    }

    /// Like [std::fs::create_dir_all] but for the guest filesystem.
    pub fn create_dir_all<P: AsRef<GuestPath>>(&mut self, path: P) -> Result<(), FsError> {
        let path = path.as_ref();
        assert!(path.as_str().starts_with('/'));
        // TODO: use GuestPathBuf push() once implemented
        let mut tmp_vec = vec![""];
        let components = resolve_path(path, None);
        for component in components {
            tmp_vec.push(component);
            let res = self.create_dir(GuestPathBuf::from(tmp_vec.join("/")));
            match res {
                Ok(_) | Err(FsError::AlreadyExist) => {}
                _ => return res,
            }
        }
        Ok(())
    }

    /// Like [std::fs::create_dir] but for the guest filesystem.
    pub fn create_dir<P: AsRef<GuestPath>>(&mut self, path: P) -> Result<(), FsError> {
        let path = path.as_ref();

        let (parent_node, new_dir_name) = self
            .lookup_parent_node(path)
            .ok_or(FsError::NonexistentParentDir)?;

        // Parent directory is not a directory
        let FsNode::Directory {
            children,
            writeable: dir_host_path,
        } = parent_node
        else {
            return Err(FsError::InvalidParentDir);
        };

        // There's already a file/directory with this name
        if children.contains_key(&new_dir_name) {
            return Err(FsError::AlreadyExist);
        }

        let Some(dir_host_path) = dir_host_path else {
            log!("Warning: attempt to create directory at path {:?}, but parent directory is read-only", path);
            return Err(FsError::ReadonlyParentDir);
        };

        for c in new_dir_name.chars() {
            if std::path::is_separator(c) {
                panic!("Attempt to create directory at path {path:?}, but directory name contains path separator character {c:?}!");
            }
        }

        let host_path = dir_host_path.join(&new_dir_name);

        handle_open_err(std::fs::create_dir(&host_path), &host_path);
        log_dbg!(
            "Created directory at path {:?} (host path: {:?})",
            path,
            host_path
        );
        children.insert(
            new_dir_name,
            FsNode::Directory {
                children: HashMap::new(),
                writeable: Some(host_path),
            },
        );
        Ok(())
    }
}
