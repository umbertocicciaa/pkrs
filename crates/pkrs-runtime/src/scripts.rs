//! [`ScriptFiles`] over the assets VFS (§9.6, §13): scripts are read through the same
//! [`Vfs`] as every other project file, never from the OS directly.

use pkrs_assets::{Vfs, VfsPath};
use pkrs_script::{ScriptError, ScriptFiles};

/// Serves project scripts (`scripts/…`, `plugins/<id>/…`) from a [`Vfs`].
pub struct VfsScriptFiles<V> {
    vfs: V,
}

impl<V: Vfs> VfsScriptFiles<V> {
    /// Wraps `vfs` (usually a `FolderVfs` on the project root, or the `.pak`).
    pub fn new(vfs: V) -> Self {
        Self { vfs }
    }
}

impl<V: Vfs> ScriptFiles for VfsScriptFiles<V> {
    fn read_script(&self, path: &str) -> pkrs_script::Result<String> {
        let load_error = |reason: String| ScriptError::Load {
            path: path.to_owned(),
            reason,
        };
        let vfs_path = VfsPath::new(path).map_err(|e| load_error(e.to_string()))?;
        let bytes = self
            .vfs
            .read(&vfs_path)
            .map_err(|e| load_error(e.to_string()))?;
        String::from_utf8(bytes).map_err(|_| load_error("script is not valid UTF-8".into()))
    }

    fn exists(&self, path: &str) -> bool {
        VfsPath::new(path).is_ok_and(|p| self.vfs.exists(&p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_assets::MemoryVfs;

    #[test]
    fn reads_through_the_vfs() {
        let vfs = MemoryVfs::new()
            .with("scripts/main.lua", "return 1")
            .unwrap()
            .with("scripts/bad.lua", vec![0xff, 0xfe])
            .unwrap();
        let files = VfsScriptFiles::new(vfs);
        assert!(files.exists("scripts/main.lua"));
        assert!(!files.exists("scripts/none.lua"));
        assert!(!files.exists("../escape.lua"));
        assert_eq!(files.read_script("scripts/main.lua").unwrap(), "return 1");
        assert!(matches!(
            files.read_script("scripts/bad.lua"),
            Err(ScriptError::Load { .. })
        ));
        assert!(matches!(
            files.read_script("../outside.lua"),
            Err(ScriptError::Load { .. })
        ));
    }
}
