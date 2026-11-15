use super::*;

const OPAQUE: Candidate = Candidate { alpha: 0, transparent: false, rgb8: true, samples: 0, depth: 0, stencil: 0 };
const ALPHA: Candidate = Candidate { alpha: 8, transparent: true, ..OPAQUE };

#[test]
fn a_transparent_alpha_config_wins_over_an_opaque_one() {
    assert_eq!(pick(&[OPAQUE, ALPHA]), Some(1));
    assert_eq!(pick(&[ALPHA, OPAQUE]), Some(0));
}

#[test]
fn without_a_transparent_config_the_smallest_alpha_is_kept() {
    let alpha_only = Candidate { transparent: false, ..ALPHA };
    assert_eq!(pick(&[alpha_only, OPAQUE]), Some(1));
    assert_eq!(pick(&[alpha_only]), Some(0));
    let wide = Candidate { alpha: 2, transparent: true, ..OPAQUE };
    assert_eq!(pick(&[wide, OPAQUE]), Some(1));
}

#[test]
fn multisample_configs_are_never_picked() {
    let msaa = Candidate { samples: 4, ..ALPHA };
    assert_eq!(pick(&[msaa, OPAQUE]), Some(1));
    assert_eq!(pick(&[msaa, Candidate { samples: 4, ..OPAQUE }]), None);
    assert_eq!(pick(&[Candidate { samples: 1, ..ALPHA }]), Some(0));
}

#[test]
fn depth_and_stencil_buffers_are_avoided() {
    let deep = Candidate { depth: 24, ..ALPHA };
    let stencil = Candidate { stencil: 8, ..ALPHA };
    assert_eq!(pick(&[deep, stencil, ALPHA]), Some(2));
    assert_eq!(pick(&[Candidate { depth: 24, ..OPAQUE }, OPAQUE]), Some(1));
    assert_eq!(pick(&[deep, OPAQUE]), Some(1));
    assert_eq!(pick(&[deep]), Some(0));
}

#[test]
fn configs_without_eight_bit_color_are_skipped() {
    let narrow = Candidate { rgb8: false, ..ALPHA };
    assert_eq!(pick(&[narrow, OPAQUE]), Some(1));
    assert_eq!(pick(&[narrow]), None);
    assert_eq!(pick(&[]), None);
}
