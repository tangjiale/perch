fn main() {
    // Tauri 会再次复制资源；JRE 的只读许可文件需要允许覆盖构建缓存。
    if let Some(profile) = std::env::var_os("OUT_DIR")
        .map(std::path::PathBuf::from)
        .and_then(|p| p.ancestors().nth(3).map(std::path::Path::to_path_buf))
    {
        prepare_resource_cache(&profile.join("resources/parser")).expect("无法准备解析器构建缓存");
    }
    tauri_build::build()
}

fn prepare_resource_cache(path: &std::path::Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            prepare_resource_cache(&entry.path())?;
        } else if kind.is_file() {
            let mut permissions = entry.metadata()?.permissions();
            if permissions.readonly() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    permissions.set_mode(permissions.mode() | 0o200);
                }
                #[cfg(not(unix))]
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                std::fs::set_permissions(entry.path(), permissions)?;
            }
        }
    }
    Ok(())
}
