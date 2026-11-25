//! Media paths that survive moving a project between folders, drives and operating systems.
//!
//! A project file stores the path of every media and proxy file. A file inside the project's own
//! folder is saved **relative to the project file**, always with `/` separators (`Assets/a.mp4`),
//! which Windows and macOS both read. Files elsewhere keep their absolute path. Reading is in
//! [`crate::doc_path`]: a relative path is taken from the folder of the document it was read from.
//!
//! Projects written by earlier builds hold absolute paths. When such a path names a file that does
//! not exist here (another computer's `C:\Users\me\Film\Assets\a.mp4`), [`locate_beside`] looks for
//! the same trailing folders under the project's own folder, so a project carried together with its
//! media opens online with no relinking.

use std::path::{Component, Path, PathBuf};

use filmcraft_project::{MediaRef, Project};

use crate::MediaPool;

/// `file` as a `/`-separated path relative to `folder`, when `file` lies inside it. A name that
/// holds a separator or `:` is not portable, so such a file keeps its absolute path (None).
pub fn relative_inside(file: &Path, folder: &Path) -> Option<String> {
    let rest = file.strip_prefix(folder).ok()?;
    let mut parts = Vec::new();
    for c in rest.components() {
        let Component::Normal(name) = c else { return None };
        let name = name.to_str()?;
        if name.is_empty() || name.contains(['/', '\\', ':']) {
            return None;
        }
        parts.push(name);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Every media and proxy path of `project`, mutably.
fn paths_mut(project: &mut Project) -> impl Iterator<Item = &mut String> {
    project.items.values_mut().filter_map(|i| i.as_media_mut()).flat_map(|m| [Some(&mut m.media), m.proxy.as_mut()]).filter_map(|r| match r {
        Some(MediaRef::File { path }) => Some(path),
        _ => None,
    })
}

/// Make the relative paths of a project just read from `document` absolute, taking them from the
/// document's folder (`doc_path`). The open project then holds one kind of path, the file it
/// names, whichever folder it was saved in; saving writes the portable form again
/// ([`for_document`]). A path that is not followed (another computer's, a device) is left as it is.
pub fn absolutize_stored(project: &mut Project, document: &Path) {
    if cfg!(target_arch = "wasm32") {
        return;
    }
    for path in paths_mut(project) {
        if !Path::new(path.as_str()).is_absolute()
            && let Ok(file) = crate::doc_path::resolve(path, Some(document))
        {
            *path = file;
        }
    }
}

/// Make every stored path absolute, as the media pool resolves it now (a relative path from the
/// folder of the document it was loaded from). A path the pool does not follow, such as another
/// computer's, is left as it is.
pub fn absolutize(project: &mut Project, pool: &MediaPool) {
    for path in paths_mut(project) {
        // Only a path that is a file here: `std::path::absolute` would prefix another computer's
        // path (`\\server\share\a.mov`, `C:\x`) with this process's folder.
        if let Ok(file) = pool.follow(path)
            && Path::new(&file).is_absolute()
        {
            *path = file;
        }
    }
}

/// Save every absolute path that lies inside the folder of `document` relative to it. Other paths
/// (outside the folder, relative, another computer's) are left as they are.
pub fn relativize(project: &mut Project, document: &Path) {
    let Some(folder) = std::path::absolute(document).ok().and_then(|d| d.parent().map(Path::to_path_buf)) else { return };
    for path in paths_mut(project) {
        let file = Path::new(path.as_str());
        if file.is_absolute()
            && let Some(rel) = relative_inside(file, &folder)
        {
            *path = rel;
        }
    }
}

/// The project as it is written to `document`: portable relative paths for the files inside the
/// document's folder. `project`'s own paths may be relative to the document it was opened from, so
/// they are made absolute first and Save As keeps every link.
pub fn for_document(project: &Project, pool: &MediaPool, document: &Path) -> Project {
    let mut out = project.clone();
    // The web host keeps its own virtual paths; there is no folder to be relative to.
    if cfg!(target_arch = "wasm32") {
        return out;
    }
    absolutize(&mut out, pool);
    relativize(&mut out, document);
    out
}

/// The file a missing stored path stands for, found under `folder`: the longest trailing part of
/// `stored` (cut at `/` and `\`, so a Windows path works on macOS and the reverse) that exists
/// there, with how many components it has. Longer matches keep the folder structure, so two files
/// of the same name in different subfolders are not mixed up. `exists` tells whether a file is there.
pub fn locate_beside(stored: &str, folder: &Path, exists: impl Fn(&Path) -> bool) -> Option<(PathBuf, usize)> {
    let stored = stored.strip_prefix("file://").unwrap_or(stored);
    let parts: Vec<&str> = stored.split(['/', '\\']).filter(|p| !p.is_empty() && *p != "." && *p != "..").collect();
    // a drive letter (`C:`) is not a folder of the project
    let parts: Vec<&str> = match parts.split_first() {
        Some((first, rest)) if first.len() == 2 && first.ends_with(':') => rest.to_vec(),
        _ => parts,
    };
    (1..=parts.len()).rev().find_map(|k| {
        let tail = &parts[parts.len() - k..];
        if tail.iter().any(|p| p.contains(':')) {
            return None;
        }
        let candidate = tail.iter().fold(folder.to_path_buf(), |dir, part| dir.join(part));
        exists(&candidate).then_some((candidate, k))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_inside_the_folder_are_relative_with_forward_slashes() {
        let folder = std::env::temp_dir().join("MyProject");
        let inside = folder.join("Assets").join("Videos").join("a.mp4");
        assert_eq!(relative_inside(&inside, &folder).as_deref(), Some("Assets/Videos/a.mp4"));
        assert_eq!(relative_inside(&std::env::temp_dir().join("Other").join("a.mp4"), &folder), None);
        assert_eq!(relative_inside(&folder, &folder), None, "the folder itself is no file");
        // a sibling that only shares the name prefix is outside
        assert_eq!(relative_inside(&std::env::temp_dir().join("MyProject 2").join("a.mp4"), &folder), None);
    }

    #[test]
    fn names_that_are_not_portable_stay_absolute() {
        let folder = std::env::temp_dir().join("MyProject");
        assert_eq!(relative_inside(&folder.join("take:2.mp4"), &folder), None);
        #[cfg(unix)]
        assert_eq!(relative_inside(&folder.join(r"back\slash.mp4"), &folder), None);
    }

    #[test]
    fn spaces_and_unicode_are_kept() {
        let folder = std::env::temp_dir().join("Film Projesi");
        let file = folder.join("Görseller").join("Çiçek ışığı.png");
        assert_eq!(relative_inside(&file, &folder).as_deref(), Some("Görseller/Çiçek ışığı.png"));
    }

    #[test]
    fn another_computers_path_is_found_by_its_trailing_folders() {
        let folder = PathBuf::from("/proj");
        let has = |want: &'static [&'static str]| move |p: &Path| want.iter().any(|w| p == Path::new(w));
        // the whole structure below the project folder is kept
        let (found, k) = locate_beside(r"C:\Users\me\Film\Assets\Videos\a.mp4", &folder, has(&["/proj/Assets/Videos/a.mp4", "/proj/a.mp4"])).unwrap();
        assert_eq!((found, k), (PathBuf::from("/proj/Assets/Videos/a.mp4"), 3));
        let (found, k) = locate_beside("/Users/me/Film/Assets/a.mp4", &folder, has(&["/proj/Assets/a.mp4"])).unwrap();
        assert_eq!((found, k), (PathBuf::from("/proj/Assets/a.mp4"), 2));
        // the same name in another subfolder is not taken for it
        assert_eq!(locate_beside("/Users/me/Film/Assets/Videos/a.mp4", &folder, has(&["/proj/Assets/Photos/a.mp4"])), None);
        // `..` and `.` never lead out of the folder
        assert_eq!(locate_beside("../../etc/passwd", &folder, has(&["/etc/passwd"])), None);
        assert_eq!(locate_beside("", &folder, |_| true), None);
    }
}
