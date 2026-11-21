use super::*;

const CTRL: Mods = Mods { ctrl: true, ..Mods::NONE };
const SHIFT: Mods = Mods { shift: true, ..Mods::NONE };
const CTRL_SHIFT: Mods = Mods { ctrl: true, shift: true, ..Mods::NONE };

#[test]
fn modifier_gesture_fires_once_after_the_whole_chord_is_released() {
    let mut gesture = ModifierGesture::default();
    for mods in [CTRL, CTRL, CTRL_SHIFT, SHIFT, SHIFT] {
        assert_eq!(gesture.change(mods), None);
    }
    assert_eq!(gesture.change(Mods::NONE), Some(CTRL_SHIFT));
    assert_eq!(gesture.change(Mods::NONE), None);
}

#[test]
fn interrupted_modifier_gesture_cannot_fire_and_the_next_gesture_is_independent() {
    let mut gesture = ModifierGesture::default();
    gesture.change(CTRL);
    gesture.cancel();
    gesture.change(CTRL_SHIFT);
    assert_eq!(gesture.change(Mods::NONE), None);
    gesture.change(SHIFT);
    assert_eq!(gesture.change(Mods::NONE), Some(SHIFT));
}
