pub mod atlas;
pub mod hash;
pub mod render;
pub mod text;

#[derive(Clone, Debug, Default)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
