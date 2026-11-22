use super::*;
use std::io::Cursor;

#[test]
fn png_background_decodes_premultiplied_pixels() {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[200, 20, 0, 128, 7, 8, 9, 255]).unwrap();
    }
    let bitmap = decode(Cursor::new(bytes)).unwrap();
    assert_eq!(bitmap.size, [2, 1]);
    assert_eq!(bitmap.source_size, [2, 1]);
    assert_eq!(bitmap.rgba, [100, 10, 0, 128, 7, 8, 9, 255]);
}

#[test]
fn lossless_jpeg_gray_samples_use_the_frame_precision() {
    let headers = [0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC3, 0, 3, 12];
    let precision = jpeg_precision(&mut Cursor::new(headers)).unwrap();
    assert_eq!(precision, 12);
    assert_eq!(Pixels::Gray16(precision).rgba(&4095_u16.to_ne_bytes()), [255; 4]);
    assert_eq!(Pixels::Gray16(precision).rgba(&2048_u16.to_ne_bytes()), [128, 128, 128, 255]);
}
