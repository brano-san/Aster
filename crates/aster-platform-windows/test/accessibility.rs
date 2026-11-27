use super::*;
use aster_core::accessibility::{Node, Text, TextLine};
use windows::core::BSTR;
use windows::Win32::System::Ole::{SafeArrayDestroy, SafeArrayGetElement};

#[test]
fn native_fragment_exposes_focused_terminal_with_unicode_selection() {
    let state = State::new(HWND(1 as _));
    state.update(Tree {
        nodes: vec![
            Node {
                id: ROOT,
                parent: None,
                role: Role::Window,
                name: "Aster".into(),
                bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
                focusable: false,
                selected: false,
                invokable: false,
                text: None,
            },
            Node {
                id: 2,
                parent: Some(ROOT),
                role: Role::Terminal,
                name: "Terminal".into(),
                bounds: Rect::new(0.0, 10.0, 100.0, 90.0),
                focusable: true,
                selected: false,
                invokable: false,
                text: Some(Text {
                    value: "a🙂界\nnext".into(),
                    selection: Some((1, 3)),
                    caret: Some(3),
                    lines: vec![TextLine {
                        start: 0,
                        end: 4,
                        bounds: Rect::new(0.0, 10.0, 30.0, 10.0),
                        offsets: vec![0.0, 5.0, 15.0, 25.0, 25.0],
                    }],
                }),
            },
        ],
        focus: Some(2),
    });
    let simple = state.element(ROOT).unwrap();
    let root: IRawElementProviderFragmentRoot = simple.cast().unwrap();
    let child: IRawElementProviderSimple = unsafe { root.GetFocus() }.unwrap().cast().unwrap();
    let pattern: ITextProvider = unsafe { child.GetPatternProvider(UIA_TextPatternId) }
        .unwrap()
        .cast()
        .unwrap();
    unsafe {
        let ranges = pattern.GetSelection().unwrap();
        let mut raw = std::ptr::null_mut();
        SafeArrayGetElement(ranges, &0, (&mut raw as *mut *mut std::ffi::c_void).cast()).unwrap();
        let range = ITextRangeProvider::from_raw(raw);
        SafeArrayDestroy(ranges).unwrap();
        assert_eq!(range.GetText(-1).unwrap(), BSTR::from("🙂界"));
        range
            .MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Character, -1)
            .unwrap();
        assert_eq!(range.GetText(-1).unwrap(), BSTR::from("🙂"));
        assert_eq!(
            pattern.DocumentRange().unwrap().GetText(-1).unwrap(),
            BSTR::from("a🙂界\nnext")
        );
    }
    state.close();
    assert!(unsafe { root.GetFocus() }.is_err());
}

#[test]
fn native_text_range_reports_search_input_as_editable() {
    let state = State::new(HWND(1 as _));
    let mut tree = (*state.snapshot(ROOT).unwrap().1).clone();
    tree.nodes.push(Node {
        id: 2,
        parent: Some(ROOT),
        role: Role::Edit,
        name: "Search".into(),
        bounds: Rect::new(0.0, 0.0, 100.0, 10.0),
        focusable: true,
        selected: false,
        invokable: false,
        text: Some(Text {
            value: "query".into(),
            ..Text::default()
        }),
    });
    state.update(tree);
    let edit = state.element(2).unwrap();
    let pattern: ITextProvider = unsafe { edit.GetPatternProvider(UIA_TextPatternId) }
        .unwrap()
        .cast()
        .unwrap();
    unsafe {
        let range = pattern.DocumentRange().unwrap();
        let read_only = range.GetAttributeValue(UIA_IsReadOnlyAttributeId).unwrap();
        assert!(!bool::try_from(&read_only).unwrap());
        let editable = range
            .FindAttribute(UIA_IsReadOnlyAttributeId, &VARIANT::from(false), false)
            .unwrap();
        assert_eq!(editable.GetText(-1).unwrap(), BSTR::from("query"));
    }
}
