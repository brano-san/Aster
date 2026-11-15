use super::*;

fn at(mask: &[u8], w: u32, x: u32, y: u32) -> u8 {
    mask[(y * w + x) as usize]
}

#[test]
fn light_lines_reach_every_edge_they_name_and_touch_no_other() {
    let (w, h) = (8, 16);
    let cross = rasterize('┼', w, h).unwrap();
    assert_eq!(at(&cross, w, 4, 0), 255);
    assert_eq!(at(&cross, w, 4, h - 1), 255);
    assert_eq!(at(&cross, w, 0, 8), 255);
    assert_eq!(at(&cross, w, w - 1, 8), 255);
    assert_eq!(at(&cross, w, 0, 0), 0);
    let corner = rasterize('┌', w, h).unwrap();
    assert_eq!(at(&corner, w, w - 1, 8), 255);
    assert_eq!(at(&corner, w, 4, h - 1), 255);
    assert_eq!(at(&corner, w, 0, 8), 0);
    assert_eq!(at(&corner, w, 4, 0), 0);
}

#[test]
fn heavy_lines_are_thicker_than_light_ones() {
    let (w, h) = (16, 32);
    let count = |c: char| rasterize(c, w, h).unwrap().iter().filter(|&&v| v == 255).count();
    assert!(count('━') > count('─'));
    assert_eq!(count('─'), (w * 2) as usize);
}

#[test]
fn blocks_shades_and_quadrants() {
    let (w, h) = (8, 16);
    assert!(rasterize('█', w, h).unwrap().iter().all(|&v| v == 255));
    let lower = rasterize('▄', w, h).unwrap();
    assert_eq!((at(&lower, w, 0, 0), at(&lower, w, 0, 15)), (0, 255));
    assert!(rasterize('▒', w, h).unwrap().iter().all(|&v| v == 128));
    let q = rasterize('▚', w, h).unwrap();
    assert_eq!((at(&q, w, 0, 0), at(&q, w, 7, 0), at(&q, w, 7, 15)), (255, 0, 255));
}

#[test]
fn powerline_triangle_points_right() {
    let (w, h) = (10, 20);
    let t = rasterize('\u{E0B0}', w, h).unwrap();
    assert_eq!(at(&t, w, 0, 10), 255);
    assert!(at(&t, w, 9, 10) > 0);
    assert_eq!(at(&t, w, 9, 0), 0);
}

#[test]
fn double_lines_and_other_chars_fall_back_to_the_font() {
    assert!(!is_procedural('═'));
    assert!(rasterize('╔', 8, 16).is_none());
    assert!(rasterize('a', 8, 16).is_none());
    assert!(is_procedural('╭') && is_procedural('╱') && is_procedural('░'));
    assert!(rasterize('╭', 8, 16).unwrap().iter().any(|&v| v > 0));
}
