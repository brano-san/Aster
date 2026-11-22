use crate::config::Config;
use crate::i18n::t;
use crate::profile::{ProfileDefaults, ProfileGroup};
use crate::settings::{found, ProfileRow};
use std::collections::{BTreeMap, BTreeSet};

mod actions;
mod ssh_actions;
pub(crate) use actions::profile_templates;
pub(crate) mod editor;
pub(crate) mod layout;

pub(crate) const KINDS: [&str; 4] = ["local", "ssh", "serial", "socket"];

pub(crate) fn kind_label(kind: &str) -> &'static str {
    match kind {
        "ssh" => t("SSH"),
        "serial" => t("Serial"),
        "socket" => t("Network"),
        _ => t("Local"),
    }
}

fn group_key(group: &ProfileGroup) -> &str {
    if group.id.is_empty() { &group.name } else { &group.id }
}

fn resolve(groups: &[ProfileGroup], key: &str) -> Option<usize> {
    if key.is_empty() {
        return None;
    }
    groups.iter().position(|g| g.id == key).or_else(|| groups.iter().position(|g| g.name == key))
}

fn can_parent(groups: &[ProfileGroup], child: Option<usize>, mut parent: usize) -> bool {
    let mut seen = BTreeSet::new();
    loop {
        if Some(parent) == child || !seen.insert(parent) {
            return false;
        }
        match groups[parent].parent.as_deref().and_then(|key| resolve(groups, key)) {
            Some(next) => parent = next,
            None => return true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Header {
    Group(String),
    Ungrouped,
    Builtin,
    Imported,
    Serial,
}

impl Header {
    pub(crate) fn group_index(&self, groups: &[ProfileGroup]) -> Option<usize> {
        if let Self::Group(key) = self { resolve(groups, key) } else { None }
    }

    pub(crate) fn label<'a>(&'a self, groups: &'a [ProfileGroup]) -> &'a str {
        match self {
            Self::Group(key) => self.group_index(groups).map_or(key, |i| &groups[i].name),
            Self::Ungrouped => t("Ungrouped"),
            Self::Builtin => t("Built-in"),
            Self::Imported => t("From ~/.ssh/config"),
            Self::Serial => t("Serial ports"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TreeKind {
    Header(Header),
    Profile(usize),
    NewGroup,
    NewProfile,
    DefaultsHeader,
    Defaults(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TreeRow {
    pub kind: TreeKind,
    pub depth: usize,
}

impl TreeRow {
    pub(crate) fn indent(&self, scale: f32) -> f32 {
        16.0 * self.depth as f32 * scale
    }
}

fn profile_header(groups: &[ProfileGroup], row: &ProfileRow) -> Header {
    if row.custom {
        let group = row.group.as_deref().and_then(|name| groups.iter().find(|g| g.name == name.trim()));
        group.map_or(Header::Ungrouped, |g| Header::Group(group_key(g).into()))
    } else if row.id.starts_with("ssh:") {
        Header::Imported
    } else if row.id.starts_with("serial:") {
        Header::Serial
    } else {
        Header::Builtin
    }
}

pub(crate) fn tree(groups: &[ProfileGroup], profiles: &[ProfileRow], collapsed: &BTreeSet<Header>, query: &str) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    let mut members = BTreeMap::<_, Vec<_>>::new();
    for (i, p) in profiles.iter().enumerate() {
        members.entry(profile_header(groups, p)).or_default().push(i);
    }
    let mut sorted: Vec<_> = (0..groups.len()).collect();
    sorted.sort_by_cached_key(|&i| groups[i].name.to_lowercase());
    let mut children = vec![Vec::new(); groups.len()];
    let mut roots = Vec::new();
    for &i in &sorted {
        if let Some(parent) = groups[i].parent.as_deref().and_then(|key| resolve(groups, key)) {
            children[parent].push(i);
        } else {
            roots.push(i);
        }
    }
    let mut visited = vec![false; groups.len()];
    for root in roots.into_iter().chain(sorted) {
        let mut pending = vec![(root, 0)];
        while let Some((i, depth)) = pending.pop() {
            if std::mem::replace(&mut visited[i], true) {
                continue;
            }
            let header = Header::Group(group_key(&groups[i]).into());
            let profiles = members.remove(&header).unwrap_or_default();
            rows.push(TreeRow { kind: TreeKind::Header(header), depth });
            rows.extend(profiles.into_iter().map(|i| TreeRow { kind: TreeKind::Profile(i), depth: depth + 1 }));
            pending.extend(children[i].iter().rev().map(|&child| (child, depth + 1)));
        }
    }
    rows.push(TreeRow { kind: TreeKind::NewGroup, depth: 0 });
    for header in [Header::Ungrouped, Header::Builtin, Header::Imported, Header::Serial] {
        let ungrouped = header == Header::Ungrouped;
        if let Some(profiles) = members.remove(&header) {
            rows.push(TreeRow { kind: TreeKind::Header(header), depth: 0 });
            rows.extend(profiles.into_iter().map(|i| TreeRow { kind: TreeKind::Profile(i), depth: 1 }));
        } else if ungrouped {
            rows.push(TreeRow { kind: TreeKind::Header(header), depth: 0 });
        }
        if ungrouped {
            rows.push(TreeRow { kind: TreeKind::NewProfile, depth: 0 });
        }
    }
    rows.push(TreeRow { kind: TreeKind::DefaultsHeader, depth: 0 });
    rows.extend(KINDS.map(|key| TreeRow { kind: TreeKind::Defaults(key), depth: 1 }));
    if query.trim().is_empty() {
        let mut hidden_below = None;
        rows.retain(|row| {
            if hidden_below.is_some_and(|depth| depth < row.depth) {
                return false;
            }
            hidden_below = match &row.kind {
                TreeKind::Header(header) if collapsed.contains(header) => Some(row.depth),
                _ => None,
            };
            true
        });
    } else {
        let mut ancestors = Vec::new();
        let mut matched = vec![false; rows.len()];
        for (i, row) in rows.iter().enumerate() {
            ancestors.truncate(row.depth);
            let text = match &row.kind {
                TreeKind::Header(h) => h.label(groups).to_owned(),
                TreeKind::Profile(k) => {
                    let p = &profiles[*k];
                    format!("shell {} {} {} {}", p.name, p.id, p.detail, p.group.as_deref().unwrap_or_default())
                }
                TreeKind::NewGroup => format!("New group {}", t("New group")),
                TreeKind::NewProfile => format!("New profile add create {}", t("New profile")),
                TreeKind::DefaultsHeader => format!("Defaults for each type {}", t("Defaults for each type")),
                TreeKind::Defaults(kind) => format!("{kind} {} {}", kind_label(kind), t("Defaults for each type")),
            };
            if found(&format!("Profiles {} {text}", t("Profiles")), query) {
                matched[i] = true;
                for &parent in &ancestors {
                    matched[parent] = true;
                }
            }
            if matches!(row.kind, TreeKind::Header(_) | TreeKind::DefaultsHeader) {
                ancestors.push(i);
            }
        }
        rows = rows.into_iter().zip(matched).filter_map(|(row, keep)| keep.then_some(row)).collect();
    }
    rows
}

pub(crate) fn reveal_profile(id: &str, groups: &[ProfileGroup], profiles: &[ProfileRow], collapsed: &mut BTreeSet<Header>) -> bool {
    if collapsed.is_empty() { return false; }
    let Some(index) = profiles.iter().position(|p| p.id == id) else { return false };
    let mut ancestors = Vec::new();
    for row in tree(groups, profiles, &BTreeSet::new(), "") {
        ancestors.truncate(row.depth);
        match row.kind {
            TreeKind::Header(h) => ancestors.push(h),
            TreeKind::Profile(i) if i == index => {
                let count = collapsed.len();
                for h in &ancestors { collapsed.remove(h); }
                return collapsed.len() != count;
            }
            _ => {}
        }
    }
    false
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GroupEditor {
    pub original: Option<usize>,
    original_key: Option<String>,
    pub fields: [String; 4],
    pub defaults: BTreeMap<String, ProfileDefaults>,
    pub focus: usize,
    pub error: String,
}

impl GroupEditor {
    pub(crate) fn new(original: Option<usize>, groups: &[ProfileGroup]) -> Self {
        let group = original.and_then(|i| groups.get(i));
        let parent = group.and_then(|g| g.parent.as_deref());
        let parent = parent.and_then(|key| resolve(groups, key)).map(|i| group_key(&groups[i])).or(parent);
        Self {
            original,
            original_key: group.map(|g| group_key(g).into()),
            fields: [group.map_or("", |g| &g.name), parent.unwrap_or_default(), group.and_then(|g| g.color.as_deref()).unwrap_or_default(), group.and_then(|g| g.icon.as_deref()).unwrap_or_default()].map(str::to_owned),
            defaults: group.map(|g| g.defaults.clone()).unwrap_or_default(),
            focus: 0,
            error: String::new(),
        }
    }

    pub(crate) fn parent_choices(&self, groups: &[ProfileGroup]) -> Vec<(String, String)> {
        let mut choices: Vec<_> = groups.iter().enumerate().filter(|(i, _)| can_parent(groups, self.original, *i))
            .map(|(_, g)| (group_key(g).to_owned(), g.name.clone())).collect();
        choices.sort_by_cached_key(|(_, name)| name.to_lowercase());
        choices.insert(0, (String::new(), t("No parent group").into()));
        choices
    }

    pub(crate) fn parent_label<'a>(&'a self, groups: &'a [ProfileGroup]) -> &'a str {
        if self.fields[1].is_empty() { t("No parent group") } else { resolve(groups, &self.fields[1]).map_or(&self.fields[1], |i| &groups[i].name) }
    }

    pub(crate) fn cycle_parent(&mut self, groups: &[ProfileGroup], step: isize) {
        let choices = self.parent_choices(groups);
        let at = choices.iter().position(|(id, _)| id == &self.fields[1]).unwrap_or(0);
        self.fields[1].clone_from(&choices[(at as isize + step).rem_euclid(choices.len() as isize) as usize].0);
        self.error.clear();
    }

    pub(crate) fn save(&self, config: &mut Config) -> Result<usize, String> {
        let groups = &mut config.groups;
        if self.original.is_some_and(|i| groups.get(i).map(group_key) != self.original_key.as_deref()) {
            return Err(t("This group has changed").into());
        }
        let [name, parent, color, icon] = self.fields.each_ref().map(|v| v.trim());
        if name.is_empty() {
            return Err(t("The group needs a name").into());
        }
        if groups.iter().enumerate().any(|(i, g)| Some(i) != self.original && (g.name.eq_ignore_ascii_case(name) || g.id == name)) {
            return Err(t("A group with this name already exists").into());
        }
        if !color.is_empty() && !color.strip_prefix('#').is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())) {
            return Err(t("Enter a color as #RRGGBB").into());
        }
        if !parent.is_empty() {
            let Some(i) = resolve(groups, parent) else { return Err(t("Choose an existing parent group").into()) };
            if !can_parent(groups, self.original, i) {
                return Err(t("This parent would create a group cycle").into());
            }
        }
        let id = self.original.map(|i| groups[i].id.clone()).unwrap_or_else(|| {
            let base = format!("group:{name}");
            if resolve(groups, &base).is_none() { return base; }
            (2..).map(|i| format!("{base}:{i}")).find(|id| resolve(groups, id).is_none()).unwrap()
        });
        let value = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        let group = ProfileGroup { id, name: name.into(), parent: value(parent), icon: value(icon), color: value(color), defaults: self.defaults.clone() };
        if let Some(i) = self.original {
            if groups[i].name != name {
                for p in &mut config.profiles {
                    if p.group.as_deref().map(str::trim) == Some(groups[i].name.as_str()) {
                        p.group = Some(name.into());
                    }
                }
                let children = group_children(groups, i);
                for child in children {
                    groups[child].parent = Some(group_key(&group).into());
                }
            }
            groups[i] = group;
            Ok(i)
        } else {
            groups.push(group);
            Ok(groups.len() - 1)
        }
    }
}

fn group_children(groups: &[ProfileGroup], parent: usize) -> Vec<usize> {
    groups.iter().enumerate().filter_map(|(i, g)| {
        (g.parent.as_deref().and_then(|key| resolve(groups, key)) == Some(parent)).then_some(i)
    }).collect()
}

pub(crate) fn delete_group(config: &mut Config, index: usize) {
    let Some(group) = config.groups.get(index) else { return };
    for profile in &mut config.profiles {
        if profile.group.as_deref().map(str::trim) == Some(group.name.as_str()) {
            profile.group = None;
        }
    }
    let parent = group.parent.as_ref().filter(|key| resolve(&config.groups, key).is_some_and(|i| can_parent(&config.groups, Some(index), i))).cloned();
    for child in group_children(&config.groups, index) {
        config.groups[child].parent.clone_from(&parent);
    }
    config.groups.remove(index);
}

#[cfg(test)]
#[path = "../test/profiles_page.rs"]
mod tests;
