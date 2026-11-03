//! Author tree and the AccessKit tree built from it.
//!
//! The forest is process-global and may be read from an assistive-technology
//! thread. Callers must not hold its mutex across a V8 call.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use accesskit::{
  Action, ActionData, ActionRequest, Live, Node, NodeId, Orientation, Rect,
  Role, Toggled, TreeId, TreeInfo, TreeUpdate,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionKind {
  Click,
  ContextMenu,
  Decrement,
  Focus,
  Increment,
  ScrollIntoView,
  SetValue,
}

#[derive(Debug)]
pub(super) enum Fail {
  AlreadyAttached,
  Closed,
  NotFocusable,
  NotConnected,
  Ancestor,
  RootMove,
  OffsetParent,
  NotChild,
  Missing,
}

#[derive(Default)]
pub(super) struct Author {
  pub(super) parent: Option<u64>,
  pub(super) children: Vec<u64>,
  pub(super) role: Option<String>,
  pub(super) name: Option<String>,
  pub(super) description: Option<String>,
  pub(super) role_description: Option<String>,
  pub(super) value_text: Option<String>,
  pub(super) checked: Option<String>,
  pub(super) pressed: Option<String>,
  pub(super) disabled: Option<bool>,
  pub(super) expanded: Option<bool>,
  pub(super) hidden: Option<bool>,
  pub(super) selected: Option<bool>,
  pub(super) busy: Option<bool>,
  pub(super) modal: Option<bool>,
  pub(super) required: Option<bool>,
  pub(super) read_only: Option<bool>,
  pub(super) multiselectable: Option<bool>,
  pub(super) multiline: Option<bool>,
  pub(super) value_now: Option<f64>,
  pub(super) value_min: Option<f64>,
  pub(super) value_max: Option<f64>,
  pub(super) col_count: Option<i32>,
  pub(super) row_count: Option<i32>,
  pub(super) col_index: Option<i32>,
  pub(super) col_span: Option<i32>,
  pub(super) row_index: Option<i32>,
  pub(super) row_span: Option<i32>,
  pub(super) level: Option<i32>,
  pub(super) set_size: Option<i32>,
  pub(super) pos_in_set: Option<i32>,
  pub(super) live: Option<String>,
  pub(super) orientation: Option<String>,
  pub(super) focusable: bool,
  pub(super) offset_left: Option<f64>,
  pub(super) offset_top: Option<f64>,
  pub(super) offset_width: Option<f64>,
  pub(super) offset_height: Option<f64>,
  pub(super) offset_parent: Option<u64>,
}

#[derive(Clone)]
struct WindowRec {
  chrome: u64,
  root: Option<u64>,
  focus: Option<u64>,
  title: String,
  css_width: f64,
  css_height: f64,
  scale: f64,
  closed: bool,
}

pub(super) struct Pending {
  pub(super) window: u64,
  pub(super) node: u64,
  pub(super) kind: ActionKind,
  pub(super) value: Option<f64>,
}

struct Forest {
  nodes: HashMap<u64, Author>,
  windows: HashMap<u64, WindowRec>,
  pending: Vec<Pending>,
}

static FOREST: LazyLock<Mutex<Forest>> = LazyLock::new(|| {
  Mutex::new(Forest {
    nodes: HashMap::new(),
    windows: HashMap::new(),
    pending: Vec::new(),
  })
});

static NEXT: AtomicU64 = AtomicU64::new(1);

fn lock() -> std::sync::MutexGuard<'static, Forest> {
  FOREST.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn allocate(forest: &Forest) -> u64 {
  loop {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    if id == 0 {
      continue;
    }
    if forest.nodes.contains_key(&id) {
      continue;
    }
    if forest.windows.values().any(|window| window.chrome == id) {
      continue;
    }
    return id;
  }
}

fn fresh_window(chrome: u64) -> WindowRec {
  WindowRec {
    chrome,
    root: None,
    focus: None,
    title: String::new(),
    css_width: 0.0,
    css_height: 0.0,
    scale: 1.0,
    closed: false,
  }
}

fn ensure_record(forest: &mut Forest, id: u64) {
  if forest.windows.contains_key(&id) {
    return;
  }
  let chrome = allocate(forest);
  forest.windows.insert(id, fresh_window(chrome));
}

pub(super) fn note_viewport(
  id: u64,
  title: String,
  css_width: f64,
  css_height: f64,
  scale: f64,
) {
  let mut forest = lock();
  ensure_record(&mut forest, id);
  let Some(window) = forest.windows.get_mut(&id) else {
    return;
  };
  window.title = title;
  window.css_width = finite_or_zero(css_width);
  window.css_height = finite_or_zero(css_height);
  window.scale = sane_scale(scale);
}

pub(super) fn note_size(id: u64, css_width: f64, css_height: f64, scale: f64) {
  let mut forest = lock();
  let Some(window) = forest.windows.get_mut(&id) else {
    return;
  };
  window.css_width = finite_or_zero(css_width);
  window.css_height = finite_or_zero(css_height);
  window.scale = sane_scale(scale);
}

pub(super) fn note_title(id: u64, title: String) {
  let mut forest = lock();
  let Some(window) = forest.windows.get_mut(&id) else {
    return;
  };
  window.title = title;
}

pub(super) fn create() -> u64 {
  let mut forest = lock();
  let id = allocate(&forest);
  forest.nodes.insert(id, Author::default());
  id
}

pub(super) fn clear() {
  let mut forest = lock();
  forest.nodes.clear();
  forest.windows.clear();
  forest.pending.clear();
}

pub(super) fn forget_author(id: u64) {
  let mut forest = lock();
  if !forest.nodes.contains_key(&id) {
    return;
  }
  clear_focus_under(&mut forest, id);
  let parent = forest.nodes.get(&id).and_then(|author| author.parent);
  let children = forest
    .nodes
    .get(&id)
    .map(|author| author.children.clone())
    .unwrap_or_default();
  if let Some(parent) = parent
    && let Some(author) = forest.nodes.get_mut(&parent)
  {
    author.children.retain(|child| *child != id);
  }
  if let Some(author) = forest.nodes.get_mut(&id) {
    author.parent = None;
  }
  for child in children {
    if let Some(author) = forest.nodes.get_mut(&child) {
      author.parent = None;
    }
  }
  for record in forest.windows.values_mut() {
    if record.root == Some(id) {
      record.root = None;
    }
  }
  for author in forest.nodes.values_mut() {
    if author.offset_parent == Some(id) {
      author.offset_parent = None;
    }
  }
  forest.nodes.remove(&id);
}

pub(super) fn has_root(id: u64) -> bool {
  lock()
    .windows
    .get(&id)
    .is_some_and(|window| window.root.is_some())
}

pub(super) fn root_id(id: u64) -> Option<u64> {
  lock().windows.get(&id).and_then(|window| window.root)
}

pub(super) fn is_closed(id: u64) -> bool {
  lock().windows.get(&id).is_some_and(|window| window.closed)
}

pub(super) fn release_window(id: u64) {
  let mut forest = lock();
  let Some(window) = forest.windows.get_mut(&id) else {
    return;
  };
  window.closed = true;
  window.focus = None;
}

pub(super) fn attach_root(window: u64, node: u64) -> Result<(), Fail> {
  let mut forest = lock();
  ensure_record(&mut forest, window);
  if forest
    .windows
    .get(&window)
    .is_some_and(|record| record.closed)
  {
    return Err(Fail::Closed);
  }
  if forest
    .windows
    .get(&window)
    .is_some_and(|record| record.root.is_some())
  {
    return Err(Fail::AlreadyAttached);
  }
  if !forest.nodes.contains_key(&node) {
    return Err(Fail::Missing);
  }
  if is_root(&forest, node) {
    return Err(Fail::RootMove);
  }
  if forest
    .nodes
    .get(&node)
    .is_some_and(|author| author.parent.is_some())
  {
    return Err(Fail::Ancestor);
  }
  if let Some(record) = forest.windows.get_mut(&window) {
    record.root = Some(node);
  }
  Ok(())
}

pub(super) fn append_child(parent: u64, node: u64) -> Result<Vec<u64>, Fail> {
  let mut forest = lock();
  require_node(&forest, parent)?;
  require_node(&forest, node)?;
  pre_insert(&mut forest, parent, node, None)
}

pub(super) fn insert_before(
  parent: u64,
  node: u64,
  before: Option<u64>,
) -> Result<Vec<u64>, Fail> {
  let mut forest = lock();
  require_node(&forest, parent)?;
  require_node(&forest, node)?;
  let Some(before) = before else {
    return pre_insert(&mut forest, parent, node, None);
  };
  require_node(&forest, before)?;
  if before == node {
    if !is_child(&forest, parent, node) {
      return Err(Fail::NotChild);
    }
    let next = next_sibling(&forest, parent, node);
    return pre_insert(&mut forest, parent, node, next);
  }
  if !is_child(&forest, parent, before) {
    return Err(Fail::NotChild);
  }
  pre_insert(&mut forest, parent, node, Some(before))
}

pub(super) fn replace_child(
  parent: u64,
  node: u64,
  child: u64,
) -> Result<Vec<u64>, Fail> {
  let mut forest = lock();
  require_node(&forest, parent)?;
  require_node(&forest, node)?;
  require_node(&forest, child)?;
  let child_of_parent = is_child(&forest, parent, child);
  if node == child {
    if child_of_parent {
      return Ok(Vec::new());
    }
    return Err(Fail::NotChild);
  }
  if !child_of_parent {
    return Err(Fail::NotChild);
  }
  if is_root(&forest, node) {
    return Err(Fail::RootMove);
  }
  if is_ancestor(&forest, node, parent) {
    return Err(Fail::Ancestor);
  }
  let old_parent = forest.nodes.get(&node).and_then(|author| author.parent);
  detach(&mut forest, node);
  let Some(index) = forest
    .nodes
    .get(&parent)
    .and_then(|author| author.children.iter().position(|item| *item == child))
  else {
    return Err(Fail::NotChild);
  };
  if let Some(author) = forest.nodes.get_mut(&parent) {
    author.children[index] = node;
  }
  if let Some(author) = forest.nodes.get_mut(&node) {
    author.parent = Some(parent);
  }
  clear_focus_under(&mut forest, child);
  if let Some(author) = forest.nodes.get_mut(&child) {
    author.parent = None;
  }
  let mut touched = vec![parent, node, child];
  if let Some(old) = old_parent
    && old != parent
    && old != child
  {
    touched.push(old);
  }
  Ok(touched)
}

pub(super) fn remove_child(parent: u64, node: u64) -> Result<Vec<u64>, Fail> {
  let mut forest = lock();
  require_node(&forest, parent)?;
  require_node(&forest, node)?;
  if !is_child(&forest, parent, node) {
    return Err(Fail::NotChild);
  }
  clear_focus_under(&mut forest, node);
  detach(&mut forest, node);
  Ok(vec![parent, node])
}

pub(super) fn set_offset_parent(
  node: u64,
  parent: Option<u64>,
) -> Result<Option<u64>, Fail> {
  let mut forest = lock();
  if !forest.nodes.contains_key(&node) {
    return Err(Fail::Missing);
  }
  if let Some(parent) = parent {
    if !forest.nodes.contains_key(&parent) {
      return Err(Fail::Missing);
    }
    if !is_strict_ancestor(&forest, parent, node) {
      return Err(Fail::OffsetParent);
    }
  }
  if let Some(author) = forest.nodes.get_mut(&node) {
    author.offset_parent = parent;
  }
  Ok(window_of(&forest, node, false))
}

pub(super) fn focus(id: u64) -> Result<u64, Fail> {
  let mut forest = lock();
  let Some(author) = forest.nodes.get(&id) else {
    return Err(Fail::Missing);
  };
  if !author.focusable {
    return Err(Fail::NotFocusable);
  }
  let Some(window) = window_of(&forest, id, true) else {
    return Err(Fail::NotConnected);
  };
  if forest
    .windows
    .get(&window)
    .is_some_and(|record| record.closed)
  {
    return Err(Fail::Closed);
  }
  if let Some(record) = forest.windows.get_mut(&window) {
    record.focus = Some(id);
  }
  Ok(window)
}

pub(super) fn map<T: Default>(id: u64, read: impl FnOnce(&Author) -> T) -> T {
  let forest = lock();
  match forest.nodes.get(&id) {
    Some(author) => read(author),
    None => T::default(),
  }
}

pub(super) fn update(id: u64, write: impl FnOnce(&mut Author)) -> Option<u64> {
  let mut forest = lock();
  {
    let Some(author) = forest.nodes.get_mut(&id) else {
      return None;
    };
    write(author);
  }
  window_of(&forest, id, false)
}

pub(super) fn links(id: u64) -> (Option<u64>, Vec<u64>) {
  let forest = lock();
  let Some(author) = forest.nodes.get(&id) else {
    return (None, Vec::new());
  };
  (author.parent, author.children.clone())
}

pub(super) fn edge_child(id: u64, last: bool) -> Option<u64> {
  let forest = lock();
  let children = &forest.nodes.get(&id)?.children;
  if last {
    children.last().copied()
  } else {
    children.first().copied()
  }
}

pub(super) fn neighbor(id: u64, previous: bool) -> Option<u64> {
  let forest = lock();
  let parent = forest.nodes.get(&id)?.parent?;
  let children = &forest.nodes.get(&parent)?.children;
  let index = children.iter().position(|child| *child == id)?;
  if previous {
    index
      .checked_sub(1)
      .and_then(|index| children.get(index).copied())
  } else {
    children.get(index + 1).copied()
  }
}

pub(super) fn ancestor_ids(id: u64) -> Vec<u64> {
  let forest = lock();
  let mut path = Vec::new();
  let mut current = Some(id);
  let mut seen = HashSet::new();
  while let Some(node) = current {
    if !forest.nodes.contains_key(&node) || !seen.insert(node) {
      break;
    }
    path.push(node);
    current = forest.nodes.get(&node).and_then(|author| author.parent);
  }
  path
}

pub(super) fn open_window(id: u64) -> Option<u64> {
  window_of(&lock(), id, false)
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn connected_window(id: u64) -> Option<u64> {
  window_of(&lock(), id, true)
}

pub(super) fn enqueue(request: ActionRequest) {
  let kind = match request.action {
    Action::Click => ActionKind::Click,
    Action::ShowContextMenu => ActionKind::ContextMenu,
    Action::Decrement => ActionKind::Decrement,
    Action::Focus => ActionKind::Focus,
    Action::Increment => ActionKind::Increment,
    Action::ScrollIntoView => ActionKind::ScrollIntoView,
    Action::SetValue => ActionKind::SetValue,
    _ => return,
  };
  let value = if kind == ActionKind::SetValue {
    number_from(request.data.as_ref())
  } else {
    None
  };
  push_pending(request.target_node.0, kind, value);
}

pub(super) fn push_pending(node: u64, kind: ActionKind, value: Option<f64>) {
  let mut forest = lock();
  let Some(window) = window_of(&forest, node, false) else {
    return;
  };
  if kind == ActionKind::Focus
    && let Some(record) = forest.windows.get_mut(&window)
  {
    record.focus = Some(node);
  }
  let value = value.filter(|number| number.is_finite());
  forest.pending.push(Pending {
    window,
    node,
    kind,
    value,
  });
}

pub(super) fn take_pending() -> Vec<Pending> {
  let mut forest = lock();
  std::mem::take(&mut forest.pending)
}

pub(super) fn full_tree(window_id: u64) -> TreeUpdate {
  let mut forest = lock();
  ensure_record(&mut forest, window_id);
  let record = forest
    .windows
    .get(&window_id)
    .expect("window record")
    .clone();
  let mut chrome = Node::new(Role::Window);
  chrome.set_label(record.title.clone());
  chrome.set_bounds(Rect::new(
    0.0,
    0.0,
    phys(record.css_width, record.scale),
    phys(record.css_height, record.scale),
  ));
  let mut nodes = Vec::new();
  let mut published = HashSet::new();
  if let Some(root) = record.root
    && forest.nodes.contains_key(&root)
  {
    chrome.set_children(vec![NodeId(root)]);
    walk(&forest, root, record.scale, &mut nodes, &mut published);
  }
  nodes.insert(0, (NodeId(record.chrome), chrome));
  let focus = match record.focus {
    Some(id) if published.contains(&id) => NodeId(id),
    _ => NodeId(record.chrome),
  };
  let mut tree = TreeInfo::new(NodeId(record.chrome));
  tree.toolkit_name = Some("Yuzuko".to_string());
  TreeUpdate {
    nodes,
    tree: Some(tree),
    tree_id: TreeId::ROOT,
    focus,
  }
}

fn number_from(data: Option<&ActionData>) -> Option<f64> {
  match data {
    Some(ActionData::NumericValue(value)) if value.is_finite() => Some(*value),
    Some(ActionData::NumericValue(_)) => None,
    Some(ActionData::Value(text)) => {
      text.parse().ok().filter(|value: &f64| value.is_finite())
    }
    _ => None,
  }
}

fn require_node(forest: &Forest, id: u64) -> Result<(), Fail> {
  if forest.nodes.contains_key(&id) {
    Ok(())
  } else {
    Err(Fail::Missing)
  }
}

fn pre_insert(
  forest: &mut Forest,
  parent: u64,
  node: u64,
  before: Option<u64>,
) -> Result<Vec<u64>, Fail> {
  if is_root(forest, node) {
    return Err(Fail::RootMove);
  }
  if is_ancestor(forest, node, parent) {
    return Err(Fail::Ancestor);
  }
  let old_parent = forest.nodes.get(&node).and_then(|author| author.parent);
  detach(forest, node);
  insert_at(forest, parent, node, before);
  let mut touched = vec![parent, node];
  if let Some(old) = old_parent
    && old != parent
  {
    touched.push(old);
  }
  Ok(touched)
}

fn is_root(forest: &Forest, id: u64) -> bool {
  forest
    .windows
    .values()
    .any(|window| window.root == Some(id))
}

fn is_child(forest: &Forest, parent: u64, node: u64) -> bool {
  forest
    .nodes
    .get(&parent)
    .is_some_and(|author| author.children.contains(&node))
}

fn next_sibling(forest: &Forest, parent: u64, node: u64) -> Option<u64> {
  let children = &forest.nodes.get(&parent)?.children;
  let index = children.iter().position(|child| *child == node)?;
  children.get(index + 1).copied()
}

fn detach(forest: &mut Forest, node: u64) {
  let parent = forest.nodes.get(&node).and_then(|author| author.parent);
  if let Some(parent) = parent
    && let Some(author) = forest.nodes.get_mut(&parent)
  {
    author.children.retain(|child| *child != node);
  }
  if let Some(author) = forest.nodes.get_mut(&node) {
    author.parent = None;
  }
}

fn insert_at(forest: &mut Forest, parent: u64, node: u64, before: Option<u64>) {
  if let Some(author) = forest.nodes.get_mut(&node) {
    author.parent = Some(parent);
  }
  let Some(author) = forest.nodes.get_mut(&parent) else {
    return;
  };
  if let Some(before) = before
    && let Some(index) =
      author.children.iter().position(|child| *child == before)
  {
    author.children.insert(index, node);
    return;
  }
  author.children.push(node);
}

fn clear_focus_under(forest: &mut Forest, id: u64) {
  let focused: Vec<(u64, u64)> = forest
    .windows
    .iter()
    .filter_map(|(window, record)| Some((*window, record.focus?)))
    .collect();
  for (window, focus) in focused {
    if is_ancestor(forest, id, focus)
      && let Some(record) = forest.windows.get_mut(&window)
    {
      record.focus = None;
    }
  }
}

fn is_ancestor(forest: &Forest, ancestor: u64, id: u64) -> bool {
  let mut current = Some(id);
  let mut seen = HashSet::new();
  while let Some(node) = current {
    if !seen.insert(node) {
      return false;
    }
    if node == ancestor {
      return true;
    }
    current = forest.nodes.get(&node).and_then(|author| author.parent);
  }
  false
}

fn is_strict_ancestor(forest: &Forest, ancestor: u64, id: u64) -> bool {
  ancestor != id && is_ancestor(forest, ancestor, id)
}

fn top_of(forest: &Forest, id: u64) -> Option<u64> {
  if !forest.nodes.contains_key(&id) {
    return None;
  }
  let mut current = id;
  let mut seen = HashSet::new();
  loop {
    if !seen.insert(current) {
      return None;
    }
    match forest.nodes.get(&current).and_then(|author| author.parent) {
      Some(parent) => current = parent,
      None => return Some(current),
    }
  }
}

fn window_of(forest: &Forest, id: u64, closed_ok: bool) -> Option<u64> {
  let top = top_of(forest, id)?;
  forest.windows.iter().find_map(|(window, record)| {
    (record.root == Some(top) && (closed_ok || !record.closed))
      .then_some(*window)
  })
}

fn walk(
  forest: &Forest,
  id: u64,
  scale: f64,
  nodes: &mut Vec<(NodeId, Node)>,
  published: &mut HashSet<u64>,
) {
  if !forest.nodes.contains_key(&id) || !published.insert(id) {
    return;
  }
  let children = child_ids(forest, id);
  nodes.push((NodeId(id), fill(forest, id, scale, &children)));
  for child in children {
    walk(forest, child, scale, nodes, published);
  }
}

fn child_ids(forest: &Forest, id: u64) -> Vec<u64> {
  forest
    .nodes
    .get(&id)
    .map(|author| {
      author
        .children
        .iter()
        .copied()
        .filter(|child| forest.nodes.contains_key(child))
        .collect()
    })
    .unwrap_or_default()
}

fn fill(forest: &Forest, id: u64, scale: f64, children: &[u64]) -> Node {
  let bounds = bounds_of(forest, id, scale);
  let author = forest.nodes.get(&id).expect("author");
  let mut node = Node::new(accesskit_role(author));
  node.set_bounds(bounds);
  node.set_children(children.iter().copied().map(NodeId).collect::<Vec<_>>());
  apply_author(&mut node, author);
  node
}

fn apply_author(node: &mut Node, author: &Author) {
  if let Some(name) = &author.name {
    node.set_label(name.clone());
  }
  if let Some(description) = &author.description {
    node.set_description(description.clone());
  }
  if let Some(text) = &author.role_description {
    node.set_role_description(text.clone());
  }
  if let Some(value) = &author.value_text {
    node.set_value(value.clone());
  }
  if let Some(value) = author.value_now {
    node.set_numeric_value(value);
  }
  if let Some(value) = author.value_min {
    node.set_min_numeric_value(value);
  }
  if let Some(value) = author.value_max {
    node.set_max_numeric_value(value);
  }
  publish_toggled(node, author);
  if author.hidden == Some(true) {
    node.set_hidden();
  }
  if author.disabled == Some(true) {
    node.set_disabled();
  }
  if author.busy == Some(true) {
    node.set_busy();
  }
  if author.modal == Some(true) {
    node.set_modal();
  }
  if author.required == Some(true) {
    node.set_required();
  }
  if author.read_only == Some(true) {
    node.set_read_only();
  }
  if author.multiselectable == Some(true) {
    node.set_multiselectable();
  }
  if let Some(expanded) = author.expanded {
    node.set_expanded(expanded);
  }
  if let Some(selected) = author.selected {
    node.set_selected(selected);
  }
  if let Some(value) = index_value(author.row_index) {
    node.set_row_index(value);
  }
  if let Some(value) = index_value(author.col_index) {
    node.set_column_index(value);
  }
  if let Some(value) = index_value(author.level) {
    node.set_level(value);
  }
  if let Some(value) = index_value(author.pos_in_set) {
    node.set_position_in_set(value);
  }
  if let Some(value) = count_value(author.row_span) {
    node.set_row_span(value);
  }
  if let Some(value) = count_value(author.col_span) {
    node.set_column_span(value);
  }
  if let Some(value) = count_value(author.row_count) {
    node.set_row_count(value);
  }
  if let Some(value) = count_value(author.col_count) {
    node.set_column_count(value);
  }
  if let Some(value) = count_value(author.set_size) {
    node.set_size_of_set(value);
  }
  match author.live.as_deref() {
    Some("off") => node.set_live(Live::Off),
    Some("polite") => node.set_live(Live::Polite),
    Some("assertive") => node.set_live(Live::Assertive),
    _ => {}
  }
  match author.orientation.as_deref() {
    Some("horizontal") => node.set_orientation(Orientation::Horizontal),
    Some("vertical") => node.set_orientation(Orientation::Vertical),
    _ => {}
  }
  add_actions(node, author);
}

fn publish_toggled(node: &mut Node, author: &Author) {
  let (checked, pressed) = toggled_pair(author);
  let toggled = match checked.or(pressed) {
    Some("true") => Some(Toggled::True),
    Some("false") => Some(Toggled::False),
    Some("mixed") => Some(Toggled::Mixed),
    _ => None,
  };
  if let Some(toggled) = toggled {
    node.set_toggled(toggled);
  }
}

fn toggle_token(value: Option<&str>) -> Option<&str> {
  match value {
    Some("true" | "false" | "mixed") => value,
    _ => None,
  }
}

fn toggled_pair(author: &Author) -> (Option<&str>, Option<&str>) {
  let role = role_token(author);
  if matches!(
    role,
    "checkbox"
      | "radio"
      | "menuitemcheckbox"
      | "menuitemradio"
      | "switch"
      | "option"
  ) {
    (toggle_token(author.checked.as_deref()), None)
  } else if role == "button" {
    (None, toggle_token(author.pressed.as_deref()))
  } else if let Some(checked) = toggle_token(author.checked.as_deref()) {
    (Some(checked), None)
  } else {
    (None, toggle_token(author.pressed.as_deref()))
  }
}

fn add_actions(node: &mut Node, author: &Author) {
  let role = role_token(author);
  if matches!(
    role,
    "button"
      | "link"
      | "checkbox"
      | "radio"
      | "tab"
      | "menuitem"
      | "menuitemcheckbox"
      | "menuitemradio"
      | "switch"
      | "option"
      | "treeitem"
  ) {
    node.add_action(Action::Click);
  }
  if matches!(role, "slider" | "scrollbar" | "spinbutton") {
    node.add_action(Action::Increment);
    node.add_action(Action::Decrement);
    node.add_action(Action::SetValue);
  }
  if matches!(role, "textbox" | "searchbox") {
    node.add_action(Action::SetValue);
  }
  if author.focusable {
    node.add_action(Action::Focus);
  }
  if author.expanded.is_some() {
    node.add_action(Action::Expand);
    node.add_action(Action::Collapse);
  }
  node.add_action(Action::ShowContextMenu);
  node.add_action(Action::ScrollIntoView);
}

fn role_token(author: &Author) -> &str {
  author.role.as_deref().unwrap_or("")
}

fn accesskit_role(author: &Author) -> Role {
  match author.role.as_deref() {
    None | Some("") | Some("generic") | Some("none") | Some("presentation") => {
      Role::GenericContainer
    }
    Some("alert") => Role::Alert,
    Some("alertdialog") => Role::AlertDialog,
    Some("application") => Role::Application,
    Some("article") => Role::Article,
    Some("banner") => Role::Banner,
    Some("blockquote") => Role::Blockquote,
    Some("button") => Role::Button,
    Some("caption") => Role::Caption,
    Some("cell") => Role::Cell,
    Some("checkbox") => Role::CheckBox,
    Some("code") => Role::Code,
    Some("columnheader") => Role::ColumnHeader,
    Some("combobox") => Role::ComboBox,
    Some("complementary") => Role::Complementary,
    Some("contentinfo") => Role::ContentInfo,
    Some("definition") => Role::Definition,
    Some("dialog") => Role::Dialog,
    Some("directory") => Role::List,
    Some("document") => Role::Document,
    Some("emphasis") => Role::Emphasis,
    Some("feed") => Role::Feed,
    Some("figure") => Role::Figure,
    Some("form") => Role::Form,
    Some("grid") => Role::Grid,
    Some("gridcell") => Role::GridCell,
    Some("group") => Role::Group,
    Some("heading") => Role::Heading,
    Some("image" | "img") => Role::Image,
    Some("link") => Role::Link,
    Some("list") => Role::List,
    Some("listbox") => Role::ListBox,
    Some("listitem") => Role::ListItem,
    Some("log") => Role::Log,
    Some("main") => Role::Main,
    Some("marquee") => Role::Marquee,
    Some("math") => Role::Math,
    Some("menu") => Role::Menu,
    Some("menubar") => Role::MenuBar,
    Some("menuitem") => Role::MenuItem,
    Some("menuitemcheckbox") => Role::MenuItemCheckBox,
    Some("menuitemradio") => Role::MenuItemRadio,
    Some("meter") => Role::Meter,
    Some("navigation") => Role::Navigation,
    Some("note") => Role::Note,
    Some("option") => Role::ListBoxOption,
    Some("paragraph") => Role::Paragraph,
    Some("progressbar") => Role::ProgressIndicator,
    Some("radio") => Role::RadioButton,
    Some("radiogroup") => Role::RadioGroup,
    Some("region") => Role::Region,
    Some("row") => Role::Row,
    Some("rowgroup") => Role::RowGroup,
    Some("rowheader") => Role::RowHeader,
    Some("scrollbar") => Role::ScrollBar,
    Some("search") => Role::Search,
    Some("searchbox") => Role::SearchInput,
    Some("separator") => Role::Splitter,
    Some("slider") => Role::Slider,
    Some("spinbutton") => Role::SpinButton,
    Some("status") => Role::Status,
    Some("strong") => Role::Strong,
    Some("switch") => Role::Switch,
    Some("tab") => Role::Tab,
    Some("table") => Role::Table,
    Some("tablist") => Role::TabList,
    Some("tabpanel") => Role::TabPanel,
    Some("term") => Role::Term,
    Some("textbox") => {
      if author.multiline == Some(true) {
        Role::MultilineTextInput
      } else {
        Role::TextInput
      }
    }
    Some("time") => Role::Time,
    Some("timer") => Role::Timer,
    Some("toolbar") => Role::Toolbar,
    Some("tooltip") => Role::Tooltip,
    Some("tree") => Role::Tree,
    Some("treegrid") => Role::TreeGrid,
    Some("treeitem") => Role::TreeItem,
    Some("window") => Role::Window,
    Some(_) => Role::Unknown,
  }
}

fn index_value(value: Option<i32>) -> Option<usize> {
  match value {
    Some(value) if value >= 1 => Some((value as usize) - 1),
    Some(0) => Some(0),
    _ => None,
  }
}

fn count_value(value: Option<i32>) -> Option<usize> {
  match value {
    Some(value) if value >= 0 => Some(value as usize),
    _ => None,
  }
}

/// One node in the published tree, in the units `ComputedAccessibleNode` reads.
#[derive(Clone)]
pub(super) struct ComputedNode {
  pub(super) parent: Option<u64>,
  pub(super) children: Vec<u64>,
  pub(super) role: String,
  pub(super) name: Option<String>,
  pub(super) description: Option<String>,
  pub(super) role_description: Option<String>,
  pub(super) value_text: Option<String>,
  pub(super) checked: Option<String>,
  pub(super) pressed: Option<String>,
  pub(super) disabled: Option<bool>,
  pub(super) expanded: Option<bool>,
  pub(super) hidden: Option<bool>,
  pub(super) selected: Option<bool>,
  pub(super) busy: Option<bool>,
  pub(super) modal: Option<bool>,
  pub(super) required: Option<bool>,
  pub(super) read_only: Option<bool>,
  pub(super) multiselectable: Option<bool>,
  pub(super) multiline: Option<bool>,
  pub(super) value_now: Option<f64>,
  pub(super) value_min: Option<f64>,
  pub(super) value_max: Option<f64>,
  pub(super) col_count: Option<i32>,
  pub(super) row_count: Option<i32>,
  pub(super) col_index: Option<i32>,
  pub(super) col_span: Option<i32>,
  pub(super) row_index: Option<i32>,
  pub(super) row_span: Option<i32>,
  pub(super) level: Option<i32>,
  pub(super) set_size: Option<i32>,
  pub(super) pos_in_set: Option<i32>,
  pub(super) live: Option<String>,
  pub(super) orientation: Option<String>,
  pub(super) focusable: bool,
  pub(super) offset_left: f64,
  pub(super) offset_top: f64,
  pub(super) offset_width: f64,
  pub(super) offset_height: f64,
  pub(super) offset_parent: Option<u64>,
}

pub(super) fn snapshot(window_id: u64) -> HashMap<u64, ComputedNode> {
  let mut forest = lock();
  ensure_record(&mut forest, window_id);
  let record = forest
    .windows
    .get(&window_id)
    .expect("window record")
    .clone();
  let mut nodes = HashMap::new();
  let mut published = HashSet::new();
  if let Some(root) = record.root
    && forest.nodes.contains_key(&root)
  {
    fill_computed(&forest, root, record.chrome, &mut nodes, &mut published);
  }
  let root = record.root.filter(|id| published.contains(id));
  nodes.insert(record.chrome, chrome_node(&record, root));
  nodes
}

fn fill_computed(
  forest: &Forest,
  id: u64,
  parent: u64,
  nodes: &mut HashMap<u64, ComputedNode>,
  published: &mut HashSet<u64>,
) {
  if !forest.nodes.contains_key(&id) || !published.insert(id) {
    return;
  }
  let children = child_ids(forest, id);
  nodes.insert(id, computed_author(forest, id, parent, &children));
  for child in children {
    fill_computed(forest, child, id, nodes, published);
  }
}

fn chrome_node(record: &WindowRec, root: Option<u64>) -> ComputedNode {
  ComputedNode {
    parent: None,
    children: root.into_iter().collect(),
    role: "window".to_string(),
    name: Some(record.title.clone()),
    description: None,
    role_description: None,
    value_text: None,
    checked: None,
    pressed: None,
    disabled: None,
    expanded: None,
    hidden: None,
    selected: None,
    busy: None,
    modal: None,
    required: None,
    read_only: None,
    multiselectable: None,
    multiline: None,
    value_now: None,
    value_min: None,
    value_max: None,
    col_count: None,
    row_count: None,
    col_index: None,
    col_span: None,
    row_index: None,
    row_span: None,
    level: None,
    set_size: None,
    pos_in_set: None,
    live: None,
    orientation: None,
    focusable: false,
    offset_left: 0.0,
    offset_top: 0.0,
    offset_width: record.css_width,
    offset_height: record.css_height,
    offset_parent: None,
  }
}

fn computed_author(
  forest: &Forest,
  id: u64,
  parent: u64,
  children: &[u64],
) -> ComputedNode {
  let author = forest.nodes.get(&id).expect("author");
  let role = accesskit_role(author);
  let (checked, pressed) = toggled_pair(author);
  let offset_parent = author
    .offset_parent
    .filter(|ancestor| is_strict_ancestor(forest, *ancestor, id));
  ComputedNode {
    parent: Some(parent),
    children: children.to_vec(),
    role: computed_role_name(role).to_string(),
    name: author.name.clone(),
    description: author.description.clone(),
    role_description: author.role_description.clone(),
    value_text: author.value_text.clone(),
    checked: checked.map(str::to_string),
    pressed: pressed.map(str::to_string),
    disabled: published_flag(author.disabled),
    expanded: author.expanded,
    hidden: published_flag(author.hidden),
    selected: author.selected,
    busy: published_flag(author.busy),
    modal: published_flag(author.modal),
    required: published_flag(author.required),
    read_only: published_flag(author.read_only),
    multiselectable: published_flag(author.multiselectable),
    multiline: matches!(role, Role::MultilineTextInput).then_some(true),
    value_now: author.value_now,
    value_min: author.value_min,
    value_max: author.value_max,
    col_count: published_count(author.col_count),
    row_count: published_count(author.row_count),
    col_index: published_index(author.col_index),
    col_span: published_count(author.col_span),
    row_index: published_index(author.row_index),
    row_span: published_count(author.row_span),
    level: published_index(author.level),
    set_size: published_count(author.set_size),
    pos_in_set: published_index(author.pos_in_set),
    live: author.live.clone(),
    orientation: author.orientation.clone(),
    focusable: author.focusable,
    offset_left: finite_or_zero(author.offset_left.unwrap_or(0.0)),
    offset_top: finite_or_zero(author.offset_top.unwrap_or(0.0)),
    offset_width: finite_or_zero(author.offset_width.unwrap_or(0.0)),
    offset_height: finite_or_zero(author.offset_height.unwrap_or(0.0)),
    offset_parent,
  }
}

fn published_flag(value: Option<bool>) -> Option<bool> {
  (value == Some(true)).then_some(true)
}

fn published_index(value: Option<i32>) -> Option<i32> {
  index_value(value).and_then(|value| i32::try_from(value).ok())
}

fn published_count(value: Option<i32>) -> Option<i32> {
  count_value(value).and_then(|value| i32::try_from(value).ok())
}

fn computed_role_name(role: Role) -> &'static str {
  match role {
    Role::GenericContainer => "generic",
    Role::Alert => "alert",
    Role::AlertDialog => "alertdialog",
    Role::Application => "application",
    Role::Article => "article",
    Role::Banner => "banner",
    Role::Blockquote => "blockquote",
    Role::Button => "button",
    Role::Caption => "caption",
    Role::Cell => "cell",
    Role::CheckBox => "checkbox",
    Role::Code => "code",
    Role::ColumnHeader => "columnheader",
    Role::ComboBox => "combobox",
    Role::Complementary => "complementary",
    Role::ContentInfo => "contentinfo",
    Role::Definition => "definition",
    Role::Dialog => "dialog",
    Role::Document => "document",
    Role::Emphasis => "emphasis",
    Role::Feed => "feed",
    Role::Figure => "figure",
    Role::Form => "form",
    Role::Grid => "grid",
    Role::GridCell => "gridcell",
    Role::Group => "group",
    Role::Heading => "heading",
    Role::Image => "image",
    Role::Link => "link",
    Role::List => "list",
    Role::ListBox => "listbox",
    Role::ListItem => "listitem",
    Role::Log => "log",
    Role::Main => "main",
    Role::Marquee => "marquee",
    Role::Math => "math",
    Role::Menu => "menu",
    Role::MenuBar => "menubar",
    Role::MenuItem => "menuitem",
    Role::MenuItemCheckBox => "menuitemcheckbox",
    Role::MenuItemRadio => "menuitemradio",
    Role::Meter => "meter",
    Role::Navigation => "navigation",
    Role::Note => "note",
    Role::ListBoxOption => "option",
    Role::Paragraph => "paragraph",
    Role::ProgressIndicator => "progressbar",
    Role::RadioButton => "radio",
    Role::RadioGroup => "radiogroup",
    Role::Region => "region",
    Role::Row => "row",
    Role::RowGroup => "rowgroup",
    Role::RowHeader => "rowheader",
    Role::ScrollBar => "scrollbar",
    Role::Search => "search",
    Role::SearchInput => "searchbox",
    Role::Splitter => "separator",
    Role::Slider => "slider",
    Role::SpinButton => "spinbutton",
    Role::Status => "status",
    Role::Strong => "strong",
    Role::Switch => "switch",
    Role::Tab => "tab",
    Role::Table => "table",
    Role::TabList => "tablist",
    Role::TabPanel => "tabpanel",
    Role::Term => "term",
    Role::TextInput | Role::MultilineTextInput => "textbox",
    Role::Time => "time",
    Role::Timer => "timer",
    Role::Toolbar => "toolbar",
    Role::Tooltip => "tooltip",
    Role::Tree => "tree",
    Role::TreeGrid => "treegrid",
    Role::TreeItem => "treeitem",
    Role::Window => "window",
    Role::Unknown => "unknown",
    _ => "unknown",
  }
}

fn finite_or_zero(value: f64) -> f64 {
  if value.is_finite() { value } else { 0.0 }
}

fn sane_scale(scale: f64) -> f64 {
  if scale.is_finite() && scale > 0.0 {
    scale
  } else {
    1.0
  }
}

fn phys(css: f64, scale: f64) -> f64 {
  let value = finite_or_zero(css) * sane_scale(scale);
  if value.is_finite() { value } else { 0.0 }
}

fn bounds_of(forest: &Forest, id: u64, scale: f64) -> Rect {
  let mut seen = HashSet::new();
  let (x, y) = origin_of(forest, id, &mut seen);
  let (width, height) = forest
    .nodes
    .get(&id)
    .map(|author| {
      (
        finite_or_zero(author.offset_width.unwrap_or(0.0)),
        finite_or_zero(author.offset_height.unwrap_or(0.0)),
      )
    })
    .unwrap_or((0.0, 0.0));
  Rect::new(
    phys(x, scale),
    phys(y, scale),
    phys(x + width, scale),
    phys(y + height, scale),
  )
}

fn origin_of(forest: &Forest, id: u64, seen: &mut HashSet<u64>) -> (f64, f64) {
  if !seen.insert(id) {
    return (0.0, 0.0);
  }
  let Some(author) = forest.nodes.get(&id) else {
    return (0.0, 0.0);
  };
  let left = finite_or_zero(author.offset_left.unwrap_or(0.0));
  let top = finite_or_zero(author.offset_top.unwrap_or(0.0));
  let offset_parent = author.offset_parent;
  let parent = author.parent;
  let (base_x, base_y) = if let Some(ancestor) = offset_parent {
    if is_strict_ancestor(forest, ancestor, id) {
      origin_of(forest, ancestor, seen)
    } else if let Some(parent) = parent {
      origin_of(forest, parent, seen)
    } else {
      (0.0, 0.0)
    }
  } else if let Some(parent) = parent {
    origin_of(forest, parent, seen)
  } else {
    (0.0, 0.0)
  };
  (base_x + left, base_y + top)
}

#[cfg(test)]
mod tests {
  use accesskit::{Action, Node, NodeId, Rect, Role, TreeId, TreeUpdate};

  use super::{
    Fail, append_child, attach_root, connected_window, create, focus,
    full_tree, insert_before, is_closed, note_size, note_title, note_viewport,
    open_window, release_window, remove_child, replace_child,
    set_offset_parent, snapshot, update,
  };

  const WINDOW: u64 = 9_000_000_001;
  const WINDOW_B: u64 = 9_000_000_002;
  const WINDOW_C: u64 = 9_000_000_003;

  struct ReleaseGuard(u64);

  impl Drop for ReleaseGuard {
    fn drop(&mut self) {
      release_window(self.0);
    }
  }

  fn published(update: &TreeUpdate, id: u64) -> &Node {
    update
      .nodes
      .iter()
      .find(|(node_id, _)| node_id.0 == id)
      .map(|(_, node)| node)
      .unwrap_or_else(|| panic!("missing node {id}"))
  }

  #[test]
  fn publishes_a_full_tree() {
    let _window = ReleaseGuard(WINDOW);
    let _other = ReleaseGuard(WINDOW_B);
    note_viewport(WINDOW, "Palette".to_string(), 100.0, 50.0, 2.0);
    let before = full_tree(WINDOW);
    let chrome_id = before.tree.as_ref().unwrap().root;
    assert_eq!(before.tree_id, TreeId::ROOT);
    assert_eq!(
      before.tree.as_ref().unwrap().toolkit_name.as_deref(),
      Some("Yuzuko")
    );
    assert_eq!(before.tree.as_ref().unwrap().toolkit_version, None);
    assert_eq!(before.focus, chrome_id);
    assert_eq!(published(&before, chrome_id.0).label(), Some("Palette"));
    assert_eq!(
      published(&before, chrome_id.0).bounds(),
      Some(Rect::new(0.0, 0.0, 200.0, 100.0))
    );
    assert!(published(&before, chrome_id.0).children().is_empty());

    let root = create();
    let button = create();
    let slider = create();
    let mystery = create();
    let directory = create();
    let textbox = create();
    let row = create();
    let cell = create();
    attach_root(WINDOW, root).unwrap();
    append_child(root, button).unwrap();
    append_child(root, slider).unwrap();
    append_child(root, mystery).unwrap();
    append_child(root, directory).unwrap();
    append_child(root, textbox).unwrap();
    append_child(root, row).unwrap();
    append_child(root, cell).unwrap();
    update(button, |author| {
      author.role = Some("button".to_string());
      author.name = Some("Go".to_string());
      author.offset_left = Some(10.0);
      author.offset_top = Some(20.0);
      author.offset_width = Some(30.0);
      author.offset_height = Some(40.0);
    });
    update(slider, |author| {
      author.role = Some("slider".to_string());
    });
    update(mystery, |author| {
      author.role = Some("mystery".to_string());
      author.expanded = Some(false);
    });
    update(directory, |author| {
      author.role = Some("directory".to_string());
    });
    update(textbox, |author| {
      author.role = Some("textbox".to_string());
      author.multiline = Some(true);
      author.name = Some(String::new());
    });
    update(row, |author| author.row_index = Some(1));
    update(cell, |author| {
      author.col_span = Some(2);
      author.row_span = Some(3);
      author.col_count = Some(4);
      author.row_count = Some(5);
    });

    assert!(matches!(
      insert_before(button, root, Some(root)),
      Err(Fail::NotChild)
    ));
    assert!(matches!(append_child(root, root), Err(Fail::RootMove)));
    assert!(matches!(
      insert_before(root, root, None),
      Err(Fail::RootMove)
    ));
    assert!(matches!(
      replace_child(button, root, root),
      Err(Fail::NotChild)
    ));
    let own = create();
    assert!(matches!(append_child(own, own), Err(Fail::Ancestor)));

    let snapshot = full_tree(WINDOW);
    assert_eq!(snapshot.tree.as_ref().unwrap().root, chrome_id);
    assert_eq!(snapshot.focus, chrome_id);
    let chrome = published(&snapshot, chrome_id.0);
    assert_eq!(chrome.role(), Role::Window);
    assert_eq!(chrome.children(), &[NodeId(root)]);
    assert!(!chrome.supports_action(Action::Click));
    let button_node = published(&snapshot, button);
    assert_eq!(button_node.role(), Role::Button);
    assert_eq!(button_node.label(), Some("Go"));
    assert_eq!(
      button_node.bounds(),
      Some(Rect::new(20.0, 40.0, 80.0, 120.0))
    );
    assert!(button_node.supports_action(Action::Click));
    assert!(!button_node.supports_action(Action::Focus));
    assert!(!button_node.supports_action(Action::Increment));
    let slider_node = published(&snapshot, slider);
    assert!(slider_node.supports_action(Action::Increment));
    assert!(slider_node.supports_action(Action::Decrement));
    assert!(slider_node.supports_action(Action::SetValue));
    assert!(slider_node.supports_action(Action::ShowContextMenu));
    assert!(slider_node.supports_action(Action::ScrollIntoView));
    assert!(!slider_node.supports_action(Action::Click));
    assert!(!slider_node.supports_action(Action::Focus));
    let mystery_node = published(&snapshot, mystery);
    assert_eq!(mystery_node.role(), Role::Unknown);
    assert!(mystery_node.supports_action(Action::Expand));
    assert!(mystery_node.supports_action(Action::Collapse));
    assert_eq!(mystery_node.is_expanded(), Some(false));
    assert_eq!(published(&snapshot, directory).role(), Role::List);
    let text = published(&snapshot, textbox);
    assert_eq!(text.role(), Role::MultilineTextInput);
    assert_eq!(text.label(), Some(""));
    assert_eq!(published(&snapshot, row).row_index(), Some(0));
    let cell_node = published(&snapshot, cell);
    assert_eq!(cell_node.column_span(), Some(2));
    assert_eq!(cell_node.row_span(), Some(3));
    assert_eq!(cell_node.column_count(), Some(4));
    assert_eq!(cell_node.row_count(), Some(5));

    update(textbox, |author| author.multiline = Some(false));
    assert_eq!(
      published(&full_tree(WINDOW), textbox).role(),
      Role::TextInput
    );
    update(row, |author| author.row_index = Some(0));
    assert_eq!(published(&full_tree(WINDOW), row).row_index(), Some(0));
    update(row, |author| author.row_index = Some(-1));
    assert_eq!(published(&full_tree(WINDOW), row).row_index(), None);

    note_viewport(WINDOW_B, "Other".to_string(), 40.0, 20.0, 1.0);
    let root_b = create();
    let mid = create();
    let child = create();
    attach_root(WINDOW_B, root_b).unwrap();
    append_child(root_b, mid).unwrap();
    append_child(mid, child).unwrap();
    update(root_b, |author| {
      author.offset_left = Some(5.0);
      author.offset_top = Some(6.0);
    });
    update(mid, |author| {
      author.offset_left = Some(10.0);
      author.offset_top = Some(0.0);
    });
    update(child, |author| {
      author.offset_left = Some(3.0);
      author.offset_top = Some(4.0);
    });
    set_offset_parent(child, Some(root_b)).unwrap();
    let other = full_tree(WINDOW_B);
    assert_eq!(
      published(&other, root_b).bounds(),
      Some(Rect::new(5.0, 6.0, 5.0, 6.0))
    );
    assert_eq!(
      published(&other, mid).bounds(),
      Some(Rect::new(15.0, 6.0, 15.0, 6.0))
    );
    assert_eq!(
      published(&other, child).bounds(),
      Some(Rect::new(8.0, 10.0, 8.0, 10.0))
    );

    update(button, |author| author.focusable = true);
    assert_eq!(focus(button).unwrap(), WINDOW);
    let snapshot = full_tree(WINDOW);
    assert_eq!(snapshot.focus, NodeId(button));
    assert!(published(&snapshot, button).supports_action(Action::Focus));
    assert_eq!(snapshot.tree.as_ref().unwrap().root, chrome_id);

    note_size(WINDOW, 80.0, 40.0, 2.0);
    let snapshot = full_tree(WINDOW);
    assert_eq!(snapshot.tree.as_ref().unwrap().root, chrome_id);
    assert_eq!(snapshot.focus, NodeId(button));
    let chrome = published(&snapshot, chrome_id.0);
    assert_eq!(chrome.label(), Some("Palette"));
    assert_eq!(chrome.bounds(), Some(Rect::new(0.0, 0.0, 160.0, 80.0)));

    note_title(WINDOW, "Renamed".to_string());
    let snapshot = full_tree(WINDOW);
    assert_eq!(published(&snapshot, chrome_id.0).label(), Some("Renamed"));
    assert_eq!(published(&snapshot, button).label(), Some("Go"));
    assert_eq!(snapshot.focus, NodeId(button));

    remove_child(root, button).unwrap();
    let snapshot = full_tree(WINDOW);
    assert_eq!(snapshot.focus, chrome_id);
    assert!(snapshot.nodes.iter().all(|(id, _)| id.0 != button));

    release_window(WINDOW);
    assert_eq!(open_window(root), None);
    assert_eq!(connected_window(root), Some(WINDOW));
    assert!(is_closed(WINDOW));
    assert!(!is_closed(9_000_000_099));
    release_window(WINDOW);
    assert!(is_closed(WINDOW));
  }

  #[test]
  fn snapshots_the_computed_tree() {
    let _window = ReleaseGuard(WINDOW_C);
    note_viewport(WINDOW_C, "Palette".to_string(), 100.0, 50.0, 2.0);
    let before = snapshot(WINDOW_C);
    assert_eq!(before.len(), 1);
    let only = before.values().next().expect("chrome");
    assert_eq!(only.role, "window");
    assert_eq!(only.name.as_deref(), Some("Palette"));
    assert!(only.children.is_empty());
    assert_eq!(only.parent, None);
    assert_eq!(only.offset_width, 100.0);
    let root = create();
    let directory = create();
    let textbox = create();
    let row = create();
    attach_root(WINDOW_C, root).unwrap();
    append_child(root, directory).unwrap();
    append_child(root, textbox).unwrap();
    append_child(root, row).unwrap();
    update(root, |author| {
      author.role = Some("hammer".to_string());
      author.name = Some(String::new());
    });
    update(directory, |author| {
      author.role = Some("directory".to_string());
    });
    update(textbox, |author| {
      author.role = Some("textbox".to_string());
      author.multiline = Some(true);
    });
    update(row, |author| {
      author.role = Some("row".to_string());
      author.row_index = Some(1);
      author.row_count = Some(-3);
      author.offset_left = Some(f64::NAN);
      author.offset_width = None;
    });
    let nodes = snapshot(WINDOW_C);
    let chrome_id = nodes
      .iter()
      .find(|(_, node)| node.parent.is_none())
      .map(|(id, _)| *id)
      .expect("chrome");
    assert_eq!(nodes[&chrome_id].role, "window");
    assert_eq!(nodes[&chrome_id].name.as_deref(), Some("Palette"));
    assert_eq!(nodes[&chrome_id].offset_width, 100.0);
    assert_eq!(nodes[&chrome_id].offset_height, 50.0);
    assert_eq!(nodes[&chrome_id].children, vec![root]);
    assert_eq!(nodes[&root].parent, Some(chrome_id));
    assert_eq!(nodes[&root].role, "unknown");
    assert_eq!(nodes[&root].name.as_deref(), Some(""));
    assert_eq!(nodes[&directory].role, "list");
    assert_eq!(nodes[&textbox].role, "textbox");
    assert_eq!(nodes[&textbox].multiline, Some(true));
    assert_eq!(nodes[&row].row_index, Some(0));
    assert_eq!(nodes[&row].row_count, None);
    assert_eq!(nodes[&row].offset_left, 0.0);
    assert_eq!(nodes[&row].offset_width, 0.0);
    let loose = create();
    assert!(!snapshot(WINDOW_C).contains_key(&loose));
  }
}
