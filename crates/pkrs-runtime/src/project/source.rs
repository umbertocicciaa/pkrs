//! Where a project's files come from: the loose folder (`pkrs run`) or a `game.pak` release
//! archive (§13, §14.1). Both serve the same paths, so the game cannot tell them apart;
//! images packed into atlases (§5.2) are loaded through [`load_image`].

use pkrs_assets::{FolderVfs, PakIndex, PakVfs, Vfs, VfsEntry, VfsPath};
use pkrs_render::{Renderer, TextureHandle};

use super::ProjectError;

/// A project's read-only files.
#[derive(Clone, Debug)]
pub enum ProjectVfs {
    /// Loose files in the project folder.
    Folder(FolderVfs),
    /// A `.pak` release archive.
    Pak(PakVfs),
}

impl ProjectVfs {
    /// The archive's index when running from a `.pak`.
    #[must_use]
    pub fn pak_index(&self) -> Option<&PakIndex> {
        match self {
            Self::Folder(_) => None,
            Self::Pak(pak) => Some(pak.index()),
        }
    }

    /// Whether an image exists at `path`, as a file or packed in an atlas.
    #[must_use]
    pub fn image_exists(&self, path: &VfsPath) -> bool {
        self.exists(path)
            || self
                .pak_index()
                .is_some_and(|index| index.atlas_rect(path).is_some())
    }
}

impl Vfs for ProjectVfs {
    fn read(&self, path: &VfsPath) -> pkrs_assets::Result<Vec<u8>> {
        match self {
            Self::Folder(vfs) => vfs.read(path),
            Self::Pak(vfs) => vfs.read(path),
        }
    }

    fn exists(&self, path: &VfsPath) -> bool {
        match self {
            Self::Folder(vfs) => vfs.exists(path),
            Self::Pak(vfs) => vfs.exists(path),
        }
    }

    fn list(&self, dir: &VfsPath) -> pkrs_assets::Result<Vec<VfsEntry>> {
        match self {
            Self::Folder(vfs) => vfs.list(dir),
            Self::Pak(vfs) => vfs.list(dir),
        }
    }
}

/// Uploads the image at `path` and returns its texture and size.
///
/// From a `.pak`, a packed image becomes a region of its atlas page (each page is uploaded
/// once per renderer), which draws exactly like the image itself; other images are read and
/// uploaded as files.
pub fn load_image(
    vfs: &ProjectVfs,
    renderer: &mut Renderer,
    path: &VfsPath,
) -> Result<(TextureHandle, u32, u32), ProjectError> {
    if let Some(found) = vfs.pak_index().and_then(|index| index.atlas_rect(path)) {
        let page = match renderer.named_texture(&found.page.path) {
            Some(page) => page,
            None => {
                let bytes = vfs.read(&VfsPath::new(&found.page.path)?)?;
                renderer.load_named_texture(&found.page.path, &bytes)?
            }
        };
        let r = found.rect;
        let region = renderer.create_texture_region(page.0, r.x, r.y, r.width, r.height)?;
        return Ok((region, r.width, r.height));
    }
    let bytes = vfs.read(path)?;
    Ok(renderer.load_texture(&bytes)?)
}
