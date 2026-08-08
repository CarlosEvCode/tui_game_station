use image::imageops::FilterType;
use image::DynamicImage;
use image::ImageReader;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::path::Path;

const MIN_SOURCE_DIM: u32 = 256;
/// Maximum dimension (px) for halfblocks *carousel* thumbnails (side cards and
/// grid cells). Kept low so the halfblocks encoder stays cheap. Banners and
/// native-protocol covers are NOT clamped here.
const MAX_HALFBLOCKS_DIM: u32 = 640;
/// Minimum width (px) for the full-width hero banner so that Resize::Crop can
/// fill any reasonably wide terminal without letterboxing. 1920px covers up to
/// a 240-column terminal at 8px-per-cell font.
const BANNER_SOURCE_WIDTH: u32 = 1920;

#[derive(Clone)]
pub struct CoverManager {
    pub picker: Picker,
    pub halfblocks_picker: Picker,
}

impl CoverManager {
    pub fn new() -> Self {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::from_fontsize((8, 16)));
        let halfblocks_picker = Picker::from_fontsize((8, 16));

        Self {
            picker,
            halfblocks_picker,
        }
    }

    /// Full-quality native-protocol image (Kitty / Sixel) — no resolution cap,
    /// the protocol itself handles the render-time scaling.
    pub fn load_native_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        if !path.exists() {
            return None;
        }

        let dyn_img = Self::decode_image(path)?;
        let mut picker = self.picker;
        Some(picker.new_resize_protocol(dyn_img))
    }

    /// Halfblocks (unicode half-block) thumbnail for carousel side cards and
    /// grid cells. Clamped to MAX_HALFBLOCKS_DIM to keep encoding cheap.
    pub fn load_halfblocks_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        if !path.exists() {
            return None;
        }

        let dyn_img = Self::decode_image(path)?;
        // Clamp only for small thumbnails to keep CPU/RAM low.
        let dyn_img = Self::clamp_max_resolution(dyn_img, MAX_HALFBLOCKS_DIM);
        let mut picker = self.halfblocks_picker;
        Some(picker.new_resize_protocol(dyn_img))
    }

    /// Halfblocks protocol for full-width hero banners. The source is pre-scaled
    /// wide so the renderer can crop (cover) it across the entire hero instead of
    /// letterboxing it into a fraction of the width (Fit never upscales).
    /// NOT clamped — we need full width so Resize::Crop fills the terminal.
    pub fn load_halfblocks_banner_protocol_from_file(
        &self,
        path: &Path,
    ) -> Option<StatefulProtocol> {
        if !path.exists() {
            return None;
        }

        let dyn_img = Self::decode_image(path)?;
        // Ensure the banner is at least BANNER_SOURCE_WIDTH pixels wide so Crop
        // mode can fill any terminal without letterboxing.
        let dyn_img = Self::ensure_min_width(dyn_img, BANNER_SOURCE_WIDTH);
        let mut picker = self.halfblocks_picker;
        Some(picker.new_resize_protocol(dyn_img))
    }

    /// Decode an image file, upscaling tiny sources to MIN_SOURCE_DIM so the
    /// ratatui-image renderer always has something visible to work with.
    /// No maximum-resolution cap is applied here; callers that need a cap
    /// (e.g. halfblocks thumbnails) apply `clamp_max_resolution` themselves.
    fn decode_image(path: &Path) -> Option<DynamicImage> {
        let dyn_img: DynamicImage = ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?
            .decode()
            .ok()?;
        Some(Self::ensure_min_resolution(dyn_img))
    }

    /// Downscale the image if its largest dimension exceeds `max_allowed` px.
    /// Uses Triangle (bilinear) for good quality at low cost.
    fn clamp_max_resolution(img: DynamicImage, max_allowed: u32) -> DynamicImage {
        let (w, h) = (img.width(), img.height());
        let max_dim = w.max(h);
        if max_dim <= max_allowed {
            return img;
        }
        let scale = max_allowed as f32 / max_dim as f32;
        let new_w = (w as f32 * scale).round().max(1.0) as u32;
        let new_h = (h as f32 * scale).round().max(1.0) as u32;
        image::imageops::resize(&img, new_w, new_h, FilterType::Triangle).into()
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
        image::imageops::resize(&img, target_width, new_h, FilterType::Lanczos3).into()
    }

    pub fn load_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        self.load_native_protocol_from_file(path)
    }
}
