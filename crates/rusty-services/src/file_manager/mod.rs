//! Standalone file manager backend services for `rusty-files` (`RUSTYOS-065`, ADR-0030, ADR-0044).
//!
//! Provides XDG user directories, mount discovery from `/proc/mounts`, `FreeDesktop`
//! `.desktop` application discovery, recent files, `FreeDesktop` Trash specification
//! operations, local tag/preference persistence (`file_tags.toml`), sorting, and search.

pub mod listing;
pub mod metadata;
pub mod operations;
pub mod trash;
pub mod types;

pub use listing::*;
pub use metadata::*;
pub use operations::*;
pub use trash::*;
pub use types::*;

pub use crate::image_decode::{
    DecodedPreviewImage, PreviewMediaKind, classify_preview_filename,
    is_supported_preview_filename, load_raster_preview_bgra as load_image_preview_bgra,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::process;
    use std::sync::atomic::AtomicBool;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(label: &str) -> io::Result<Self> {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = env::temp_dir().join(format!(
                "rustyos-fm-test-{label}-{}-{unique}",
                process::id()
            ));
            fs::create_dir_all(&dir)?;
            Ok(Self { path: dir })
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn proc_mounts_parser_filters_pseudo_fs_and_decodes_spaces() {
        let sample = "\
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0
/dev/nvme0n1p2 / ext4 rw,relatime 0 0
tmpfs /run tmpfs rw,nosuid,nodev 0 0
/dev/sdb1 /run/media/anc/My\\040Drive exfat rw,nosuid 0 0
//nas.local/media /mnt/iNAS cifs rw,username=anc 0 0
";
        let vols = parse_proc_mounts(sample);
        assert_eq!(vols.len(), 3);
        assert_eq!(vols[0].mount_point, PathBuf::from("/"));
        assert_eq!(vols[0].name, "RustyOS HD");
        assert!(!vols[0].is_network);

        assert_eq!(
            vols[1].mount_point,
            PathBuf::from("/run/media/anc/My Drive")
        );
        assert_eq!(vols[1].name, "My Drive");
        assert!(!vols[1].is_network);

        assert_eq!(vols[2].name, "iNAS");
        assert!(vols[2].is_network);
    }

    #[test]
    fn tags_config_roundtrip_and_toggle_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("tags-cfg")?;
        let cfg_path = guard.path.join("rustyos/file_tags.toml");

        let mut cfg = FileTagsConfig {
            theme: FilesThemeMode::Dark,
            view_mode: FileViewMode::List,
            sort_field: FileSortField::Size,
            sort_order: FileSortOrder::Descending,
            show_hidden: true,
            ..Default::default()
        };

        let target = PathBuf::from("/home/alice/Documents/Quote \"A\"\\Draft.txt");
        assert!(cfg.toggle_tag(&target, FileTagColor::Blue));
        assert!(cfg.toggle_tag(&target, FileTagColor::Red));
        assert_eq!(
            cfg.tags_for(&target),
            &[FileTagColor::Red, FileTagColor::Blue]
        );
        assert_eq!(cfg.paths_with_tag(FileTagColor::Blue), vec![target.clone()]);

        cfg.save_to_path(&cfg_path)?;
        let loaded = FileTagsConfig::load_from_path(&cfg_path);
        assert_eq!(loaded, cfg);

        let mut mutated = loaded;
        assert!(!mutated.toggle_tag(&target, FileTagColor::Red));
        assert_eq!(mutated.tags_for(&target), &[FileTagColor::Blue]);
        assert!(!mutated.toggle_tag(&target, FileTagColor::Blue));
        assert_eq!(mutated.tags_for(&target), []);
        Ok(())
    }

    #[test]
    fn xdg_user_dirs_and_applications_discovery_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("xdg-apps")?;
        let home = guard.path.join("home/shackett");
        let cfg_root = guard.path.join("cfg");
        let data_home = guard.path.join("data");
        let user_apps = data_home.join("applications");
        let sys_apps = guard.path.join("sys_applications");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&user_apps)?;
        fs::create_dir_all(&sys_apps)?;

        let parsed = parse_xdg_user_dirs(
            "XDG_DESKTOP_DIR=\"$HOME/MyDesktop\"\nXDG_DOWNLOAD_DIR=\"$HOME/DL\"\n",
            &home,
        );
        assert_eq!(parsed.get("XDG_DESKTOP_DIR"), Some(&home.join("MyDesktop")));
        assert_eq!(parsed.get("XDG_DOWNLOAD_DIR"), Some(&home.join("DL")));

        fs::write(
            user_apps.join("editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=RustyEdit User\nExec=rusty-edit %F\n",
        )?;
        fs::write(
            sys_apps.join("editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=RustyEdit Sys\nExec=old-edit\n",
        )?;
        fs::write(
            sys_apps.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden Helper\nNoDisplay=true\nExec=noop\n",
        )?;

        let mut locs = UserFileLocations::with_roots(&home, &cfg_root, &data_home);
        locs.app_dirs = vec![user_apps, sys_apps];
        let apps = list_entries_for_target(
            &ListingTarget::Applications,
            &locs,
            &FileTagsConfig::default(),
        )?;
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "RustyEdit User");
        assert_eq!(apps[0].kind, FileEntryKind::Application);
        assert_eq!(apps[0].desktop_exec.as_deref(), Some("rusty-edit %F"));
        Ok(())
    }

    #[test]
    fn create_folder_trash_restore_and_empty_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("trash-ops")?;
        let work = guard.path.join("work");
        let trash = guard.path.join("Trash");
        fs::create_dir_all(&work)?;

        let f1 = create_unique_folder(&work, "untitled folder")?;
        let f2 = create_unique_folder(&work, "untitled folder")?;
        assert!(f1.ends_with("untitled folder"));
        assert!(f2.ends_with("untitled folder 2"));
        assert!(validate_single_component_name("bad/name").is_err());
        assert!(validate_single_component_name("..").is_err());

        let doc = work.join("hello world.txt");
        fs::write(&doc, b"rustyos")?;
        let trashed_doc = move_to_trash(&doc, &trash)?;
        assert!(!doc.exists());
        assert!(trashed_doc.exists());

        let trash_items = list_trash_entries(&trash)?;
        assert_eq!(trash_items.len(), 1);
        assert_eq!(trash_items[0].name, "hello world.txt");
        assert_eq!(trash_items[0].trash_original_path.as_ref(), Some(&doc));

        let restored = restore_from_trash("hello world.txt", &trash)?;
        assert_eq!(restored, doc);
        assert_eq!(fs::read(&doc)?, b"rustyos");
        assert_eq!(list_trash_entries(&trash)?, []);

        move_to_trash(&f2, &trash)?;
        assert_eq!(list_trash_entries(&trash)?.len(), 1);
        let deleted = empty_trash(&trash)?;
        assert_eq!(deleted, 1);
        assert_eq!(list_trash_entries(&trash)?, []);
        assert!(trash.exists());
        Ok(())
    }

    #[test]
    fn sort_filter_and_formatters_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("sort-fmt")?;
        fs::create_dir_all(guard.path.join("alpha_dir"))?;
        fs::write(guard.path.join(".secret"), b"hi")?;
        fs::write(guard.path.join("beta.rs"), b"1234567890")?;
        fs::write(guard.path.join("Alpha.txt"), b"123")?;

        let raw = list_directory_at(&guard.path)?;
        let visible_name = filter_and_sort_entries(
            raw.clone(),
            false,
            "",
            FileSortField::Name,
            FileSortOrder::Ascending,
        );
        let names: Vec<&str> = visible_name.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha_dir", "Alpha.txt", "beta.rs"]);

        let filtered = filter_and_sort_entries(
            raw,
            true,
            "sec",
            FileSortField::Size,
            FileSortOrder::Descending,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, ".secret");

        assert_eq!(format_byte_size(512), "512 B");
        assert_eq!(format_byte_size(1_500), "1.5 KB");
        assert_eq!(format_byte_size(2_400_000), "2.4 MB");
        assert_eq!(format_unix_timestamp(0), "1970-01-01 00:00");
        assert_eq!(format_unix_timestamp(1_763_114_400), "2025-11-14 10:00");
        Ok(())
    }

    #[test]
    fn stage_b_view_modes_roundtrip_in_tags_config() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("view-modes-stage-b")?;
        let defaults = FileTagsConfig::default();
        assert_eq!(defaults.view_mode, FileViewMode::Icons);
        assert!(!defaults.show_path_bar);
        for mode in [
            FileViewMode::Icons,
            FileViewMode::List,
            FileViewMode::Columns,
            FileViewMode::Gallery,
        ] {
            assert!(mode.is_supported());
            let cfg = FileTagsConfig {
                view_mode: mode,
                show_path_bar: mode == FileViewMode::Icons,
                ..Default::default()
            };
            let cfg_path = guard.path.join(format!("{}.toml", mode.as_config_str()));
            cfg.save_to_path(&cfg_path)?;
            let loaded = FileTagsConfig::load_from_path(&cfg_path);
            assert_eq!(loaded.view_mode, mode);
            assert_eq!(loaded.show_path_bar, mode == FileViewMode::Icons);
        }
        let sub = guard.path.join("nested");
        fs::create_dir_all(&sub)?;
        fs::write(sub.join("needle_doc.txt"), b"ok")?;
        let found = search_directory_recursive(&guard.path, "needle", false, 16);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "needle_doc.txt");
        Ok(())
    }

    #[test]
    fn inspect_entry_info_and_text_preview_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("inspect-preview")?;
        let home = guard.path.join("home/tester");
        let docs = home.join("Documents");
        fs::create_dir_all(&docs)?;
        let locs =
            UserFileLocations::with_roots(&home, &home.join(".config"), &home.join(".local/share"));

        let text_file = docs.join("notes.md");
        fs::write(&text_file, b"Line 1\nLine 2\nLine 3\n")?;
        let ppm_file = docs.join("sample.ppm");
        fs::write(&ppm_file, b"P6\n2 3\n255\n\x10\x20\x30\x40\x50\x60\x10\x20\x30\x40\x50\x60\x10\x20\x30\x40\x50\x60")?;
        let bin_file = docs.join("blob.bin");
        fs::write(&bin_file, b"\x00\xFF\x00\x01binary")?;

        let dir_info = inspect_entry_info(&docs, &locs)?;
        assert_eq!(dir_info.kind_label, "Folder");
        assert_eq!(dir_info.item_count, Some(3));
        assert_eq!(dir_info.where_parent, "~");
        assert!(dir_info.size_bytes >= 30);

        let img_info = inspect_entry_info(&ppm_file, &locs)?;
        assert_eq!(img_info.dimensions, Some((2, 3)));
        assert_eq!(img_info.where_parent, "~/Documents");

        let preview = read_text_preview(&text_file, 1024, 2).ok_or("expected text preview")?;
        assert_eq!(preview.lines, vec!["Line 1", "Line 2"]);
        assert!(preview.truncated);
        assert!(read_text_preview(&bin_file, 1024, 10).is_none());
        Ok(())
    }

    #[test]
    fn rename_and_go_to_folder_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("rename-goto")?;
        let home = guard.path.join("home");
        let downloads = home.join("Downloads");
        let docs = home.join("Documents");
        fs::create_dir_all(&downloads)?;
        fs::create_dir_all(&docs)?;
        let locs =
            UserFileLocations::with_roots(&home, &home.join(".config"), &home.join(".local/share"));

        let file_a = docs.join("draft.txt");
        let file_b = docs.join("taken.txt");
        fs::write(&file_a, b"alpha")?;
        fs::write(&file_b, b"beta")?;

        assert!(rename_entry(&file_a, "bad/name").is_err());
        assert!(rename_entry(&file_a, "taken.txt").is_err());
        let renamed = rename_entry(&file_a, "final.txt")?;
        assert_eq!(renamed, docs.join("final.txt"));
        assert!(!file_a.exists());
        assert!(renamed.exists());

        assert_eq!(resolve_go_to_path("~", None, &locs), Some(home.clone()));
        assert_eq!(
            resolve_go_to_path("~/Downloads/../Documents", None, &locs),
            Some(docs.clone())
        );
        let suggestions = suggest_go_to_paths("~/Do", None, &locs, 8);
        assert!(suggestions.contains(&docs));
        assert!(suggestions.contains(&downloads));
        Ok(())
    }

    #[test]
    fn copy_move_conflict_and_cancel_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("transfer-ops")?;
        let src_dir = guard.path.join("src");
        let dst_dir = guard.path.join("dst");
        fs::create_dir_all(src_dir.join("folder"))?;
        fs::create_dir_all(&dst_dir)?;

        let f_alpha = src_dir.join("alpha.txt");
        fs::write(&f_alpha, b"new-alpha")?;
        fs::write(src_dir.join("folder/nested.txt"), b"nested-data")?;
        let cancel = AtomicBool::new(false);

        // Same-folder copy automatically chooses "<name> copy.<ext>".
        let mut snapshots = Vec::new();
        let dup = copy_or_move_with_progress(
            std::slice::from_ref(&f_alpha),
            &src_dir,
            TransferKind::Copy,
            &cancel,
            |p| snapshots.push(p.completed_bytes),
            |_, _| TransferConflictDecision::Stop,
        )?;
        assert_eq!(dup.created_paths, vec![src_dir.join("alpha copy.txt")]);
        assert!(!dup.cancelled);
        assert!(snapshots.last().copied().unwrap_or(0) >= 9);

        // Cross-directory copy with KeepBoth, Replace, Skip, Stop.
        fs::write(dst_dir.join("alpha.txt"), b"existing-alpha")?;
        let keep_both = copy_or_move_with_progress(
            std::slice::from_ref(&f_alpha),
            &dst_dir,
            TransferKind::Copy,
            &cancel,
            |_| {},
            |_, _| TransferConflictDecision::KeepBoth,
        )?;
        assert_eq!(
            keep_both.created_paths,
            vec![dst_dir.join("alpha copy.txt")]
        );
        assert_eq!(fs::read(dst_dir.join("alpha.txt"))?, b"existing-alpha");

        let replaced = copy_or_move_with_progress(
            std::slice::from_ref(&f_alpha),
            &dst_dir,
            TransferKind::Copy,
            &cancel,
            |_| {},
            |_, _| TransferConflictDecision::Replace,
        )?;
        assert_eq!(replaced.created_paths, vec![dst_dir.join("alpha.txt")]);
        assert_eq!(fs::read(dst_dir.join("alpha.txt"))?, b"new-alpha");

        let moved = copy_or_move_with_progress(
            &[src_dir.join("folder")],
            &dst_dir,
            TransferKind::Move,
            &cancel,
            |_| {},
            |_, _| TransferConflictDecision::Stop,
        )?;
        assert_eq!(moved.created_paths, vec![dst_dir.join("folder")]);
        assert!(!src_dir.join("folder").exists());
        assert_eq!(fs::read(dst_dir.join("folder/nested.txt"))?, b"nested-data");

        // Self-nested copy guard.
        let nested_err = copy_or_move_with_progress(
            &[dst_dir.join("folder")],
            &dst_dir.join("folder"),
            TransferKind::Copy,
            &cancel,
            |_| {},
            |_, _| TransferConflictDecision::Stop,
        );
        assert!(nested_err.is_err());

        // Pre-cancelled flag stops immediately.
        let stopped_cancel = AtomicBool::new(true);
        let aborted = copy_or_move_with_progress(
            std::slice::from_ref(&f_alpha),
            &dst_dir,
            TransferKind::Copy,
            &stopped_cancel,
            |_| {},
            |_, _| TransferConflictDecision::KeepBoth,
        )?;
        assert!(aborted.cancelled);
        assert_eq!(aborted.created_paths, Vec::<PathBuf>::new());
        Ok(())
    }
}
