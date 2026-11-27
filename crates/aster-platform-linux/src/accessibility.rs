mod text;

use aster_core::accessibility::{Action, Node, NodeId, Role, Text, Tree, ROOT};
use aster_gfx::render::Rect;
use aster_platform::WindowId;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use zbus::zvariant::{OwnedObjectPath, Value};
use zbus::{connection::Builder, interface, Connection};

const APP_PATH: &str = "/org/a11y/atspi/accessible/root";
const CACHE_PATH: &str = "/org/a11y/atspi/cache";
const CACHE_INTERFACE: &str = "org.a11y.atspi.Cache";
const NULL_PATH: &str = "/org/a11y/atspi/null";
const OBJECT_EVENT: &str = "org.a11y.atspi.Event.Object";
const WINDOW_EVENT: &str = "org.a11y.atspi.Event.Window";
const TIMEOUT: Duration = Duration::from_secs(3);

type ObjectRef = (String, OwnedObjectPath);
type Pending = Arc<Mutex<BTreeMap<WindowId, Option<Snapshot>>>>;
type ActionHandler = dyn Fn(WindowId, Action) -> bool + Send + Sync;

pub(crate) struct Bridge {
    active: Arc<AtomicBool>,
    pending: Pending,
    changed: mpsc::Sender<()>,
}

impl Bridge {
    pub(crate) fn start(
        action: impl Fn(WindowId, Action) -> bool + Send + Sync + 'static,
        ready: impl FnOnce() + Send + 'static,
    ) -> Self {
        let active = Arc::new(AtomicBool::new(false));
        let pending = Pending::default();
        let (changed, receiver) = mpsc::channel(1);
        if std::env::var_os("NO_AT_BRIDGE").is_none_or(|value| value != "1") {
            let (active, pending) = (active.clone(), pending.clone());
            let _ = std::thread::Builder::new()
                .name("aster-atspi".into())
                .spawn(move || {
                    let result = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(zbus::Error::from)
                        .and_then(|runtime| {
                            runtime.block_on(serve(
                                active.clone(),
                                pending,
                                receiver,
                                Arc::new(action),
                                ready,
                            ))
                        });
                    active.store(false, Ordering::Release);
                    if let Err(error) = result {
                        tracing::debug!("AT-SPI unavailable: {error}");
                    }
                });
        }
        Self {
            active,
            pending,
            changed,
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub(crate) fn update(&self, window: WindowId, tree: Tree, origin: Option<(i32, i32)>) {
        if self.active() {
            self.send(window, Some(Snapshot { tree, origin }));
        }
    }

    pub(crate) fn remove(&self, window: WindowId) {
        if self.active() {
            self.send(window, None);
        }
    }

    fn send(&self, window: WindowId, value: Option<Snapshot>) {
        self.pending.lock().unwrap().insert(window, value);
        let _ = self.changed.try_send(());
    }
}

#[derive(PartialEq)]
struct Snapshot {
    tree: Tree,
    origin: Option<(i32, i32)>,
}

#[derive(Default)]
struct State {
    windows: BTreeMap<WindowId, Snapshot>,
    parent: Option<ObjectRef>,
}

struct Service {
    bus: String,
    state: RwLock<State>,
    action: Arc<ActionHandler>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Object {
    Application,
    Widget(WindowId, NodeId),
}

impl Object {
    fn path(self) -> OwnedObjectPath {
        match self {
            Self::Application => OwnedObjectPath::try_from(APP_PATH).unwrap(),
            Self::Widget(window, node) => {
                OwnedObjectPath::try_from(format!("/org/a11y/atspi/accessible/w{window}_n{node}"))
                    .unwrap()
            }
        }
    }

    fn parent(self, state: &State) -> Option<Self> {
        let Self::Widget(window, id) = self else {
            return None;
        };
        let node = state.windows.get(&window)?.tree.node(id)?;
        Some(
            node.parent
                .map_or(Self::Application, |parent| Self::Widget(window, parent)),
        )
    }

    fn children(self, state: &State) -> Vec<Self> {
        match self {
            Self::Application => state
                .windows
                .keys()
                .map(|&window| Self::Widget(window, ROOT))
                .collect(),
            Self::Widget(window, id) => children(state.windows.get(&window), Some(id))
                .map(|child| Self::Widget(window, child))
                .collect(),
        }
    }
}

fn children(
    snapshot: Option<&Snapshot>,
    parent: Option<NodeId>,
) -> impl Iterator<Item = NodeId> + '_ {
    snapshot
        .into_iter()
        .flat_map(|snapshot| &snapshot.tree.nodes)
        .filter(move |node| node.parent == parent)
        .map(|node| node.id)
}

fn null_ref() -> ObjectRef {
    (String::new(), OwnedObjectPath::try_from(NULL_PATH).unwrap())
}

#[derive(Clone)]
struct Widget {
    object: Object,
    service: Arc<Service>,
}

impl Widget {
    fn reference(&self, object: Object) -> ObjectRef {
        (self.service.bus.clone(), object.path())
    }

    fn read<T: Default>(&self, read: impl FnOnce(&Snapshot, &Node) -> T) -> T {
        let Object::Widget(window, id) = self.object else {
            return T::default();
        };
        let state = self.service.state.read().unwrap();
        state
            .windows
            .get(&window)
            .and_then(|snapshot| snapshot.tree.node(id).map(|node| read(snapshot, node)))
            .unwrap_or_default()
    }

    fn apply(&self, make: impl FnOnce(&Node) -> Option<Action>) -> bool {
        let Object::Widget(window, _) = self.object else {
            return false;
        };
        self.read(|_, node| make(node))
            .is_some_and(|action| (self.service.action)(window, action))
    }

    fn origin(&self, coords: u32) -> Option<(f32, f32)> {
        self.read(|snapshot, node| match coords {
            0 => snapshot.origin.map(|(x, y)| (x as f32, y as f32)),
            1 => Some((0., 0.)),
            2 => {
                let parent = node
                    .parent
                    .and_then(|id| snapshot.tree.node(id))
                    .map(|node| node.bounds)
                    .unwrap_or_default();
                Some((-parent.x, -parent.y))
            }
            _ => None,
        })
    }

    fn extents(&self, bounds: Rect, coords: u32) -> (i32, i32, i32, i32) {
        self.origin(coords).map_or((-1, -1, 0, 0), |(x, y)| {
            let rect = Rect::new(bounds.x + x, bounds.y + y, bounds.w, bounds.h).snap();
            (rect.x as i32, rect.y as i32, rect.w as i32, rect.h as i32)
        })
    }
}

async fn bus_connection() -> zbus::Result<Connection> {
    let address = match std::env::var("AT_SPI_BUS_ADDRESS")
        .ok()
        .filter(|value| !value.is_empty())
    {
        Some(address) => address,
        None => {
            let session = Builder::session()?.method_timeout(TIMEOUT).build().await?;
            let proxy =
                zbus::Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus").await?;
            proxy.call("GetAddress", &()).await?
        }
    };
    Builder::address(address.as_str())?
        .method_timeout(TIMEOUT)
        .build()
        .await
}

async fn serve(
    active: Arc<AtomicBool>,
    pending: Pending,
    mut changed: mpsc::Receiver<()>,
    action: Arc<ActionHandler>,
    ready: impl FnOnce(),
) -> zbus::Result<()> {
    let (connection, service) = tokio::time::timeout(TIMEOUT, connect(action))
        .await
        .map_err(|_| zbus::Error::Failure("AT-SPI connection timed out".into()))??;
    active.store(true, Ordering::Release);
    ready();
    while changed.recv().await.is_some() {
        let updates = std::mem::take(&mut *pending.lock().unwrap());
        for (window, snapshot) in updates {
            publish(&connection, &service, window, snapshot).await?;
        }
    }
    Ok(())
}

async fn connect(action: Arc<ActionHandler>) -> zbus::Result<(Connection, Arc<Service>)> {
    let connection = bus_connection().await?;
    let bus = connection
        .unique_name()
        .ok_or_else(|| zbus::Error::Failure("AT-SPI requires a message bus".into()))?
        .to_string();
    let service = Arc::new(Service {
        bus,
        state: RwLock::default(),
        action,
    });
    let root = Widget {
        object: Object::Application,
        service: service.clone(),
    };
    connection
        .object_server()
        .at(APP_PATH, Accessible(root.clone()))
        .await?;
    connection
        .object_server()
        .at(APP_PATH, Application::default())
        .await?;
    connection
        .object_server()
        .at(CACHE_PATH, Cache(service.clone()))
        .await?;
    let registry = zbus::Proxy::new(
        &connection,
        "org.a11y.atspi.Registry",
        APP_PATH,
        "org.a11y.atspi.Socket",
    )
    .await?;
    let parent = registry
        .call("Embed", &(root.reference(Object::Application),))
        .await?;
    service.state.write().unwrap().parent = Some(parent);
    drop(registry);
    Ok((connection, service))
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Interfaces {
    component: bool,
    action: bool,
    text: bool,
    selection: bool,
}

impl Interfaces {
    fn for_node(node: &Node) -> Self {
        Self {
            component: true,
            action: node.invokable,
            text: node.text.is_some(),
            selection: matches!(node.role, Role::TabList | Role::List),
        }
    }
}

async fn update_interfaces(
    connection: &Connection,
    widget: Widget,
    before: Interfaces,
    after: Interfaces,
) -> zbus::Result<()> {
    let server = connection.object_server();
    let path = widget.object.path();
    if !before.component && after.component {
        server.at(&path, Accessible(widget.clone())).await?;
    }
    macro_rules! update {
        ($field:ident, $interface:ident) => {
            if before.$field != after.$field {
                if after.$field {
                    server.at(&path, $interface(widget.clone())).await?;
                } else {
                    server.remove::<$interface, _>(&path).await?;
                }
            }
        };
    }
    update!(action, Actions);
    update!(text, AccessibleText);
    update!(selection, Selection);
    update!(component, Component);
    if before.component && !after.component {
        server.remove::<Accessible, _>(&path).await?;
    }
    Ok(())
}

async fn publish(
    connection: &Connection,
    service: &Arc<Service>,
    window: WindowId,
    snapshot: Option<Snapshot>,
) -> zbus::Result<()> {
    let old_interfaces = {
        let state = service.state.read().unwrap();
        let old = state.windows.get(&window);
        if old == snapshot.as_ref() {
            return Ok(());
        }
        node_interfaces(old)
    };
    let new_interfaces = node_interfaces(snapshot.as_ref());
    for (&id, &after) in &new_interfaces {
        let before = old_interfaces.get(&id).copied().unwrap_or_default();
        if before != after {
            update_interfaces(
                connection,
                Widget {
                    object: Object::Widget(window, id),
                    service: service.clone(),
                },
                before,
                after,
            )
            .await?;
        }
    }
    let (old, exists, root_index) = {
        let mut state = service.state.write().unwrap();
        let root_index = state.windows.range(..window).count();
        let old = match snapshot {
            Some(snapshot) => state.windows.insert(window, snapshot),
            None => state.windows.remove(&window),
        };
        (old, state.windows.contains_key(&window), root_index)
    };
    let root = Object::Widget(window, ROOT);
    let added = {
        let state = service.state.read().unwrap();
        state
            .windows
            .get(&window)
            .into_iter()
            .flat_map(|snapshot| &snapshot.tree.nodes)
            .filter(|node| !old_interfaces.contains_key(&node.id))
            .map(|node| node.id)
            .collect::<Vec<_>>()
    };
    let cache = Cache(service.clone());
    for id in added {
        let item = cache.item(Object::Widget(window, id));
        connection
            .emit_signal(
                None::<&str>,
                CACHE_PATH,
                CACHE_INTERFACE,
                "AddAccessible",
                &(item,),
            )
            .await?;
    }
    match (old.is_some(), exists) {
        (false, true) => {
            emit(
                connection,
                Object::Application,
                OBJECT_EVENT,
                "ChildrenChanged",
                "add",
                index(root_index),
                Value::from((service.bus.as_str(), root.path())),
            )
            .await?
        }
        (true, false) => {
            emit(
                connection,
                Object::Application,
                OBJECT_EVENT,
                "ChildrenChanged",
                "remove",
                index(root_index),
                Value::from((service.bus.as_str(), root.path())),
            )
            .await?
        }
        _ => {}
    }
    let events = {
        let state = service.state.read().unwrap();
        change_events(window, old.as_ref(), state.windows.get(&window))
    };
    for event in events {
        event.send(connection, service).await?;
    }
    for (id, before) in old_interfaces {
        if !new_interfaces.contains_key(&id) {
            let widget = Widget {
                object: Object::Widget(window, id),
                service: service.clone(),
            };
            connection
                .emit_signal(
                    None::<&str>,
                    CACHE_PATH,
                    CACHE_INTERFACE,
                    "RemoveAccessible",
                    &(widget.reference(widget.object),),
                )
                .await?;
            update_interfaces(connection, widget, before, Interfaces::default()).await?;
        }
    }
    Ok(())
}

fn node_interfaces(snapshot: Option<&Snapshot>) -> HashMap<NodeId, Interfaces> {
    snapshot
        .into_iter()
        .flat_map(|snapshot| &snapshot.tree.nodes)
        .map(|node| (node.id, Interfaces::for_node(node)))
        .collect()
}

fn index(index: usize) -> i32 {
    index.min(i32::MAX as usize) as i32
}

enum Payload {
    Empty,
    Child(Object),
    Text(String),
}

struct Change {
    object: Object,
    member: &'static str,
    detail: &'static str,
    first: i32,
    data: Payload,
}

impl Change {
    fn state(object: Object, detail: &'static str, active: bool) -> Self {
        Self {
            object,
            member: "StateChanged",
            detail,
            first: i32::from(active),
            data: Payload::Empty,
        }
    }

    async fn send(self, connection: &Connection, service: &Service) -> zbus::Result<()> {
        let interface = match self.member {
            "Activate" | "Deactivate" => WINDOW_EVENT,
            _ => OBJECT_EVENT,
        };
        let payload = match &self.data {
            Payload::Empty => Value::from(0i32),
            Payload::Child(child) => Value::from((service.bus.as_str(), child.path())),
            Payload::Text(text) => Value::from(text.as_str()),
        };
        emit(
            connection,
            self.object,
            interface,
            self.member,
            self.detail,
            self.first,
            payload,
        )
        .await
    }
}

async fn emit(
    connection: &Connection,
    object: Object,
    interface: &str,
    member: &str,
    detail: &str,
    first: i32,
    data: Value<'_>,
) -> zbus::Result<()> {
    let second = if member == "TextChanged" {
        data.downcast_ref::<&str>()
            .map_or(0, |text| index(text.chars().count()))
    } else {
        0
    };
    connection
        .emit_signal(
            None::<&str>,
            object.path(),
            interface,
            member,
            &(
                detail,
                first,
                second,
                data,
                HashMap::<&str, Value<'_>>::new(),
            ),
        )
        .await
}

fn change_events(window: WindowId, old: Option<&Snapshot>, new: Option<&Snapshot>) -> Vec<Change> {
    let mut events = Vec::new();
    let object = |id| Object::Widget(window, id);
    let before_focus = old.and_then(|snapshot| snapshot.tree.focus);
    let after_focus = new.and_then(|snapshot| snapshot.tree.focus);
    if before_focus != after_focus {
        if let Some(id) = before_focus {
            events.push(Change::state(object(id), "focused", false));
        }
        if before_focus.is_some() != after_focus.is_some() {
            events.push(Change::state(object(ROOT), "active", after_focus.is_some()));
            let member = if after_focus.is_some() {
                "Activate"
            } else {
                "Deactivate"
            };
            events.push(Change {
                object: object(ROOT),
                member,
                detail: "",
                first: 0,
                data: Payload::Empty,
            });
        }
    }
    if let (Some(old), Some(new)) = (old, new) {
        for node in &new.tree.nodes {
            let Some(before) = old.tree.node(node.id) else {
                continue;
            };
            let source = object(node.id);
            let before_children: Vec<_> = children(Some(old), Some(node.id)).collect();
            let after_children: Vec<_> = children(Some(new), Some(node.id)).collect();
            for (detail, from, to) in [
                ("remove", &before_children, &after_children),
                ("add", &after_children, &before_children),
            ] {
                let mut changed: Vec<_> = from
                    .iter()
                    .enumerate()
                    .filter(|(i, id)| to.get(*i) != Some(id))
                    .collect();
                if detail == "remove" {
                    changed.reverse();
                }
                for (i, &id) in changed {
                    events.push(Change {
                        object: source,
                        member: "ChildrenChanged",
                        detail,
                        first: index(i),
                        data: Payload::Child(object(id)),
                    });
                }
            }
            if node.name != before.name {
                events.push(Change {
                    object: source,
                    member: "PropertyChange",
                    detail: "accessible-name",
                    first: 0,
                    data: Payload::Text(node.name.clone()),
                });
            }
            if node.selected != before.selected {
                events.push(Change::state(source, "selected", node.selected));
                if let Some(parent) = node.parent {
                    events.push(Change {
                        object: object(parent),
                        member: "SelectionChanged",
                        detail: "",
                        first: 0,
                        data: Payload::Empty,
                    });
                }
            }
            text_events(
                source,
                before.text.as_ref(),
                node.text.as_ref(),
                &mut events,
            );
        }
    }
    if before_focus != after_focus {
        if let Some(id) = after_focus {
            events.push(Change::state(object(id), "focused", true));
        }
    }
    events
}

fn text_events(object: Object, old: Option<&Text>, new: Option<&Text>, events: &mut Vec<Change>) {
    let (Some(old), Some(new)) = (old, new) else {
        return;
    };
    if old.value != new.value {
        let (offset, deleted, inserted) = text_change(&old.value, &new.value);
        for (detail, text) in [("delete", deleted), ("insert", inserted)] {
            if !text.is_empty() {
                events.push(Change {
                    object,
                    member: "TextChanged",
                    detail,
                    first: index(offset),
                    data: Payload::Text(text.to_owned()),
                });
            }
        }
    }
    if old.caret != new.caret {
        events.push(Change {
            object,
            member: "TextCaretMoved",
            detail: "",
            first: new.caret.map_or(-1, index),
            data: Payload::Empty,
        });
    }
    if old.selection != new.selection {
        events.push(Change {
            object,
            member: "TextSelectionChanged",
            detail: "",
            first: 0,
            data: Payload::Empty,
        });
    }
}

#[derive(Default)]
struct Application {
    id: i32,
}

#[interface(name = "org.a11y.atspi.Application")]
impl Application {
    #[zbus(property)]
    fn toolkit_name(&self) -> &str {
        "Aster"
    }

    #[zbus(property)]
    fn version(&self) -> &str {
        self.toolkit_version()
    }

    #[zbus(property)]
    fn toolkit_version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    #[zbus(property)]
    fn atspi_version(&self) -> &str {
        "2.1"
    }

    #[zbus(property)]
    fn interface_version(&self) -> u32 {
        1
    }

    #[zbus(property)]
    fn id(&self) -> i32 {
        self.id
    }

    #[zbus(property)]
    fn set_id(&mut self, id: i32) {
        self.id = id;
    }

    fn get_locale(&self, _category: u32) -> String {
        locale()
    }

    fn get_application_bus_address(&self) -> &str {
        ""
    }
}

fn locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .unwrap_or_else(|| "C".into())
}

struct Accessible(Widget);

type CacheItem = (
    ObjectRef,
    ObjectRef,
    ObjectRef,
    i32,
    i32,
    Vec<&'static str>,
    String,
    u32,
    &'static str,
    Vec<u32>,
);

struct Cache(Arc<Service>);

#[interface(name = "org.a11y.atspi.Cache")]
impl Cache {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }

    fn get_items(&self) -> Vec<CacheItem> {
        let objects: Vec<_> = self
            .0
            .state
            .read()
            .unwrap()
            .windows
            .iter()
            .flat_map(|(&window, snapshot)| {
                snapshot
                    .tree
                    .nodes
                    .iter()
                    .map(move |node| Object::Widget(window, node.id))
            })
            .collect();
        std::iter::once(Object::Application)
            .chain(objects)
            .map(|object| self.item(object))
            .collect()
    }
}

impl Cache {
    fn item(&self, object: Object) -> CacheItem {
        let accessible = Accessible(Widget {
            object,
            service: self.0.clone(),
        });
        (
            accessible.0.reference(object),
            accessible.get_application().0,
            accessible.parent(),
            accessible.get_index_in_parent(),
            accessible.child_count(),
            accessible.get_interfaces(),
            accessible.name(),
            accessible.get_role(),
            "",
            accessible.get_state(),
        )
    }
}

#[interface(name = "org.a11y.atspi.Accessible")]
impl Accessible {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }

    #[zbus(property)]
    fn name(&self) -> String {
        if self.0.object == Object::Application {
            "Aster".into()
        } else {
            self.0.read(|_, node| node.name.clone())
        }
    }

    #[zbus(property)]
    fn description(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn parent(&self) -> ObjectRef {
        let state = self.0.service.state.read().unwrap();
        if self.0.object == Object::Application {
            return state.parent.clone().unwrap_or_else(null_ref);
        }
        self.0
            .object
            .parent(&state)
            .map_or_else(null_ref, |object| self.0.reference(object))
    }

    #[zbus(property)]
    fn child_count(&self) -> i32 {
        index(self.get_children().len())
    }

    #[zbus(property)]
    fn locale(&self) -> String {
        locale()
    }

    #[zbus(property)]
    fn accessible_id(&self) -> String {
        match self.0.object {
            Object::Application => "Aster".into(),
            Object::Widget(window, id) => format!("{window}:{id}"),
        }
    }

    #[zbus(property)]
    fn help_text(&self) -> &str {
        ""
    }

    fn get_child_at_index(&self, child_index: i32) -> (ObjectRef,) {
        (self
            .get_children()
            .into_iter()
            .nth(child_index as usize)
            .unwrap_or_else(null_ref),)
    }

    fn get_children(&self) -> Vec<ObjectRef> {
        let state = self.0.service.state.read().unwrap();
        self.0
            .object
            .children(&state)
            .into_iter()
            .map(|object| self.0.reference(object))
            .collect()
    }

    fn get_index_in_parent(&self) -> i32 {
        let state = self.0.service.state.read().unwrap();
        self.0
            .object
            .parent(&state)
            .and_then(|parent| {
                parent
                    .children(&state)
                    .iter()
                    .position(|child| *child == self.0.object)
            })
            .map_or(-1, index)
    }

    fn get_role(&self) -> u32 {
        self.role().0
    }

    fn get_role_name(&self) -> &str {
        self.role().1
    }

    fn get_localized_role_name(&self) -> &str {
        self.get_role_name()
    }

    fn get_relation_set(&self) -> Vec<(u32, Vec<ObjectRef>)> {
        Vec::new()
    }

    fn get_state(&self) -> Vec<u32> {
        let mut bits = 1 << 8 | 1 << 24 | 1 << 25 | 1 << 30;
        if self.0.object != Object::Application {
            bits = self.0.read(|snapshot, node| {
                let mut bits = bits;
                if node.focusable {
                    bits |= 1 << 11;
                }
                if snapshot.tree.focus == Some(node.id) {
                    bits |= 1 << 12;
                }
                if node.selected {
                    bits |= 1 << 23;
                }
                match node.role {
                    Role::Window if snapshot.tree.focus.is_some() => bits |= 1 << 1,
                    Role::Tab | Role::ListItem => bits |= 1 << 22,
                    Role::Terminal => bits |= 1 << 17,
                    Role::Edit => bits |= 1 << 7 | 1 << 26,
                    _ => {}
                }
                bits
            });
        }
        let extra = self.0.read(|_, node| {
            if node.role == Role::Terminal && node.text.is_some() {
                1 << (38 - 32)
            } else {
                0
            }
        });
        vec![if bits == 0 { 1 << 6 } else { bits }, extra]
    }

    fn get_attributes(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn get_application(&self) -> (ObjectRef,) {
        (self.0.reference(Object::Application),)
    }

    fn get_interfaces(&self) -> Vec<&'static str> {
        let mut names = vec!["org.a11y.atspi.Accessible"];
        if self.0.object == Object::Application {
            names.push("org.a11y.atspi.Application");
        } else {
            let interfaces = self.0.read(|_, node| Interfaces::for_node(node));
            if interfaces.component {
                names.push("org.a11y.atspi.Component");
            }
            if interfaces.action {
                names.push("org.a11y.atspi.Action");
            }
            if interfaces.text {
                names.push("org.a11y.atspi.Text");
            }
            if interfaces.selection {
                names.push("org.a11y.atspi.Selection");
            }
        }
        names
    }
}

impl Accessible {
    fn role(&self) -> (u32, &'static str) {
        if self.0.object == Object::Application {
            return (75, "application");
        }
        self.0.read(|_, node| match node.role {
            Role::Window => (23, "frame"),
            Role::TabList => (38, "page tab list"),
            Role::Tab => (37, "page tab"),
            Role::Terminal => (60, "terminal"),
            Role::Dialog => (16, "dialog"),
            Role::Edit => (79, "entry"),
            Role::List => (31, "list"),
            Role::ListItem => (32, "list item"),
        })
    }
}

struct Component(Widget);

#[interface(name = "org.a11y.atspi.Component")]
impl Component {
    fn contains(&self, x: i32, y: i32, coords: u32) -> bool {
        self.0.origin(coords).is_some_and(|(dx, dy)| {
            self.0
                .read(|_, node| node.bounds.contains(x as f32 - dx, y as f32 - dy))
        })
    }

    fn get_accessible_at_point(&self, x: i32, y: i32, coords: u32) -> (ObjectRef,) {
        let (Object::Widget(window, id), Some((dx, dy))) = (self.0.object, self.0.origin(coords))
        else {
            return (null_ref(),);
        };
        (self
            .0
            .read(|snapshot, _| {
                let (x, y) = (x as f32 - dx, y as f32 - dy);
                hit_test(snapshot, id, x, y)
            })
            .map_or_else(null_ref, |id| self.0.reference(Object::Widget(window, id))),)
    }

    fn get_extents(&self, coords: u32) -> ((i32, i32, i32, i32),) {
        (self.0.extents(self.0.read(|_, node| node.bounds), coords),)
    }

    #[zbus(out_args("x", "y"))]
    fn get_position(&self, coords: u32) -> (i32, i32) {
        let ((x, y, _, _),) = self.get_extents(coords);
        (x, y)
    }

    #[zbus(out_args("width", "height"))]
    fn get_size(&self) -> (i32, i32) {
        let ((_, _, w, h),) = self.get_extents(1);
        (w, h)
    }

    fn get_layer(&self) -> u32 {
        self.0
            .read(|_, node| if node.role == Role::Window { 7 } else { 3 })
    }

    #[zbus(name = "GetMDIZOrder")]
    fn get_mdi_z_order(&self) -> i16 {
        0
    }

    fn grab_focus(&self) -> bool {
        self.0
            .apply(|node| node.focusable.then_some(Action::Focus(node.id)))
    }

    fn get_alpha(&self) -> f64 {
        1.
    }
}

fn hit_test(snapshot: &Snapshot, parent: NodeId, x: f32, y: f32) -> Option<NodeId> {
    snapshot
        .tree
        .nodes
        .iter()
        .rev()
        .filter(|node| node.parent == Some(parent) && node.bounds.contains(x, y))
        .map(|node| hit_test(snapshot, node.id, x, y).unwrap_or(node.id))
        .next()
}

struct Actions(Widget);

#[interface(name = "org.a11y.atspi.Action")]
impl Actions {
    #[zbus(property, name = "NActions")]
    fn n_actions(&self) -> i32 {
        i32::from(self.0.read(|_, node| node.invokable))
    }

    fn get_description(&self, _index: i32) -> &str {
        ""
    }

    fn get_name(&self, index: i32) -> &str {
        if index == 0 && self.n_actions() != 0 {
            "activate"
        } else {
            ""
        }
    }

    fn get_localized_name(&self, index: i32) -> &str {
        self.get_name(index)
    }

    fn get_key_binding(&self, _index: i32) -> &str {
        ""
    }

    fn get_actions(&self) -> Vec<(&str, &str, &str)> {
        (self.n_actions() != 0)
            .then_some(("activate", "", ""))
            .into_iter()
            .collect()
    }

    fn do_action(&self, index: i32) -> bool {
        index == 0
            && self
                .0
                .apply(|node| node.invokable.then_some(Action::Invoke(node.id)))
    }
}

struct Selection(Widget);

#[interface(name = "org.a11y.atspi.Selection")]
impl Selection {
    #[zbus(property, name = "NSelectedChildren")]
    fn n_selected_children(&self) -> i32 {
        index(self.selected().len())
    }

    fn get_selected_child(&self, child: i32) -> (ObjectRef,) {
        (self
            .selected()
            .into_iter()
            .nth(child as usize)
            .map_or_else(null_ref, |object| self.0.reference(object)),)
    }

    fn select_child(&self, child: i32) -> bool {
        let Object::Widget(window, _) = self.0.object else {
            return false;
        };
        let id = self
            .0
            .read(|snapshot, node| children(Some(snapshot), Some(node.id)).nth(child as usize));
        id.is_some_and(|id| (self.0.service.action)(window, Action::SelectItem(id)))
    }

    fn is_child_selected(&self, child: i32) -> bool {
        self.0.read(|snapshot, node| {
            children(Some(snapshot), Some(node.id))
                .nth(child as usize)
                .and_then(|id| snapshot.tree.node(id))
                .is_some_and(|child| child.selected)
        })
    }

    fn deselect_selected_child(&self, _child: i32) -> bool {
        false
    }

    fn deselect_child(&self, _child: i32) -> bool {
        false
    }

    fn select_all(&self) -> bool {
        false
    }

    fn clear_selection(&self) -> bool {
        false
    }
}

impl Selection {
    fn selected(&self) -> Vec<Object> {
        let Object::Widget(window, _) = self.0.object else {
            return Vec::new();
        };
        self.0.read(|snapshot, node| {
            snapshot
                .tree
                .nodes
                .iter()
                .filter(|child| child.parent == Some(node.id) && child.selected)
                .map(|child| Object::Widget(window, child.id))
                .collect()
        })
    }
}

use text::{text_change, AccessibleText};

#[cfg(test)]
#[path = "../test/accessibility.rs"]
mod tests;
