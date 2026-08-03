use image::imageops::FilterType;
use image::DynamicImage;
use image::ImageReader;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::path::Path;

const MIN_SOURCE_DIM: u32 = 256;
const BANNER_SOURCE_WIDTH: u32 = 1600;

#[derive(Clone)]
pub struct CoverManager {
    pub picker: Picker,
    pub halfblocks_picker: Picker,
}

impl CoverManager {
    pub fn new() -> Self {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| {
            Picker::from_fontsize((8, 16))
        });
        let halfblocks_picker = Picker::from_fontsize((8, 16));

        Self { picker, halfblocks_picker }
    }

    pub fn load_native_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        if !path.exists() {
            return None;
        }

        let dyn_img = Self::decode_image(path)?;
        let mut picker = self.picker;
        Some(picker.new_resize_protocol(dyn_img))
    }

    pub fn load_halfblocks_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        if !path.exists() {
            return None;
        }

        let dyn_img = Self::decode_image(path)?;
        let mut picker = self.halfblocks_picker;
        Some(picker.new_resize_protocol(dyn_img))
    }

    /// Halfblocks protocol for full-width hero banners. The source is pre-scaled
    /// wide so the renderer can crop (cover) it across the entire hero instead of
    /// letterboxing it into a fraction of the width (Fit never upscales).
    pub fn load_halfblocks_banner_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        if !path.exists() {
            return None;
        }

        let dyn_img = Self::decode_image(path)?;
        let dyn_img = Self::ensure_min_width(dyn_img, BANNER_SOURCE_WIDTH);
        let mut picker = self.halfblocks_picker;
        Some(picker.new_resize_protocol(dyn_img))
    }

    fn decode_image(path: &Path) -> Option<DynamicImage> {
        let dyn_img: DynamicImage = ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?
            .decode()
            .ok()?;
        Some(Self::ensure_min_resolution(dyn_img))
    }

    fn ensure_min_resolution(img: DynamicImage) -> DynamicImage {
        let (w, h) = (img.width(), img.height());
        let max_dim = w.max(h);
        if max_dim >= MIN_SOURCE_DIM {
            return img;
        }
        let scale = MIN_SOURCE_DIM as f32 / max_dim as f32;
        let new_w = (w as f32 * scale).round().max(1.0) as u32;
        let new_h = (h as f32 * scale).round().max(1.0) as u32;
        image::imageops::resize(&img, new_w, new_h, FilterType::CatmullRom).into()
    }

    fn ensure_min_width(img: DynamicImage, target_width: u32) -> DynamicImage {
        if img.width() >= target_width {
            return img;
        }
        let scale = target_width as f32 / img.width() as f32;
        let new_h = (img.height() as f32 * scale).round().max(1.0) as u32;
        image::imageops::resize(&img, target_width, new_h, FilterType::Nearest).into()
    }

    pub fn load_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        self.load_native_protocol_from_file(path)
    }
}
