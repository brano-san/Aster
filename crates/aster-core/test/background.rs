use super::*;
use aster_gfx::render::Rect;

#[test]
fn cover_and_contain_center_the_original_aspect_ratio() {
    let pane = Rect::new(10.0, 20.0, 200.0, 100.0);
    assert_eq!(placement(pane, [50, 50], Fit::Cover).unwrap().rect, Rect::new(10.0, -30.0, 200.0, 200.0));
    assert_eq!(placement(pane, [50, 50], Fit::Contain).unwrap().rect, Rect::new(60.0, 20.0, 100.0, 100.0));
    assert_eq!(placement(pane, [200, 50], Fit::Contain).unwrap().rect, Rect::new(10.0, 45.0, 200.0, 50.0));
}

#[test]
fn stretch_fills_the_pane_and_tile_repeats_at_the_original_pixel_size() {
    let pane = Rect::new(10.0, 20.0, 200.0, 100.0);
    let stretch = placement(pane, [8, 25], Fit::Stretch).unwrap();
    let tile = placement(pane, [8, 25], Fit::Tile).unwrap();
    assert_eq!(stretch.rect, pane);
    assert_eq!(stretch.tile, None);
    assert_eq!(tile.rect, pane);
    assert_eq!(tile.tile, Some([8.0, 25.0]));
    assert!(placement(pane, [0, 10], Fit::Tile).is_none());
    assert!(placement(Rect::default(), [10, 10], Fit::Cover).is_none());
}

#[test]
fn changing_background_paths_discards_late_results() {
    let mut config = crate::config::Config::default();
    config.appearance.background_image = "first.png".into();
    let mut app = App::new(config.clone(), Vec::new(), true, None, std::sync::Arc::new(|| {}));
    let request = |app: &mut App| app.take_effects().into_iter().find_map(|effect| match effect {
        Effect::LoadBackgroundImage { revision, path } => Some((revision, path)),
        _ => None,
    }).unwrap();
    let (first, path) = request(&mut app);
    assert_eq!(path, "first.png");
    config.appearance.background_image = "second.jpg".into();
    app.set_config(config.clone());
    let (second, path) = request(&mut app);
    assert_eq!(path, "second.jpg");
    let bitmap = || Bitmap { source_size: [1, 1], size: [1, 1], rgba: vec![48, 82, 150, 255] };
    app.background_image_loaded(first, Some(bitmap()));
    assert!(app.background.bitmap.is_none());
    app.background_image_loaded(second, Some(bitmap()));
    assert!(app.background.bitmap.is_some());
    config.appearance.background_image.clear();
    app.set_config(config);
    app.background_image_loaded(second, None);
    assert!(app.background.bitmap.is_none());
    assert!(!app.background.failed);
}
