use image::imageops::FilterType;
use image::ImageReader;
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use std::path::Path;

pub fn load_and_render_cover_ansi(
    image_path: &Path,
    target_width: u32,
    target_height: u32,
) -> Option<Vec<Line<'static>>> {
    if !image_path.exists() {
        return None;
    }

    let img = ImageReader::open(image_path).ok()?.decode().ok()?;
    let pixel_height = target_height * 2;
    if target_width == 0 || pixel_height == 0 {
        return None;
    }

    let resized = img.resize_exact(target_width, pixel_height, FilterType::Triangle);
    let rgb = resized.to_rgb8();

    let mut lines = Vec::new();
    for y in (0..resized.height()).step_by(2) {
        let mut spans = Vec::new();
        for x in 0..resized.width() {
            let top_pixel = rgb.get_pixel(x, y);
            let bot_pixel = if y + 1 < resized.height() {
                rgb.get_pixel(x, y + 1)
            } else {
                top_pixel
            };

            let top_color = Color::Rgb(top_pixel[0], top_pixel[1], top_pixel[2]);
            let bot_color = Color::Rgb(bot_pixel[0], bot_pixel[1], bot_pixel[2]);

            spans.push(Span::styled(
                "▄",
                Style::default().bg(top_color).fg(bot_color),
            ));
        }
        lines.push(Line::from(spans));
    }

    Some(lines)
}
