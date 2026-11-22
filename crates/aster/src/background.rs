use aster_core::background::Bitmap;
use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::PathBuf;

const ATLAS_EDGE: u32 = 1024;
const DECODE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Copy)]
enum Pixels {
    Gray,
    GrayAlpha,
    Gray16(u8),
    Rgb,
    Rgba,
    Cmyk,
}

impl Pixels {
    fn channels(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::GrayAlpha | Self::Gray16(_) => 2,
            Self::Rgb => 3,
            Self::Rgba | Self::Cmyk => 4,
        }
    }

    fn rgba(self, p: &[u8]) -> [u8; 4] {
        let mul = |v, a| ((u32::from(v) * u32::from(a) + 127) / 255) as u8;
        match self {
            Self::Gray => [p[0], p[0], p[0], 255],
            Self::GrayAlpha => [mul(p[0], p[1]), mul(p[0], p[1]), mul(p[0], p[1]), p[1]],
            Self::Gray16(precision) => {
                let maximum = (1_u32 << precision) - 1;
                let sample = u32::from(u16::from_ne_bytes([p[0], p[1]])).min(maximum);
                let gray = ((sample * 255 + maximum / 2) / maximum) as u8;
                [gray, gray, gray, 255]
            }
            Self::Rgb => [p[0], p[1], p[2], 255],
            Self::Rgba => [mul(p[0], p[3]), mul(p[1], p[3]), mul(p[2], p[3]), p[3]],
            Self::Cmyk => [mul(255 - p[0], 255 - p[3]), mul(255 - p[1], 255 - p[3]), mul(255 - p[2], 255 - p[3]), 255],
        }
    }
}

fn checked_size(size: [u32; 2]) -> Result<(), Box<dyn Error>> {
    let [w, h] = size.map(u64::from);
    if w == 0 || h == 0 || w * h > DECODE_BYTES as u64 / 4 {
        return Err("unsupported background image dimensions".into());
    }
    Ok(())
}

fn atlas_size(size: [u32; 2]) -> [u32; 2] {
    let largest = size[0].max(size[1]);
    if largest <= ATLAS_EDGE {
        return size;
    }
    size.map(|n| ((u64::from(n) * u64::from(ATLAS_EDGE) + u64::from(largest / 2)) / u64::from(largest)).max(1) as u32)
}

fn bitmap(source_size: [u32; 2], decoded: [u32; 2], pixels: &[u8], format: Pixels) -> Result<Bitmap, Box<dyn Error>> {
    checked_size(decoded)?;
    let [w, h] = decoded.map(|n| n as usize);
    let stride = format.channels();
    if pixels.len() != w * h * stride {
        return Err("invalid background image data length".into());
    }
    let size = atlas_size(source_size);
    let [width, height] = size.map(|n| n as usize);
    let mut rgba = Vec::with_capacity(width * height * 4);
    if size == decoded {
        for pixel in pixels.chunks_exact(stride) {
            rgba.extend_from_slice(&format.rgba(pixel));
        }
    } else {
        let at = |x: usize, y: usize| {
            let start = (y * w + x) * stride;
            format.rgba(&pixels[start..start + stride])
        };
        let axis = |dst: usize, source: usize, target: usize| {
            let coord = ((dst as f32 + 0.5) * source as f32 / target as f32 - 0.5).clamp(0.0, (source - 1) as f32);
            let low = coord as usize;
            (low, (low + 1).min(source - 1), coord - low as f32)
        };
        let blend = |a: f32, b: f32, t| a + (b - a) * t;
        for y in 0..height {
            let (y0, y1, fy) = axis(y, h, height);
            for x in 0..width {
                let (x0, x1, fx) = axis(x, w, width);
                let [a, b, c, d] = [at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1)].map(|pixel| pixel.map(f32::from));
                let pixel: [u8; 4] = std::array::from_fn(|i| blend(blend(a[i], b[i], fx), blend(c[i], d[i], fx), fy).round() as u8);
                rgba.extend_from_slice(&pixel);
            }
        }
    }
    Ok(Bitmap { source_size, size, rgba })
}

fn png(reader: impl Read) -> Result<Bitmap, Box<dyn Error>> {
    let mut decoder = png::Decoder::new(reader);
    decoder.set_limits(png::Limits { bytes: DECODE_BYTES });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    checked_size([reader.info().width, reader.info().height])?;
    let mut data = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut data)?;
    let format = match frame.color_type {
        png::ColorType::Grayscale => Pixels::Gray,
        png::ColorType::GrayscaleAlpha => Pixels::GrayAlpha,
        png::ColorType::Rgb => Pixels::Rgb,
        png::ColorType::Rgba => Pixels::Rgba,
        png::ColorType::Indexed => return Err("unexpanded PNG palette".into()),
    };
    let size = [frame.width, frame.height];
    bitmap(size, size, &data[..frame.buffer_size()], format)
}

fn jpeg_precision(reader: &mut (impl Read + Seek)) -> io::Result<u8> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid JPEG sample precision");
    reader.seek(SeekFrom::Start(2))?;
    let mut byte = [0];
    loop {
        reader.read_exact(&mut byte)?;
        if byte[0] != 0xFF {
            return Err(invalid());
        }
        while byte[0] == 0xFF {
            reader.read_exact(&mut byte)?;
        }
        let marker = byte[0];
        if matches!(marker, 0x00 | 0xD9 | 0xDA) {
            return Err(invalid());
        }
        if marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            continue;
        }
        let mut length = [0; 2];
        reader.read_exact(&mut length)?;
        let length = u16::from_be_bytes(length).checked_sub(2).ok_or_else(invalid)?;
        if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            if length == 0 {
                return Err(invalid());
            }
            reader.read_exact(&mut byte)?;
            return (9..=16).contains(&byte[0]).then_some(byte[0]).ok_or_else(invalid);
        }
        reader.seek(SeekFrom::Current(i64::from(length)))?;
    }
}

fn jpeg(mut reader: impl Read + Seek) -> Result<Bitmap, Box<dyn Error>> {
    let mut decoder = jpeg_decoder::Decoder::new(&mut reader);
    decoder.set_max_decoding_buffer_size(DECODE_BYTES);
    decoder.read_info()?;
    let info = decoder.info().ok_or("missing JPEG dimensions")?;
    let size = [u32::from(info.width), u32::from(info.height)];
    checked_size(size)?;
    let format = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => Pixels::Gray,
        jpeg_decoder::PixelFormat::L16 => {
            drop(decoder);
            let precision = jpeg_precision(&mut reader)?;
            reader.rewind()?;
            decoder = jpeg_decoder::Decoder::new(&mut reader);
            decoder.set_max_decoding_buffer_size(DECODE_BYTES);
            Pixels::Gray16(precision)
        }
        jpeg_decoder::PixelFormat::RGB24 => Pixels::Rgb,
        jpeg_decoder::PixelFormat::CMYK32 => Pixels::Cmyk,
    };
    let data = decoder.decode()?;
    bitmap(size, size, &data, format)
}

fn decode(mut reader: impl BufRead + Seek) -> Result<Bitmap, Box<dyn Error>> {
    let signature = reader.fill_buf()?;
    if signature.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(reader)
    } else if signature.starts_with(&[0xFF, 0xD8]) {
        jpeg(reader)
    } else {
        Err("background image must be PNG or JPEG".into())
    }
}

pub fn request(path: PathBuf, reply: impl FnOnce(Option<Bitmap>) + Send + 'static) -> io::Result<()> {
    std::thread::Builder::new().name("aster-background-image".into()).spawn(move || {
        let result = (|| decode(BufReader::new(File::open(&path)?)))();
        match result {
            Ok(bitmap) => reply(Some(bitmap)),
            Err(error) => {
                tracing::warn!("background image: {error}");
                reply(None);
            }
        }
    }).map(drop)
}

#[cfg(test)]
#[path = "../test/background.rs"]
mod tests;
