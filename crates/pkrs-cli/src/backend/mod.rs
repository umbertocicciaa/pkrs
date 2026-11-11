//! Vendor adapters of the CLI (§4.5): one module per vendor crate.

mod texture_packer;

pub use self::texture_packer::TexturePackerAdapter;
