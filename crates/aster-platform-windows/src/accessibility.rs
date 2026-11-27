#![allow(non_upper_case_globals)]

mod abi;
mod text;

use aster_core::accessibility::{Action, Node, NodeId, Role, Tree, ROOT};
use aster_gfx::render::Rect;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex, MutexGuard};
use windows::core::{implement, Error, IUnknown, Interface, Result, BOOL, HRESULT};
use windows::Win32::Foundation::{
    E_INVALIDARG, E_OUTOFMEMORY, HWND, LPARAM, LRESULT, POINT, S_OK, WPARAM,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::SAFEARRAY;
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayDestroy, SafeArrayPutElement};
use windows::Win32::System::Variant::{VARENUM, VARIANT, VT_I4, VT_UNKNOWN};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible, PostMessageW, WM_APP};

pub(super) const MESSAGE: u32 = WM_APP + 4;

pub(super) struct Accessibility {
    state: Arc<State>,
    provider: IRawElementProviderSimple,
}

impl Accessibility {
    pub(super) fn new(hwnd: HWND) -> Self {
        let state = State::new(hwnd);
        let provider = state.element(ROOT).expect("fragment root interface");
        Self { state, provider }
    }

    pub(super) fn update(&self, tree: Tree) {
        self.state.update(tree);
    }

    pub(super) fn raw(&self, hwnd: HWND, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { UiaReturnRawElementProvider(hwnd, wp, lp, &self.provider) }
    }

    pub(super) fn disconnect(&self, hwnd: HWND) {
        self.state.close();
        unsafe {
            let _ = UiaReturnRawElementProvider(
                hwnd,
                WPARAM(0),
                LPARAM(0),
                None::<&IRawElementProviderSimple>,
            );
            let _ = UiaDisconnectProvider(&self.provider);
        }
    }

    pub(super) fn drain_actions(&self) -> VecDeque<Action> {
        let mut snapshot = self.state.lock();
        std::mem::take(&mut snapshot.actions)
            .into_iter()
            .filter(|pending| pending.is_current(&snapshot.tree))
            .map(|pending| pending.action)
            .collect()
    }

    pub(super) fn notify(&self) {
        let pending = std::mem::take(&mut self.state.lock().events);
        if pending.is_empty() || !unsafe { UiaClientsAreListening().as_bool() } {
            return;
        }
        for (id, event) in pending {
            if event == UIA_AutomationFocusChangedEventId
                && self.state.lock().tree.focus != Some(id)
            {
                continue;
            }
            if event == UIA_SelectionItem_ElementSelectedEventId
                && !self
                    .state
                    .lock()
                    .tree
                    .node(id)
                    .is_some_and(|node| node.selected)
            {
                continue;
            }
            if let Ok(element) = self.state.element(id) {
                unsafe {
                    if event == UIA_StructureChangedEventId {
                        let _ = UiaRaiseStructureChangedEvent(
                            &element,
                            StructureChangeType_ChildrenInvalidated,
                            null_mut(),
                            0,
                        );
                    } else {
                        let _ = UiaRaiseAutomationEvent(&element, event);
                    }
                }
            }
        }
    }
}

struct State(Mutex<Snapshot>);

struct Snapshot {
    hwnd: Option<isize>,
    tree: Arc<Tree>,
    events: Vec<(NodeId, UIA_EVENT_ID)>,
    actions: VecDeque<PendingAction>,
}

struct PendingAction {
    action: Action,
    document: Option<Arc<Tree>>,
}

impl PendingAction {
    fn is_current(&self, tree: &Tree) -> bool {
        if let (Action::SelectText { node, .. }, Some(document)) = (self.action, &self.document) {
            return node_text(tree, node) == node_text(document, node);
        }
        true
    }
}

fn node_text(tree: &Tree, node: NodeId) -> Option<&str> {
    tree.node(node)?
        .text
        .as_ref()
        .map(|text| text.value.as_str())
}

impl State {
    fn new(hwnd: HWND) -> Arc<Self> {
        Arc::new(Self(Mutex::new(Snapshot {
            hwnd: Some(hwnd.0 as isize),
            tree: Arc::new(Tree {
                nodes: vec![Node {
                    id: ROOT,
                    parent: None,
                    role: Role::Window,
                    name: "Aster".into(),
                    bounds: Rect::default(),
                    focusable: false,
                    selected: false,
                    invokable: false,
                    text: None,
                }],
                focus: None,
            }),
            events: Vec::new(),
            actions: VecDeque::new(),
        })))
    }

    fn lock(&self) -> MutexGuard<'_, Snapshot> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn update(&self, tree: Tree) {
        let mut snapshot = self.lock();
        let Some(hwnd) = snapshot.hwnd else { return };
        let old = Arc::clone(&snapshot.tree);
        if old.as_ref() == &tree {
            return;
        }
        let events = &mut snapshot.events;
        let wake = events.is_empty();
        let mut push = |id, event| {
            if !events.contains(&(id, event)) {
                events.push((id, event));
            }
        };
        let topology = |node: &Node| (node.id, node.parent, node.role);
        if !old
            .nodes
            .iter()
            .map(topology)
            .eq(tree.nodes.iter().map(topology))
        {
            push(ROOT, UIA_StructureChangedEventId);
        }
        if old.focus != tree.focus {
            if let Some(focus) = tree.focus {
                push(focus, UIA_AutomationFocusChangedEventId);
            }
        }
        for node in &tree.nodes {
            let previous = old.node(node.id);
            if node.selected
                && is_selection_item(node.role)
                && !previous.is_some_and(|old| old.selected)
            {
                push(node.id, UIA_SelectionItem_ElementSelectedEventId);
            }
            if let (Some(text), Some(previous)) =
                (&node.text, previous.and_then(|old| old.text.as_ref()))
            {
                if text.value != previous.value {
                    push(node.id, UIA_Text_TextChangedEventId);
                }
                if (text.selection, text.caret) != (previous.selection, previous.caret) {
                    push(node.id, UIA_Text_TextSelectionChangedEventId);
                }
            }
        }
        let post = wake && !events.is_empty();
        snapshot.tree = Arc::new(tree);
        drop(snapshot);
        if post {
            let _ = post_message(hwnd);
        }
    }

    fn close(&self) {
        let mut snapshot = self.lock();
        snapshot.hwnd = None;
        snapshot.tree = Arc::new(Tree::default());
        snapshot.actions.clear();
        snapshot.events.clear();
    }

    fn snapshot(&self, id: NodeId) -> Result<(HWND, Arc<Tree>)> {
        let snapshot = self.lock();
        let hwnd = snapshot
            .hwnd
            .filter(|_| snapshot.tree.node(id).is_some())
            .ok_or_else(unavailable)?;
        Ok((HWND(hwnd as _), Arc::clone(&snapshot.tree)))
    }

    fn element(self: &Arc<Self>, id: NodeId) -> Result<IRawElementProviderSimple> {
        self.snapshot(id)?;
        let handle = ElementHandle {
            state: Arc::clone(self),
            id,
        };
        let simple: abi::Simple = if id == ROOT {
            RootElement(handle).into()
        } else {
            Element(handle).into()
        };
        simple.cast()
    }

    fn action(&self, action: Action) -> Result<()> {
        self.queue_action(PendingAction {
            action,
            document: None,
        })
    }

    fn queue_action(&self, pending: PendingAction) -> Result<()> {
        let mut snapshot = self.lock();
        let hwnd = snapshot.hwnd.ok_or_else(unavailable)?;
        let id = match pending.action {
            Action::Focus(id) | Action::Invoke(id) | Action::SelectItem(id) => id,
            Action::SelectText { node, .. } => node,
        };
        let node = snapshot.tree.node(id).ok_or_else(unavailable)?;
        let supported = match pending.action {
            Action::Focus(_) => node.focusable || id == ROOT,
            Action::Invoke(_) => node.invokable,
            Action::SelectItem(_) => is_selection_item(node.role),
            Action::SelectText { .. } => node.role == Role::Terminal && node.text.is_some(),
        };
        if !supported {
            return Err(invalid_operation());
        }
        if !pending.is_current(&snapshot.tree) {
            return Err(unavailable());
        }
        post_message(hwnd)?;
        snapshot.actions.push_back(pending);
        Ok(())
    }
}

fn post_message(hwnd: isize) -> Result<()> {
    unsafe { PostMessageW(Some(HWND(hwnd as _)), MESSAGE, WPARAM(0), LPARAM(0)) }
}

fn unavailable() -> Error {
    HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32).into()
}
fn invalid_operation() -> Error {
    HRESULT(UIA_E_INVALIDOPERATION as i32).into()
}
fn absent() -> Error {
    Error::from_hresult(S_OK)
}

fn origin(hwnd: HWND) -> Result<POINT> {
    let mut point = POINT::default();
    unsafe { ClientToScreen(hwnd, &mut point) }.ok()?;
    Ok(point)
}

fn screen_rect(bounds: Rect, origin: POINT) -> UiaRect {
    UiaRect {
        left: f64::from(bounds.x) + f64::from(origin.x),
        top: f64::from(bounds.y) + f64::from(origin.y),
        width: f64::from(bounds.w),
        height: f64::from(bounds.h),
    }
}

fn safe_array<T>(
    kind: VARENUM,
    values: &[T],
    as_element: impl Fn(&T) -> *const c_void,
) -> Result<*mut SAFEARRAY> {
    let count = u32::try_from(values.len()).map_err(|_| Error::from_hresult(E_OUTOFMEMORY))?;
    let array = unsafe { SafeArrayCreateVector(kind, 0, count) };
    if array.is_null() {
        return Err(E_OUTOFMEMORY.into());
    }
    for (index, value) in values.iter().enumerate() {
        if let Err(error) =
            unsafe { SafeArrayPutElement(array, &(index as i32), as_element(value)) }
        {
            unsafe {
                let _ = SafeArrayDestroy(array);
            }
            return Err(error);
        }
    }
    Ok(array)
}

fn unknown_array<T: Interface>(values: &[T]) -> Result<*mut SAFEARRAY> {
    safe_array(VT_UNKNOWN, values, |value| value.as_raw())
}

fn control_type(role: Role) -> UIA_CONTROLTYPE_ID {
    match role {
        Role::Window => UIA_WindowControlTypeId,
        Role::TabList => UIA_TabControlTypeId,
        Role::Tab => UIA_TabItemControlTypeId,
        Role::Terminal => UIA_DocumentControlTypeId,
        Role::Dialog => UIA_PaneControlTypeId,
        Role::Edit => UIA_EditControlTypeId,
        Role::List => UIA_ListControlTypeId,
        Role::ListItem => UIA_ListItemControlTypeId,
    }
}

fn is_selection_item(role: Role) -> bool {
    matches!(role, Role::Tab | Role::ListItem)
}

fn is_selection_container(role: Role) -> bool {
    matches!(role, Role::TabList | Role::List)
}

#[derive(Clone)]
struct ElementHandle {
    state: Arc<State>,
    id: NodeId,
}

impl ElementHandle {
    fn snapshot(&self) -> Result<(HWND, Arc<Tree>)> {
        self.state.snapshot(self.id)
    }

    fn element(&self) -> Result<IRawElementProviderSimple> {
        self.state.element(self.id)
    }

    fn property(&self, property: UIA_PROPERTY_ID) -> Result<VARIANT> {
        let (hwnd, tree) = self.snapshot()?;
        let node = tree.node(self.id).ok_or_else(unavailable)?;
        Ok(match property {
            UIA_NamePropertyId => node.name.as_str().into(),
            UIA_ControlTypePropertyId => control_type(node.role).0.into(),
            UIA_AutomationIdPropertyId => format!("aster-{}", self.id).as_str().into(),
            UIA_IsControlElementPropertyId
            | UIA_IsContentElementPropertyId
            | UIA_IsEnabledPropertyId => true.into(),
            UIA_IsKeyboardFocusablePropertyId => node.focusable.into(),
            UIA_HasKeyboardFocusPropertyId => (tree.focus == Some(self.id)).into(),
            UIA_IsOffscreenPropertyId => unsafe {
                (!IsWindowVisible(hwnd).as_bool()
                    || IsIconic(hwnd).as_bool()
                    || node.bounds.w <= 0.0
                    || node.bounds.h <= 0.0)
                    .into()
            },
            UIA_IsDialogPropertyId => (node.role == Role::Dialog).into(),
            UIA_IsInvokePatternAvailablePropertyId => node.invokable.into(),
            UIA_IsSelectionPatternAvailablePropertyId => is_selection_container(node.role).into(),
            UIA_IsSelectionItemPatternAvailablePropertyId => is_selection_item(node.role).into(),
            UIA_SelectionItemIsSelectedPropertyId => node.selected.into(),
            UIA_IsTextPatternAvailablePropertyId | UIA_IsTextPattern2AvailablePropertyId => {
                node.text.is_some().into()
            }
            _ => VARIANT::default(),
        })
    }

    fn pattern(&self, pattern: UIA_PATTERN_ID) -> Result<IUnknown> {
        let (_, tree) = self.snapshot()?;
        let node = tree.node(self.id).ok_or_else(unavailable)?;
        match pattern {
            UIA_InvokePatternId if node.invokable => {
                Ok(IInvokeProvider::from(Invoke(self.clone())).into())
            }
            UIA_SelectionPatternId if is_selection_container(node.role) => {
                Ok(ISelectionProvider::from(Selection(self.clone())).into())
            }
            UIA_SelectionItemPatternId if is_selection_item(node.role) => {
                Ok(ISelectionItemProvider::from(SelectionItem(self.clone())).into())
            }
            UIA_TextPatternId | UIA_TextPattern2Id if node.text.is_some() => {
                Ok(ITextProvider2::from(text::Provider(self.clone())).into())
            }
            _ => Err(absent()),
        }
    }

    fn navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        let (_, tree) = self.snapshot()?;
        let node = tree.node(self.id).ok_or_else(unavailable)?;
        let target = match direction {
            NavigateDirection_Parent => node.parent,
            NavigateDirection_FirstChild => tree
                .nodes
                .iter()
                .find(|n| n.parent == Some(self.id))
                .map(|n| n.id),
            NavigateDirection_LastChild => tree
                .nodes
                .iter()
                .rev()
                .find(|n| n.parent == Some(self.id))
                .map(|n| n.id),
            NavigateDirection_NextSibling => {
                if node.parent.is_none() {
                    return Err(absent());
                }
                tree.nodes
                    .iter()
                    .skip_while(|n| n.id != self.id)
                    .skip(1)
                    .find(|n| n.parent == node.parent)
                    .map(|n| n.id)
            }
            NavigateDirection_PreviousSibling => {
                if node.parent.is_none() {
                    return Err(absent());
                }
                tree.nodes
                    .iter()
                    .rev()
                    .skip_while(|n| n.id != self.id)
                    .skip(1)
                    .find(|n| n.parent == node.parent)
                    .map(|n| n.id)
            }
            _ => return Err(E_INVALIDARG.into()),
        };
        self.state.element(target.ok_or_else(absent)?)?.cast()
    }

    fn runtime_id(&self) -> Result<*mut SAFEARRAY> {
        self.snapshot()?;
        if self.id == ROOT {
            return Ok(null_mut());
        }
        safe_array(
            VT_I4,
            &[
                UiaAppendRuntimeId as i32,
                self.id as i32,
                (self.id >> 32) as i32,
            ],
            |value| (value as *const i32).cast(),
        )
    }

    fn bounds(&self) -> Result<UiaRect> {
        let (hwnd, tree) = self.snapshot()?;
        let bounds = tree.node(self.id).ok_or_else(unavailable)?.bounds;
        if !unsafe { IsWindowVisible(hwnd).as_bool() } || unsafe { IsIconic(hwnd).as_bool() } {
            return Ok(UiaRect::default());
        }
        Ok(screen_rect(bounds, origin(hwnd)?))
    }
}

#[implement(abi::Simple, abi::Fragment)]
struct Element(ElementHandle);

#[implement(abi::Simple, abi::Fragment, abi::FragmentRoot)]
struct RootElement(ElementHandle);

macro_rules! implement_element {
    ($($element:ty),+ $(,)?) => {$(
        impl IRawElementProviderSimple_Impl for $element {
            fn ProviderOptions(&self) -> Result<ProviderOptions> {
                Ok(ProviderOptions_ServerSideProvider)
            }
            fn GetPatternProvider(&self, pattern: UIA_PATTERN_ID) -> Result<IUnknown> {
                self.0.pattern(pattern)
            }
            fn GetPropertyValue(&self, property: UIA_PROPERTY_ID) -> Result<VARIANT> {
                self.0.property(property)
            }
            fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
                let (hwnd, _) = self.0.snapshot()?;
                if self.0.id == ROOT { unsafe { UiaHostProviderFromHwnd(hwnd) } } else { Err(absent()) }
            }
        }
        impl IRawElementProviderFragment_Impl for $element {
            fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
                self.0.navigate(direction)
            }
            fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
                self.0.runtime_id()
            }
            fn BoundingRectangle(&self) -> Result<UiaRect> {
                self.0.bounds()
            }
            fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
                self.0.snapshot()?;
                Ok(null_mut())
            }
            fn SetFocus(&self) -> Result<()> {
                self.0.state.action(Action::Focus(self.0.id))
            }
            fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
                self.0.state.element(ROOT)?.cast()
            }
        }
    )+};
}

implement_element!(Element_Impl, RootElement_Impl);

impl IRawElementProviderFragmentRoot_Impl for RootElement_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        let (hwnd, tree) = self.0.snapshot()?;
        let point = origin(hwnd)?;
        let x = (x - f64::from(point.x)) as f32;
        let y = (y - f64::from(point.y)) as f32;
        let node = tree
            .nodes
            .iter()
            .rev()
            .find(|node| node.bounds.contains(x, y))
            .ok_or_else(absent)?;
        self.0.state.element(node.id)?.cast()
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        let (_, tree) = self.0.snapshot()?;
        self.0.state.element(tree.focus.ok_or_else(absent)?)?.cast()
    }
}

#[implement(IInvokeProvider)]
struct Invoke(ElementHandle);

impl IInvokeProvider_Impl for Invoke_Impl {
    fn Invoke(&self) -> Result<()> {
        self.0.state.action(Action::Invoke(self.0.id))
    }
}

#[implement(ISelectionProvider)]
struct Selection(ElementHandle);

impl ISelectionProvider_Impl for Selection_Impl {
    fn GetSelection(&self) -> Result<*mut SAFEARRAY> {
        let (_, tree) = self.0.snapshot()?;
        let selected = tree
            .nodes
            .iter()
            .filter(|node| node.parent == Some(self.0.id) && node.selected)
            .map(|node| self.0.state.element(node.id))
            .collect::<Result<Vec<_>>>()?;
        unknown_array(&selected)
    }

    fn CanSelectMultiple(&self) -> Result<BOOL> {
        Ok(false.into())
    }

    fn IsSelectionRequired(&self) -> Result<BOOL> {
        let (_, tree) = self.0.snapshot()?;
        Ok((tree
            .node(self.0.id)
            .is_some_and(|node| node.role == Role::TabList)
            && tree.nodes.iter().any(|node| node.parent == Some(self.0.id)))
        .into())
    }
}

#[implement(ISelectionItemProvider)]
struct SelectionItem(ElementHandle);

impl ISelectionItemProvider_Impl for SelectionItem_Impl {
    fn Select(&self) -> Result<()> {
        self.0.state.action(Action::SelectItem(self.0.id))
    }

    fn AddToSelection(&self) -> Result<()> {
        let (_, tree) = self.0.snapshot()?;
        let node = tree.node(self.0.id).ok_or_else(unavailable)?;
        if node.selected {
            Ok(())
        } else if tree
            .nodes
            .iter()
            .any(|item| item.parent == node.parent && item.selected)
        {
            Err(invalid_operation())
        } else {
            self.Select()
        }
    }

    fn RemoveFromSelection(&self) -> Result<()> {
        if self.IsSelected()?.as_bool() {
            Err(invalid_operation())
        } else {
            Ok(())
        }
    }

    fn IsSelected(&self) -> Result<BOOL> {
        let (_, tree) = self.0.snapshot()?;
        Ok(tree
            .node(self.0.id)
            .ok_or_else(unavailable)?
            .selected
            .into())
    }

    fn SelectionContainer(&self) -> Result<IRawElementProviderSimple> {
        let (_, tree) = self.0.snapshot()?;
        let parent = tree
            .node(self.0.id)
            .and_then(|node| node.parent)
            .ok_or_else(unavailable)?;
        self.0.state.element(parent)
    }
}

#[cfg(test)]
#[path = "../test/accessibility.rs"]
mod tests;
