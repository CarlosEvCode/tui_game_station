use image::imageops::FilterType;
use image::DynamicImage;
use image::ImageReader;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::path::Path;

const MIN_SOURCE_DIM: u32 = 256;
const BANNER_SOURCE_WIDTH: u32 = 1600;
const ICON_CANVAS: u32 = 256;

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
        let mut picker = self.picker.clone();
        Some(picker.new_resize_protocol(dyn_img))
    }

    pub fn load_halfblocks_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        if !path.exists() {
            return None;
        }

        let dyn_img = Self::decode_image(path)?;
        let mut picker = self.halfblocks_picker.clone();
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
        let mut picker = self.halfblocks_picker.clone();
        Some(picker.new_resize_protocol(dyn_img))
    }

    /// Halfblocks protocol for icons. The source is normalized once to a fixed
    /// square canvas (transparent borders trimmed, then letterboxed) so every
    /// icon renders the "subject" at the same visual scale inside its box,
    /// regardless of the source image's resolution or padding.
    pub fn load_halfblocks_icon_protocol_from_file(&self, path: &Path) -> Option<StatefulProtocol> {
        if !path.exists() {
            return None;
        }

        let dyn_img = Self::decode_image(path)?;
        let dyn_img = Self::normalize_icon(dyn_img);
        let mut picker = self.halfblocks_picker.clone();
        Some(picker.new_resize_protocol(dyn_img))
    }

    fn normalize_icon(img: DynamicImage) -> DynamicImage {
        let trimmed = Self::trim_transparent(&img);
        let (w, h) = (trimmed.width(), trimmed.height());
        let scale = ICON_CANVAS as f32 / w.max(h) as f32;
        let new_w = ((w as f32 * scale).round() as u32).max(1);
        let new_h = ((h as f32 * scale).round() as u32).max(1);
        let scaled = image::imageops::resize(&trimmed, new_w, new_h, FilterType::Nearest);
        let mut canvas = DynamicImage::new_rgba8(ICON_CANVAS, ICON_CANVAS);
        let off_x = ((ICON_CANVAS - new_w) / 2) as i64;
        let off_y = ((ICON_CANVAS - new_h) / 2) as i64;
        image::imageops::overlay(&mut canvas, &scaled, off_x, off_y);
        canvas
    }

    /// Crop fully-transparent borders so the icon's "subject" fills a consistent
    /// area of the canvas instead of varying with source-image padding.
    fn trim_transparent(img: &DynamicImage) -> DynamicImage {
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let mut min_x = w;
        let mut min_y = h;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut found = false;
        for (x, y, p) in rgba.enumerate_pixels() {
            if p[3] > 8 {
                found = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        if !found || (min_x == 0 && min_y == 0 && max_x == w - 1 && max_y == h - 1) {
            return img.clone();
        }
        img.crop_imm(min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
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
