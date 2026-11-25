//! Projects keep their media links when the project folder moves between drives and operating
//! systems: files inside the project's folder are saved relative to the project file, files
//! elsewhere keep their absolute path, projects from earlier builds are linked beside the project
//! when that is safe, and Project Manager's collected folder is portable.

use std::path::{Path, PathBuf};

use filmcraft_media::DemoScene;
use filmcraft_project::MediaRef;
use serde_json::json;

use crate::Session;
use crate::media_test_util::{make_movie, session_with, tmp_dir};

const W: u32 = 160;
const H: u32 = 90;

/// A project of two clips with spaces and non-ASCII letters in their paths, in
/// `<root>/Assets/Videos` and `<root>/Assets/Çiçek`, saved as `<root>/Film Projesi.fcproj`.
fn project_in(root: &Path) -> (Session, String) {
    let (a, b) = (root.join("Assets").join("Videos").join("clip one.mov"), root.join("Assets").join("Çiçek").join("bölüm ışık.mov"));
    for f in [&a, &b] {
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    }
    make_movie(&a, DemoScene::Aurora, W, H, 12);
    make_movie(&b, DemoScene::Dunes, W, H, 12);
    let (mut s, _, _) = session_with(&[&a, &b]);
    let path = root.join("Film Projesi.fcproj").to_string_lossy().into_owned();
    s.execute("file.save", json!({"path": path})).unwrap();
    (s, path)
}

/// The media paths as the project file stores them (sorted).
fn stored_in_file(path: &str) -> Vec<String> {
    let loaded = filmcraft_format::decode(&std::fs::read(path).unwrap()).unwrap();
    stored(&loaded.project)
}

fn stored(p: &filmcraft_project::Project) -> Vec<String> {
    let mut v: Vec<String> =
        p.items.values().filter_map(|i| i.as_media()).filter_map(|m| if let MediaRef::File { path } = &m.media { Some(path.clone()) } else { None }).collect();
    v.sort();
    v
}

/// Open `path` and return the session and the number of missing media files.
fn open(path: &str) -> (Session, u64) {
    let mut s = Session::default();
    let r = s.execute("file.open", json!({"path": path})).unwrap();
    let missing = r["missingMedia"].as_u64().unwrap();
    (s, missing)
}

/// Every media file of `s` exists where the media pool looks for it.
fn all_online(s: &Session) -> bool {
    let paths = stored(&s.project);
    !paths.is_empty() && paths.iter().all(|p| s.media.follow(p).is_ok_and(|f| Path::new(&f).is_file()))
}

#[test]
fn media_inside_the_project_folder_is_saved_relative_with_forward_slashes() {
    let root = tmp_dir("portable-save");
    let (s, path) = project_in(&root);
    assert_eq!(stored_in_file(&path), ["Assets/Videos/clip one.mov", "Assets/Çiçek/bölüm ışık.mov"]);
    // the open session keeps working with what it has
    assert!(stored(&s.project).iter().all(|p| Path::new(p).is_absolute()));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_renamed_or_moved_project_folder_opens_with_every_file_online() {
    let root = tmp_dir("portable-move");
    let (s, path) = project_in(&root.join("Original"));
    drop(s); // Windows does not allow renaming a directory while decoder files are open.
    let (other_drive, renamed) = (root.join("Other drive"), root.join("Yeni ad"));
    std::fs::create_dir_all(&other_drive).unwrap();
    std::fs::rename(root.join("Original"), &renamed).unwrap();
    std::fs::rename(&renamed, other_drive.join("MyProject")).unwrap();
    assert!(!Path::new(&path).exists());
    let moved = other_drive.join("MyProject").join("Film Projesi.fcproj");
    let (t, missing) = open(moved.to_str().unwrap());
    assert_eq!(missing, 0);
    assert!(all_online(&t));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn media_outside_the_project_folder_keeps_its_absolute_path() {
    let root = tmp_dir("portable-outside");
    let shared = root.join("Shared").join("take 1.mov");
    std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
    make_movie(&shared, DemoScene::Plasma, W, H, 12);
    let (mut s, _, _) = session_with(&[&shared]);
    let path = root.join("Project").join("p.fcproj");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    s.execute("file.save", json!({"path": path.to_string_lossy()})).unwrap();
    assert_eq!(stored_in_file(path.to_str().unwrap()), [shared.to_string_lossy().into_owned()]);
    let (t, missing) = open(path.to_str().unwrap());
    assert_eq!(missing, 0);
    assert!(all_online(&t));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn save_as_keeps_every_link_whether_or_not_the_media_comes_along() {
    let root = tmp_dir("portable-saveas");
    let (s, path) = project_in(&root);
    drop(s);
    // a project opened from its relative paths, saved into a folder that does not hold the media
    let (mut t, _) = open(&path);
    let elsewhere = root.join("Backup").join("Kopya.fcproj");
    std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    t.execute("file.saveAs", json!({"path": elsewhere.to_string_lossy()})).unwrap();
    assert!(stored_in_file(elsewhere.to_str().unwrap()).iter().all(|p| Path::new(p).is_absolute()), "the media is outside the new folder");
    let (u, missing) = open(elsewhere.to_str().unwrap());
    assert_eq!((missing, all_online(&u)), (0, true));
    // and into a folder of the project that holds one of them: that one is relative from the new
    // place, the other keeps its absolute path
    let (mut t, _) = open(&path);
    let deeper = root.join("Assets").join("Videos").join("v2.fcproj");
    t.execute("file.saveAs", json!({"path": deeper.to_string_lossy()})).unwrap();
    let paths = stored_in_file(deeper.to_str().unwrap());
    assert_eq!(paths.iter().filter(|p| p.as_str() == "clip one.mov").count(), 1, "{paths:?}");
    assert_eq!(paths.iter().filter(|p| Path::new(p).is_absolute()).count(), 1, "{paths:?}");
    let (u, missing) = open(deeper.to_str().unwrap());
    assert_eq!((missing, all_online(&u)), (0, true));
    let _ = std::fs::remove_dir_all(&root);
}

/// Rewrite the media paths of the project file at `path`.
fn rewrite_paths(path: &str, f: impl Fn(&str) -> String) {
    let mut loaded = filmcraft_format::decode(&std::fs::read(path).unwrap()).unwrap();
    for m in loaded.project.items.values_mut().filter_map(|i| i.as_media_mut()) {
        if let MediaRef::File { path } = &mut m.media {
            *path = f(path);
        }
    }
    std::fs::write(path, filmcraft_format::encode(&loaded.project, false)).unwrap();
}

#[test]
fn an_old_project_with_another_computers_absolute_paths_links_beside_itself() {
    let root = tmp_dir("portable-legacy");
    let (s, path) = project_in(&root);
    drop(s);
    // what a build from before relative paths wrote on Windows, and on a Mac
    for old_root in [r"C:\Users\Biri\Film", "/Users/biri/Film"] {
        rewrite_paths(&path, |p| {
            let tail = p.rsplit_once("Assets/").map_or(p, |(_, t)| t);
            let sep = if old_root.contains('\\') { "\\" } else { "/" };
            format!("{old_root}{sep}Assets{sep}{}", tail.replace('/', sep))
        });
        let (t, missing) = open(&path);
        assert_eq!(missing, 0, "{old_root}");
        assert!(all_online(&t));
        // saving writes the portable form
        let mut t = t;
        t.execute("file.save", json!({})).unwrap();
        assert_eq!(stored_in_file(&path).len(), 2);
        assert!(stored_in_file(&path).iter().all(|p| !Path::new(p).is_absolute() && !p.contains('\\')), "{:?}", stored_in_file(&path));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_file_of_the_same_name_in_another_folder_is_not_linked_in_its_place() {
    let root = tmp_dir("portable-lookalike");
    let (s, path) = project_in(&root);
    drop(s);
    rewrite_paths(&path, |p| format!(r"C:\Film\{}", p.rsplit('/').take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(r"\")));
    // the structure the old paths name is gone; a same-named file sits in a different folder
    let wrong = root.join("Baska").join("clip one.mov");
    std::fs::create_dir_all(wrong.parent().unwrap()).unwrap();
    std::fs::copy(root.join("Assets").join("Videos").join("clip one.mov"), &wrong).unwrap();
    std::fs::remove_dir_all(root.join("Assets")).unwrap();
    let (_t, missing) = open(&path);
    assert_eq!(missing, 2, "nothing is linked by file name alone");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn auto_relink_by_folder_refuses_to_pick_between_look_alikes() {
    let root = tmp_dir("portable-ambiguous");
    let media = root.join("Media").join("a.mov");
    std::fs::create_dir_all(media.parent().unwrap()).unwrap();
    make_movie(&media, DemoScene::Aurora, W, H, 12);
    let (mut s, _, _) = session_with(&[&media]);
    let path = root.join("Project").join("p.fcproj");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    s.execute("file.save", json!({"path": path.to_string_lossy()})).unwrap();
    drop(s);
    let (one, two) = (root.join("Search").join("one").join("a.mov"), root.join("Search").join("two").join("a.mov"));
    for f in [&one, &two] {
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::copy(&media, f).unwrap();
    }
    std::fs::remove_dir_all(root.join("Media")).unwrap();
    let (mut t, missing) = open(path.to_str().unwrap());
    assert_eq!(missing, 1);
    let r = t.execute("media.autoRelink", json!({"folder": root.join("Search").to_string_lossy()})).unwrap();
    assert_eq!(r["relinked"].as_array().unwrap().len(), 0, "{r}");
    assert_eq!(r["failed"][0]["candidates"].as_array().unwrap().len(), 2, "{r}");
    // once only one of them is left there is nothing to choose
    std::fs::remove_dir_all(root.join("Search").join("two")).unwrap();
    let r = t.execute("media.autoRelink", json!({"folder": root.join("Search").to_string_lossy()})).unwrap();
    assert_eq!(r["relinked"].as_array().unwrap().len(), 1, "{r}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_collected_project_is_portable() {
    let root = tmp_dir("portable-collect");
    let media: Vec<PathBuf> = ["Mavi dosya.mov", "b.mov"].iter().map(|n| root.join("Source files").join(n)).collect();
    std::fs::create_dir_all(media[0].parent().unwrap()).unwrap();
    make_movie(&media[0], DemoScene::Aurora, W, H, 12);
    make_movie(&media[1], DemoScene::Dunes, W, H, 12);
    let refs: Vec<&Path> = media.iter().map(PathBuf::as_path).collect();
    let (mut s, _, _) = session_with(&refs);
    let dest = root.join("Collected");
    let r = s.execute("file.projectManager", json!({"destination": dest.to_string_lossy(), "wait": true})).unwrap();
    let project = r["project"].as_str().unwrap().to_string();
    let jobs = s.execute("jobs.list", json!({})).unwrap();
    assert!(jobs[0]["result"].get("error").is_none(), "{jobs}");
    let paths = stored_in_file(&project);
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().all(|p| !Path::new(p).is_absolute() && !p.contains('\\') && !p.contains(':')), "{paths:?}");
    // the original media is gone, the folder moves: nothing needs linking
    drop(s);
    std::fs::remove_dir_all(root.join("Source files")).unwrap();
    let moved = root.join("USB").join("Teslim");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(&dest, &moved).unwrap();
    let (t, missing) = open(moved.join(Path::new(&project).file_name().unwrap()).to_str().unwrap());
    assert_eq!(missing, 0);
    assert!(all_online(&t));
    let _ = std::fs::remove_dir_all(&root);
}
