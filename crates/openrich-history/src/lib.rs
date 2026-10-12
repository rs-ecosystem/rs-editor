//! Undo history for OpenRich, a port of
//! [prosemirror-history](https://github.com/ProseMirror/prosemirror-history)
//! (MIT, see `LICENSE-ProseMirror`).
//!
//! The history stores *inverted steps*, not document snapshots. Changes
//! that are kept out of the history (remote collaboration edits, for
//! instance) only add position maps, so undo still applies to the
//! current document. Consecutive changes are grouped into one undo event
//! when they are adjacent and less than `new_group_delay` apart, or when
//! they belong to the same IME composition.

use std::any::Any;
use std::sync::Arc;

use openrich_state::{
    Dispatch, EditorState, Plugin, PluginValue, SelectionBookmark, Transaction, ViewContext, meta,
};
use openrich_transform::{Assoc, Mappable, Mapping, Step, StepMap, Transform};

/// Plugin key of the history plugin.
pub const HISTORY_KEY: &str = "history$";
/// Meta key (`bool`) that forces the next change into a new undo event.
pub const CLOSE_HISTORY: &str = "closeHistory$";
/// Meta key (`usize`) set by collaboration when local steps were rebased.
pub const REBASED: &str = "rebased";

const MAX_EMPTY_ITEMS: usize = 500;

/// Applies an inverted step. When changes outside the history made it
/// invalid (for example a mark that a code block does not allow), the
/// replaced content is fitted instead, so undo restores as much as it can
/// rather than silently dropping the event.
fn apply_or_fit(tr: &mut Transaction, step: Step) -> bool {
    if tr.maybe_step(step.clone()) {
        return true;
    }
    match step {
        Step::Replace(replace) => {
            let before = tr.steps.len();
            tr.replace(replace.from, replace.to, replace.slice).is_ok() && tr.steps.len() > before
        }
        _ => false,
    }
}
const DEPTH_OVERFLOW: usize = 20;

#[derive(Debug, Clone)]
struct Item {
    /// Forward step map.
    map: StepMap,
    /// The inverted step, when this item is undoable.
    step: Option<Step>,
    /// Set on the first item of an event: the selection before it.
    selection: Option<SelectionBookmark>,
    /// Offset back to the item this one mirrors.
    mirror_offset: Option<usize>,
}

impl Item {
    fn map_only(map: StepMap) -> Self {
        Self {
            map,
            step: None,
            selection: None,
            mirror_offset: None,
        }
    }

    fn merge(&self, other: &Item) -> Option<Item> {
        let (Some(step), Some(other_step)) = (&self.step, &other.step) else {
            return None;
        };
        if other.selection.is_some() {
            return None;
        }
        let merged = other_step.merge(step)?;
        Some(Item {
            map: merged.get_map().invert(),
            step: Some(merged),
            selection: self.selection,
            mirror_offset: None,
        })
    }
}

/// One side of the history (undo or redo).
#[derive(Debug, Clone, Default)]
struct Branch {
    items: Arc<Vec<Item>>,
    event_count: usize,
}

struct Popped {
    remaining: Branch,
    transaction: Transaction,
    selection: SelectionBookmark,
}

impl Branch {
    fn new(items: Vec<Item>, event_count: usize) -> Self {
        Self {
            items: Arc::new(items),
            event_count,
        }
    }

    /// Pops the latest event and applies it to a new transaction.
    fn pop_event(&self, state: &EditorState, preserve_items: bool) -> Option<Popped> {
        if self.event_count == 0 {
            return None;
        }
        let items = &self.items;
        let mut end = items.len();
        loop {
            end -= 1;
            if items[end].selection.is_some() {
                break;
            }
        }
        let mut remap: Option<Mapping> = None;
        let mut map_from = 0usize;
        if preserve_items {
            let mapping = self.remapping(end, items.len());
            map_from = mapping.maps().len();
            remap = Some(mapping);
        }
        let mut tr = state.tr();
        let mut add_after: Vec<Item> = Vec::new();
        let mut add_before: Vec<Item> = Vec::new();
        for i in (0..items.len()).rev() {
            let item = &items[i];
            let Some(step) = &item.step else {
                if remap.is_none() {
                    let mapping = self.remapping(end, i + 1);
                    map_from = mapping.maps().len();
                    remap = Some(mapping);
                }
                map_from -= 1;
                add_before.push(item.clone());
                continue;
            };
            if let Some(mapping) = &mut remap {
                add_before.push(Item::map_only(item.map.clone()));
                let mapped = step.map(&mapping.slice_from(map_from));
                let mut applied_map = None;
                if let Some(mapped) = mapped
                    && apply_or_fit(&mut tr, mapped)
                {
                    let map = tr.mapping.maps()[tr.mapping.maps().len() - 1].clone();
                    add_after.push(Item {
                        map: map.clone(),
                        step: None,
                        selection: None,
                        mirror_offset: Some(add_after.len() + add_before.len()),
                    });
                    applied_map = Some(map);
                }
                map_from -= 1;
                if let Some(map) = applied_map {
                    mapping.append_map(map, Some(map_from));
                }
            } else {
                apply_or_fit(&mut tr, step.clone());
            }
            if let Some(selection) = item.selection {
                let selection = match &remap {
                    Some(mapping) => selection.map(&mapping.slice_from(map_from)),
                    None => selection,
                };
                add_before.reverse();
                let mut kept: Vec<Item> = items[..end].to_vec();
                kept.extend(add_before);
                kept.extend(add_after);
                return Some(Popped {
                    remaining: Branch::new(kept, self.event_count - 1),
                    transaction: tr,
                    selection,
                });
            }
        }
        None
    }

    /// A branch with the inverted steps of `transform` added.
    fn add_transform(
        &self,
        transform: &Transform,
        selection: Option<SelectionBookmark>,
        options: &HistoryOptions,
        preserve_items: bool,
    ) -> Branch {
        let mut new_items: Vec<Item> = Vec::new();
        let mut event_count = self.event_count;
        let mut old_items: Vec<Item> = self.items.as_ref().clone();
        let mut last_item: Option<Item> = if preserve_items {
            None
        } else {
            old_items.last().cloned()
        };
        let mut selection = selection;
        for i in 0..transform.steps.len() {
            let Ok(step) = transform.steps[i].invert(&transform.docs[i]) else {
                continue;
            };
            let mut item = Item {
                map: transform.mapping.maps()[i].clone(),
                step: Some(step),
                selection,
                mirror_offset: None,
            };
            if let Some(merged) = last_item.as_ref().and_then(|last| last.merge(&item)) {
                item = merged;
                if i > 0 {
                    new_items.pop();
                } else {
                    old_items.pop();
                }
            }
            new_items.push(item.clone());
            if selection.is_some() {
                event_count += 1;
                selection = None;
            }
            if !preserve_items {
                last_item = Some(item);
            }
        }
        let overflow = event_count.saturating_sub(options.depth);
        if overflow > DEPTH_OVERFLOW {
            old_items = cut_off_events(&old_items, overflow);
            event_count -= overflow;
        }
        old_items.extend(new_items);
        Branch::new(old_items, event_count)
    }

    fn remapping(&self, from: usize, to: usize) -> Mapping {
        let mut maps = Mapping::new();
        for i in from..to {
            let item = &self.items[i];
            let mirror = item
                .mirror_offset
                .filter(|offset| i >= *offset && i - offset >= from)
                .map(|offset| maps.maps().len() - offset);
            maps.append_map(item.map.clone(), mirror);
        }
        maps
    }

    fn add_maps(&self, maps: &[StepMap]) -> Branch {
        if self.event_count == 0 {
            return self.clone();
        }
        let mut items = self.items.as_ref().clone();
        items.extend(maps.iter().cloned().map(Item::map_only));
        Branch::new(items, self.event_count)
    }

    /// Adjusts for local steps that collaboration rebased over remote ones.
    fn rebased(&self, rebased: &Transform, rebased_count: usize) -> Branch {
        if self.event_count == 0 {
            return self.clone();
        }
        let mut rebased_items = Vec::new();
        let start = self.items.len().saturating_sub(rebased_count);
        let mapping = &rebased.mapping;
        let mut new_until = rebased.steps.len();
        let mut event_count = self.event_count;
        event_count -= self.items[start..]
            .iter()
            .filter(|item| item.selection.is_some())
            .count();
        let mut i_rebased = rebased_count;
        for item in &self.items[start..] {
            i_rebased = i_rebased.wrapping_sub(1);
            let Some(pos) = mapping.get_mirror(i_rebased) else {
                continue;
            };
            new_until = new_until.min(pos);
            let map = mapping.maps()[pos].clone();
            if item.step.is_some() {
                let Ok(step) = rebased.steps[pos].invert(&rebased.docs[pos]) else {
                    continue;
                };
                let selection = item
                    .selection
                    .map(|s| s.map(&mapping.slice(i_rebased + 1, pos)));
                if selection.is_some() {
                    event_count += 1;
                }
                rebased_items.push(Item {
                    map,
                    step: Some(step),
                    selection,
                    mirror_offset: None,
                });
            } else {
                rebased_items.push(Item::map_only(map));
            }
        }
        let mut items: Vec<Item> = self.items[..start].to_vec();
        items.extend((rebased_count..new_until).map(|i| Item::map_only(mapping.maps()[i].clone())));
        let rebased_len = rebased_items.len();
        items.extend(rebased_items);
        let branch = Branch::new(items, event_count);
        if branch.empty_item_count() > MAX_EMPTY_ITEMS {
            branch.compress(self.items.len() - rebased_len)
        } else {
            branch
        }
    }

    fn empty_item_count(&self) -> usize {
        self.items.iter().filter(|item| item.step.is_none()).count()
    }

    /// Rewrites items below `upto`, folding map-only items into the steps.
    fn compress(&self, upto: usize) -> Branch {
        let mut remap = self.remapping(0, upto);
        let mut map_from = remap.maps().len();
        let mut items: Vec<Item> = Vec::new();
        let mut events = 0;
        for i in (0..self.items.len()).rev() {
            let item = &self.items[i];
            if i >= upto {
                items.push(item.clone());
                if item.selection.is_some() {
                    events += 1;
                }
            } else if let Some(step) = &item.step {
                let mapped = step.map(&remap.slice_from(map_from));
                let map = mapped.as_ref().map(Step::get_map);
                map_from -= 1;
                if let Some(map) = &map {
                    remap.append_map(map.clone(), Some(map_from));
                }
                if let (Some(mapped), Some(map)) = (mapped, map) {
                    let selection = item.selection.map(|s| s.map(&remap.slice_from(map_from)));
                    if selection.is_some() {
                        events += 1;
                    }
                    let new_item = Item {
                        map: map.invert(),
                        step: Some(mapped),
                        selection,
                        mirror_offset: None,
                    };
                    match items.last().and_then(|last| last.merge(&new_item)) {
                        Some(merged) => {
                            let last = items.len() - 1;
                            items[last] = merged;
                        }
                        None => items.push(new_item),
                    }
                }
            } else {
                map_from -= 1;
            }
        }
        items.reverse();
        Branch::new(items, events)
    }
}

fn cut_off_events(items: &[Item], n: usize) -> Vec<Item> {
    let mut n = n;
    for (index, item) in items.iter().enumerate() {
        if item.selection.is_some() {
            if n == 0 {
                return items[index..].to_vec();
            }
            n -= 1;
        }
    }
    items.to_vec()
}

/// Options of the history plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryOptions {
    /// Undo events to keep.
    pub depth: usize,
    /// Milliseconds after which a change starts a new undo event.
    pub new_group_delay: u64,
}

impl Default for HistoryOptions {
    fn default() -> Self {
        Self {
            depth: 100,
            new_group_delay: 500,
        }
    }
}

/// The history plugin's state.
#[derive(Debug, Clone)]
pub struct HistoryState {
    done: Branch,
    undone: Branch,
    prev_ranges: Option<Vec<usize>>,
    prev_time: u64,
    /// Id of the last composition, or -1.
    prev_composition: i64,
}

/// Meta stored on undo/redo transactions.
#[derive(Debug, Clone)]
struct HistoryMeta {
    redo: bool,
    state: Arc<HistoryState>,
}

fn must_preserve_items(state: &EditorState) -> bool {
    state
        .plugins()
        .iter()
        .any(|plugin| plugin.history_preserve_items())
}

fn ranges_for(maps: &[StepMap]) -> Vec<usize> {
    let mut result = Vec::new();
    for map in maps.iter().rev() {
        map.for_each(|_, _, from, to| {
            result.push(from);
            result.push(to);
        });
        if !result.is_empty() {
            break;
        }
    }
    result
}

fn map_ranges(ranges: &Option<Vec<usize>>, mapping: &Mapping) -> Option<Vec<usize>> {
    let ranges = ranges.as_ref()?;
    let mut result = Vec::new();
    for pair in ranges.chunks(2) {
        let from = mapping.map(pair[0], Assoc::After);
        let to = mapping.map(pair[1], Assoc::Before);
        if from <= to {
            result.push(from);
            result.push(to);
        }
    }
    Some(result)
}

fn is_adjacent_to(transform: &Transform, prev_ranges: &Option<Vec<usize>>) -> bool {
    let Some(prev_ranges) = prev_ranges else {
        return false;
    };
    if !transform.doc_changed() {
        return true;
    }
    let mut adjacent = false;
    transform.mapping.maps()[0].for_each(|start, end, _, _| {
        for pair in prev_ranges.chunks(2) {
            if start <= pair[1] && end >= pair[0] {
                adjacent = true;
            }
        }
    });
    adjacent
}

fn apply_transaction(
    history: &HistoryState,
    state: &EditorState,
    tr: &Transaction,
    options: &HistoryOptions,
) -> HistoryState {
    if let Some(meta) = tr.get_meta::<HistoryMeta>(HISTORY_KEY) {
        return meta.state.as_ref().clone();
    }
    let mut history = history.clone();
    if tr.get_meta::<bool>(CLOSE_HISTORY) == Some(&true) {
        history.prev_ranges = None;
        history.prev_time = 0;
        history.prev_composition = -1;
    }
    let appended = tr.appended_to();
    let preserve = must_preserve_items(state);
    if tr.steps.is_empty() {
        return history;
    }
    if let Some(root_meta) = appended.and_then(|root| root.get_meta::<HistoryMeta>(HISTORY_KEY)) {
        return if root_meta.redo {
            HistoryState {
                done: history.done.add_transform(tr, None, options, preserve),
                prev_ranges: Some(ranges_for(tr.mapping.maps())),
                ..history
            }
        } else {
            HistoryState {
                undone: history.undone.add_transform(tr, None, options, preserve),
                prev_ranges: None,
                ..history
            }
        };
    }
    let keep_out = tr.get_meta::<bool>(meta::ADD_TO_HISTORY) == Some(&false)
        || appended.is_some_and(|root| root.get_meta::<bool>(meta::ADD_TO_HISTORY) == Some(&false));
    if !keep_out {
        let composition = tr.get_meta::<u64>(meta::COMPOSITION).copied();
        let composition_changed =
            composition.is_none_or(|id| history.prev_composition != id as i64);
        let new_group = history.prev_time == 0
            || (appended.is_none()
                && composition_changed
                && (history.prev_time + options.new_group_delay < tr.time
                    || !is_adjacent_to(tr, &history.prev_ranges)));
        let prev_ranges = if appended.is_some() {
            map_ranges(&history.prev_ranges, &tr.mapping)
        } else {
            Some(ranges_for(tr.mapping.maps()))
        };
        let bookmark = new_group.then(|| state.selection().bookmark());
        return HistoryState {
            done: history.done.add_transform(tr, bookmark, options, preserve),
            undone: Branch::default(),
            prev_ranges,
            prev_time: tr.time,
            prev_composition: composition.map_or(history.prev_composition, |id| id as i64),
        };
    }
    if let Some(&rebased) = tr.get_meta::<usize>(REBASED) {
        return HistoryState {
            done: history.done.rebased(tr, rebased),
            undone: history.undone.rebased(tr, rebased),
            prev_ranges: map_ranges(&history.prev_ranges, &tr.mapping),
            ..history
        };
    }
    HistoryState {
        done: history.done.add_maps(tr.mapping.maps()),
        undone: history.undone.add_maps(tr.mapping.maps()),
        prev_ranges: map_ranges(&history.prev_ranges, &tr.mapping),
        ..history
    }
}

/// The undo history plugin.
#[derive(Debug, Clone, Default)]
pub struct History {
    pub options: HistoryOptions,
}

impl History {
    pub fn new(options: HistoryOptions) -> Arc<Self> {
        Arc::new(Self { options })
    }
}

impl Plugin for History {
    fn key(&self) -> &str {
        HISTORY_KEY
    }

    fn init(&self, _state: &EditorState) -> Option<PluginValue> {
        Some(Arc::new(HistoryState {
            done: Branch::default(),
            undone: Branch::default(),
            prev_ranges: None,
            prev_time: 0,
            prev_composition: -1,
        }))
    }

    fn apply(
        &self,
        tr: &Transaction,
        value: &PluginValue,
        old: &EditorState,
        _new: &EditorState,
    ) -> PluginValue {
        match value.downcast_ref::<HistoryState>() {
            Some(history) => Arc::new(apply_transaction(history, old, tr, &self.options)),
            None => value.clone(),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn history_of(state: &EditorState) -> Option<(Arc<HistoryState>, HistoryOptions)> {
    let history = state.plugin_state::<HistoryState>(HISTORY_KEY)?;
    let options = state
        .plugin(HISTORY_KEY)
        .and_then(|plugin| plugin.as_any().downcast_ref::<History>())
        .map(|plugin| plugin.options)
        .unwrap_or_default();
    Some((history, options))
}

fn hist_transaction(
    history: &HistoryState,
    options: &HistoryOptions,
    state: &EditorState,
    redo: bool,
) -> Option<Transaction> {
    let preserve = must_preserve_items(state);
    let source = if redo { &history.undone } else { &history.done };
    let popped = source.pop_event(state, preserve)?;
    let selection = popped.selection.resolve(&popped.transaction.doc);
    let target = if redo { &history.done } else { &history.undone };
    let added = target.add_transform(
        &popped.transaction,
        Some(state.selection().bookmark()),
        options,
        preserve,
    );
    let new_history = HistoryState {
        done: if redo {
            added.clone()
        } else {
            popped.remaining.clone()
        },
        undone: if redo { popped.remaining } else { added },
        prev_ranges: None,
        prev_time: 0,
        prev_composition: -1,
    };
    let mut tr = popped.transaction;
    tr.set_selection(selection);
    tr.set_meta(
        HISTORY_KEY,
        HistoryMeta {
            redo,
            state: Arc::new(new_history),
        },
    );
    Some(tr)
}

fn history_command(state: &EditorState, dispatch: Dispatch, redo: bool, scroll: bool) -> bool {
    let Some((history, options)) = history_of(state) else {
        return false;
    };
    let count = if redo {
        history.undone.event_count
    } else {
        history.done.event_count
    };
    if count == 0 {
        return false;
    }
    if let Some(dispatch) = dispatch
        && let Some(mut tr) = hist_transaction(&history, &options, state, redo)
    {
        if scroll {
            tr.scroll_into_view();
        }
        dispatch(tr);
    }
    true
}

/// Undoes the last change event.
pub fn undo(state: &EditorState, dispatch: Dispatch, _view: Option<&dyn ViewContext>) -> bool {
    history_command(state, dispatch, false, true)
}

/// Redoes the last undone event.
pub fn redo(state: &EditorState, dispatch: Dispatch, _view: Option<&dyn ViewContext>) -> bool {
    history_command(state, dispatch, true, true)
}

pub fn undo_no_scroll(
    state: &EditorState,
    dispatch: Dispatch,
    _view: Option<&dyn ViewContext>,
) -> bool {
    history_command(state, dispatch, false, false)
}

pub fn redo_no_scroll(
    state: &EditorState,
    dispatch: Dispatch,
    _view: Option<&dyn ViewContext>,
) -> bool {
    history_command(state, dispatch, true, false)
}

/// Number of undoable events.
pub fn undo_depth(state: &EditorState) -> usize {
    history_of(state).map_or(0, |(history, _)| history.done.event_count)
}

/// Number of redoable events.
pub fn redo_depth(state: &EditorState) -> usize {
    history_of(state).map_or(0, |(history, _)| history.undone.event_count)
}

/// Makes the next change start a new undo event.
pub fn close_history(tr: &mut Transaction) -> &mut Transaction {
    tr.set_meta(CLOSE_HISTORY, true)
}

pub fn is_history_transaction(tr: &Transaction) -> bool {
    tr.has_meta(HISTORY_KEY)
}

#[cfg(test)]
mod tests;
