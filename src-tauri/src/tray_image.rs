//! The macOS menu bar item drawn as one template image: the app's gauge, then each shown
//! tool's logo and 5-hour percent. Two tools stack in two rows. macOS tints template
//! images for light and dark menu bars, so everything is black with alpha.

use objc2::AnyThread;
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSBitmapImageRep, NSColor, NSDeviceRGBColorSpace, NSFont,
    NSFontAttributeName, NSFontWeightMedium, NSFontWeightSemibold, NSForegroundColorAttributeName,
    NSGraphicsContext, NSImage, NSImageInterpolation,
};
use objc2_foundation::{
    NSData, NSMutableAttributedString, NSPoint, NSRange, NSRect, NSSize, NSString,
};
use token_audit_core::model::Tool;

/// tray-icon shows the image 18pt tall; drawing at 2x keeps it sharp on Retina.
const HEIGHT: f64 = 18.0;
const SCALE: f64 = 2.0;
const GAUGE: &[u8] = include_bytes!("../icons/tray-template.png");

fn logo(tool: Tool) -> &'static [u8] {
    match tool {
        Tool::Claude => include_bytes!("../icons/tray/claude-template.png"),
        Tool::Codex => include_bytes!("../icons/tray/codex-template.png"),
    }
}

/// Logo size, gap to the text, font size and weight for one row of `rows`.
fn metrics(rows: usize) -> (f64, f64, f64, f64) {
    if rows == 1 {
        (12.0, 3.0, 13.0, unsafe { NSFontWeightMedium })
    } else {
        (8.5, 2.0, 9.0, unsafe { NSFontWeightSemibold })
    }
}

fn text(value: &str, font: &NSFont) -> objc2::rc::Retained<NSMutableAttributedString> {
    let string = NSString::from_str(value);
    let text =
        NSMutableAttributedString::initWithString(NSMutableAttributedString::alloc(), &string);
    let range = NSRange::new(0, string.length());
    unsafe {
        text.addAttribute_value_range(NSFontAttributeName, font, range);
        text.addAttribute_value_range(
            NSForegroundColorAttributeName,
            &NSColor::blackColor(),
            range,
        );
    }
    text
}

fn image(bytes: &[u8]) -> Option<objc2::rc::Retained<NSImage>> {
    NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))
}

/// RGBA pixels with their width and height, or `None` for an empty selection, which
/// keeps the plain gauge icon.
pub fn render(badges: &[(Tool, String)]) -> Option<(Vec<u8>, u32, u32)> {
    if badges.is_empty() {
        return None;
    }
    let (logo_size, gap, font_size, weight) = metrics(badges.len());
    let font = NSFont::monospacedDigitSystemFontOfSize_weight(font_size, weight);
    let texts: Vec<_> = badges.iter().map(|(_, value)| text(value, &font)).collect();
    // Digits share one width, so reserving two of them keeps the item from shifting
    // as the number changes; only 100% or a mark widens it.
    let column = texts
        .iter()
        .map(|text| text.size().width)
        .fold(text("00%", &font).size().width, f64::max)
        .ceil();
    let content_x = HEIGHT + 4.0;
    let width = content_x + logo_size + gap + column;
    let (pixels_wide, pixels_high) = ((width * SCALE).ceil() as isize, (HEIGHT * SCALE) as isize);
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            pixels_wide,
            pixels_high,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            pixels_wide * 4,
            32,
        )
    }?;
    // Drawing coordinates are in points from here on.
    rep.setSize(NSSize::new(pixels_wide as f64 / SCALE, HEIGHT));
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    context.setImageInterpolation(NSImageInterpolation::High);
    if let Some(gauge) = image(GAUGE) {
        gauge.drawInRect(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(HEIGHT, HEIGHT),
        ));
    }
    let row_height = HEIGHT / badges.len() as f64;
    for (index, ((tool, _), text)) in badges.iter().zip(&texts).enumerate() {
        // AppKit's origin is bottom left; the first badge takes the top row.
        let bottom = HEIGHT - row_height * (index + 1) as f64;
        if let Some(logo) = image(logo(*tool)) {
            let y = bottom + (row_height - logo_size) / 2.0;
            logo.drawInRect(NSRect::new(
                NSPoint::new(content_x, y),
                NSSize::new(logo_size, logo_size),
            ));
        }
        // Center the digits' cap height in the row; drawAtPoint places the descender
        // at the point, so the baseline sits `-descender` above it.
        let baseline = bottom + (row_height - font.capHeight()) / 2.0;
        let x = content_x + logo_size + gap;
        text.drawAtPoint(NSPoint::new(x, baseline + font.descender()));
    }
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    let length = (rep.bytesPerRow() * pixels_high) as usize;
    let data = rep.bitmapData();
    if data.is_null() {
        return None;
    }
    let mut rgba = unsafe { std::slice::from_raw_parts(data, length) }.to_vec();
    // The bitmap is premultiplied; black stays black, but tray-icon expects straight alpha.
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = pixel[3] as u32;
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    Some((rgba, pixels_wide as u32, pixels_high as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes each case to `TRAY_IMAGE_DIR` when set, for looking at the result.
    #[test]
    fn renders_one_and_two_rows_at_a_stable_width() {
        assert!(render(&[]).is_none());
        let cases = [
            ("one", vec![(Tool::Claude, "12%".to_owned())]),
            ("one-low", vec![(Tool::Claude, "8%".to_owned())]),
            ("one-full", vec![(Tool::Claude, "100%".to_owned())]),
            (
                "two",
                vec![
                    (Tool::Claude, "12%".to_owned()),
                    (Tool::Codex, "0%".to_owned()),
                ],
            ),
            (
                "two-marked",
                vec![
                    (Tool::Claude, "≈82%!".to_owned()),
                    (Tool::Codex, "100%".to_owned()),
                ],
            ),
        ];
        let mut widths = Vec::new();
        for (name, badges) in cases {
            let (rgba, width, height) = render(&badges).expect("rendered");
            assert_eq!(height, 36);
            assert_eq!(rgba.len(), (width * height * 4) as usize);
            assert!(rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0));
            widths.push(width);
            if let Some(dir) = std::env::var_os("TRAY_IMAGE_DIR") {
                let image = tauri::image::Image::new_owned(rgba, width, height);
                write_png(
                    &std::path::Path::new(&dir).join(format!("{name}.png")),
                    &image,
                );
            }
        }
        // Same width for 12% and 8%, so the item does not jump.
        assert_eq!(widths[0], widths[1]);
    }

    fn write_png(path: &std::path::Path, image: &tauri::image::Image<'_>) {
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                image.width() as isize,
                image.height() as isize,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                image.width() as isize * 4,
                32,
            )
        }
        .expect("bitmap");
        unsafe {
            std::ptr::copy_nonoverlapping(
                image.rgba().as_ptr(),
                rep.bitmapData(),
                image.rgba().len(),
            );
        }
        let png = unsafe {
            rep.representationUsingType_properties(
                objc2_app_kit::NSBitmapImageFileType::PNG,
                &objc2_foundation::NSDictionary::new(),
            )
        }
        .expect("png");
        std::fs::write(path, png.to_vec()).expect("write png");
    }
}
