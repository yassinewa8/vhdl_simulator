//! The list of rows in the wave view: signals, divider lines and groups.
//!
//! A group is like a folder: it holds signals and can be collapsed to one
//! header row. Groups are one level deep, which keeps moving rows around
//! simple. Dividers sit between rows, outside groups.
//!
//! No egui here, so every list operation can be tested.

use wave::SignalId;

use crate::wave_state::DisplayedSignal;

/// One entry at the top level of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Signal(DisplayedSignal),
    /// A horizontal line with an optional label, for separating rows.
    Divider(String),
    Group(Group),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub collapsed: bool,
    pub signals: Vec<DisplayedSignal>,
}

/// A row on screen: a top-level item, or a signal inside a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowRef {
    /// Index into the top-level items.
    pub item: usize,
    /// Index of the signal inside the group, for signals in a group.
    pub child: Option<usize>,
}

impl RowRef {
    pub fn top(item: usize) -> RowRef {
        RowRef { item, child: None }
    }
}

/// Where a row being dragged will land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drop {
    /// Just before this row (`None`: at the very end).
    Before(Option<RowRef>),
    /// Into the group whose header is at this top-level index.
    IntoGroup(usize),
}

/// What a row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind<'a> {
    Signal { signal: &'a DisplayedSignal, in_group: bool },
    Divider(&'a str),
    GroupHeader { name: &'a str, collapsed: bool, count: usize },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignalList {
    items: Vec<Item>,
    /// For naming new groups Group 1, Group 2, ...
    groups_made: u32,
}

impl SignalList {
    /// A list made of `items`, e.g. from a saved layout.
    pub fn from_items(items: Vec<Item>) -> SignalList {
        let groups = items.iter().filter(|item| matches!(item, Item::Group(_))).count();
        SignalList {
            items,
            // So new groups don't repeat the names of restored ones.
            groups_made: groups as u32,
        }
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn push_signal(&mut self, signal: DisplayedSignal) {
        self.items.push(Item::Signal(signal));
    }

    /// Every signal in the list, including those in collapsed groups.
    pub fn signals(&self) -> impl Iterator<Item = &DisplayedSignal> {
        self.items.iter().flat_map(|item| match item {
            Item::Signal(signal) => std::slice::from_ref(signal).iter(),
            Item::Group(group) => group.signals.iter(),
            Item::Divider(_) => [].iter(),
        })
    }

    pub fn contains(&self, id: SignalId) -> bool {
        self.signals().any(|signal| signal.id == id)
    }

    /// The rows on screen, top to bottom. A collapsed group shows only
    /// its header.
    pub fn rows(&self) -> Vec<RowRef> {
        let mut rows = Vec::new();
        for (index, item) in self.items.iter().enumerate() {
            rows.push(RowRef::top(index));
            if let Item::Group(group) = item
                && !group.collapsed
            {
                rows.extend((0..group.signals.len()).map(|child| RowRef { item: index, child: Some(child) }));
            }
        }
        rows
    }

    pub fn kind(&self, row: RowRef) -> Option<RowKind<'_>> {
        Some(match (self.items.get(row.item)?, row.child) {
            (Item::Signal(signal), None) => RowKind::Signal { signal, in_group: false },
            (Item::Divider(label), None) => RowKind::Divider(label),
            (Item::Group(group), None) => RowKind::GroupHeader {
                name: &group.name,
                collapsed: group.collapsed,
                count: group.signals.len(),
            },
            (Item::Group(group), Some(child)) => RowKind::Signal {
                signal: group.signals.get(child)?,
                in_group: true,
            },
            (_, Some(_)) => return None,
        })
    }

    /// The signal shown in `row`, if it is a signal row.
    pub fn signal(&self, row: RowRef) -> Option<&DisplayedSignal> {
        match self.kind(row)? {
            RowKind::Signal { signal, .. } => Some(signal),
            _ => None,
        }
    }

    pub fn signal_mut(&mut self, row: RowRef) -> Option<&mut DisplayedSignal> {
        match (self.items.get_mut(row.item)?, row.child) {
            (Item::Signal(signal), None) => Some(signal),
            (Item::Group(group), Some(child)) => group.signals.get_mut(child),
            _ => None,
        }
    }

    /// The label of a divider, or the name of a group, for renaming.
    pub fn title_mut(&mut self, row: RowRef) -> Option<&mut String> {
        match (self.items.get_mut(row.item)?, row.child) {
            (Item::Divider(label), None) => Some(label),
            (Item::Group(group), None) => Some(&mut group.name),
            _ => None,
        }
    }

    /// Removes a row. Removing a group header removes the group and the
    /// signals in it (use [`SignalList::ungroup`] to keep them).
    pub fn remove(&mut self, row: RowRef) {
        match row.child {
            None if row.item < self.items.len() => {
                self.items.remove(row.item);
            }
            Some(child) => {
                if let Some(Item::Group(group)) = self.items.get_mut(row.item)
                    && child < group.signals.len()
                {
                    group.signals.remove(child);
                }
            }
            None => {}
        }
    }

    /// Adds an empty divider after `row` (at the end if `None`). Returns
    /// its row.
    pub fn add_divider_after(&mut self, row: Option<RowRef>) -> RowRef {
        let at = row.map_or(self.items.len(), |row| (row.item + 1).min(self.items.len()));
        self.items.insert(at, Item::Divider(String::new()));
        RowRef::top(at)
    }

    /// Adds an empty, expanded group before the top-level item holding
    /// `row` (at the end if `None`). Returns its header row.
    pub fn add_group_before(&mut self, row: Option<RowRef>) -> RowRef {
        self.groups_made += 1;
        let at = row.map_or(self.items.len(), |row| row.item.min(self.items.len()));
        self.items.insert(
            at,
            Item::Group(Group {
                name: format!("Group {}", self.groups_made),
                collapsed: false,
                signals: Vec::new(),
            }),
        );
        RowRef::top(at)
    }

    /// Replaces the group at `item` by its signals, in place.
    pub fn ungroup(&mut self, item: usize) {
        if let Some(Item::Group(_)) = self.items.get(item)
            && let Item::Group(group) = self.items.remove(item)
        {
            for (offset, signal) in group.signals.into_iter().enumerate() {
                self.items.insert(item + offset, Item::Signal(signal));
            }
        }
    }

    pub fn toggle_collapsed(&mut self, item: usize) {
        if let Some(Item::Group(group)) = self.items.get_mut(item) {
            group.collapsed = !group.collapsed;
        }
    }

    /// Moves the row `from` so it sits just before the row `before`, or
    /// at the very end if `before` is `None`. A signal dropped before a
    /// signal inside a group joins that group; a group or divider always
    /// stays at the top level (before the group, if aimed inside one).
    /// Returns where the row ended up.
    pub fn move_row(&mut self, from: RowRef, before: Option<RowRef>) -> Option<RowRef> {
        if Some(from) == before {
            return Some(from);
        }
        let moving_whole_item = from.child.is_none();
        let taken = self.take(from)?;
        // Taking a top-level item shifts the later ones up by one.
        let shift = |item: usize| if moving_whole_item && from.item < item { item - 1 } else { item };

        match (taken, before) {
            (Taken::Signal(signal), Some(RowRef { item, child: Some(child) })) => {
                let item = shift(item);
                // Taking a signal out of the same group shifts its later signals.
                let child = if from.child.is_some_and(|c| from.item == item && c < child) { child - 1 } else { child };
                if let Some(Item::Group(group)) = self.items.get_mut(item) {
                    let child = child.min(group.signals.len());
                    group.signals.insert(child, signal);
                    return Some(RowRef { item, child: Some(child) });
                }
                None
            }
            (taken, Some(RowRef { item, .. })) => {
                let item = shift(item).min(self.items.len());
                self.items.insert(item, taken.into_item());
                Some(RowRef::top(item))
            }
            (taken, None) => {
                self.items.push(taken.into_item());
                Some(RowRef::top(self.items.len() - 1))
            }
        }
    }

    /// Where `moving` lands if dropped with the pointer at `position`,
    /// measured in rows from the top of the list (2.5 is half-way down the
    /// third row). Over the middle of a group's header, a signal goes
    /// into the group; otherwise the row goes in the nearest gap between
    /// rows.
    pub fn drop_target(&self, moving: RowRef, position: f32) -> Drop {
        let rows = self.rows();
        let under = position.floor();
        if under >= 0.0
            && let Some(&row) = rows.get(under as usize)
            && matches!(self.kind(row), Some(RowKind::GroupHeader { .. }))
            && self.signal(moving).is_some()
            && (0.25..0.75).contains(&(position - under))
        {
            return Drop::IntoGroup(row.item);
        }
        let gap = position.round().max(0.0) as usize;
        Drop::Before(rows.get(gap).copied())
    }

    /// Moves `moving` to where `drop` says. Returns its new row.
    pub fn apply_drop(&mut self, moving: RowRef, drop: Drop) -> Option<RowRef> {
        match drop {
            Drop::Before(before) => self.move_row(moving, before),
            Drop::IntoGroup(group) => self.move_into_group(moving, group),
        }
    }

    /// Moves the signal in row `from` to the end of the group at
    /// `group_item`. Returns its new row.
    pub fn move_into_group(&mut self, from: RowRef, group_item: usize) -> Option<RowRef> {
        if !matches!(self.items.get(group_item), Some(Item::Group(_))) || self.signal(from).is_none() {
            return None;
        }
        let Taken::Signal(signal) = self.take(from)? else { return None };
        let group_item = if from.child.is_none() && from.item < group_item { group_item - 1 } else { group_item };
        let Some(Item::Group(group)) = self.items.get_mut(group_item) else { return None };
        group.signals.push(signal);
        group.collapsed = false;
        Some(RowRef { item: group_item, child: Some(group.signals.len() - 1) })
    }

    /// Where the signal `id` is in the list (also inside collapsed groups).
    pub fn row_of(&self, id: SignalId) -> Option<RowRef> {
        self.items.iter().enumerate().find_map(|(index, item)| match item {
            Item::Signal(signal) if signal.id == id => Some(RowRef::top(index)),
            Item::Group(group) => group
                .signals
                .iter()
                .position(|signal| signal.id == id)
                .map(|child| RowRef { item: index, child: Some(child) }),
            _ => None,
        })
    }

    /// The signals in `rows`, in on-screen order, by id. Signals are
    /// tracked by id while moving several, because every removal shifts
    /// the positions of the rows after it.
    fn signal_ids_in_order(&self, rows: &[RowRef]) -> Vec<SignalId> {
        let mut rows = rows.to_vec();
        rows.sort_by_key(|row| (row.item, row.child.map_or(0, |c| c + 1)));
        rows.dedup();
        rows.iter().filter_map(|&row| self.signal(row)).map(|signal| signal.id).collect()
    }

    /// Takes the signal `id` out of the list.
    fn take_signal(&mut self, id: SignalId) -> Option<DisplayedSignal> {
        match self.take(self.row_of(id)?)? {
            Taken::Signal(signal) => Some(signal),
            Taken::Item(_) => None,
        }
    }

    /// Puts `signal` just before `before` (inside its group, if `before`
    /// is in one), or at the end. Returns its row.
    fn insert_signal(&mut self, signal: DisplayedSignal, before: Option<RowRef>) -> RowRef {
        match before {
            Some(RowRef { item, child: Some(child) }) => {
                if let Some(Item::Group(group)) = self.items.get_mut(item) {
                    let child = child.min(group.signals.len());
                    group.signals.insert(child, signal);
                    return RowRef { item, child: Some(child) };
                }
                self.items.push(Item::Signal(signal));
                RowRef::top(self.items.len() - 1)
            }
            Some(RowRef { item, child: None }) => {
                let item = item.min(self.items.len());
                self.items.insert(item, Item::Signal(signal));
                RowRef::top(item)
            }
            None => {
                self.items.push(Item::Signal(signal));
                RowRef::top(self.items.len() - 1)
            }
        }
    }

    /// Puts the signals among `rows` into a new group, placed where the
    /// first of them was (before its group, if it was in one). Other kinds
    /// of rows are left alone. Returns the new group's header row.
    pub fn group_rows(&mut self, rows: &[RowRef]) -> Option<RowRef> {
        let ids = self.signal_ids_in_order(rows);
        let first = self.row_of(*ids.first()?)?;
        // Moved top-level signals before that spot shift it up.
        let moved_before = ids
            .iter()
            .filter_map(|&id| self.row_of(id))
            .filter(|row| row.child.is_none() && row.item < first.item)
            .count();
        let signals: Vec<DisplayedSignal> = ids.iter().filter_map(|&id| self.take_signal(id)).collect();
        self.groups_made += 1;
        let at = (first.item - moved_before).min(self.items.len());
        self.items.insert(
            at,
            Item::Group(Group {
                name: format!("Group {}", self.groups_made),
                collapsed: false,
                signals,
            }),
        );
        Some(RowRef::top(at))
    }

    /// Removes several rows. Removing a group header removes its signals.
    pub fn remove_rows(&mut self, rows: &[RowRef]) {
        let mut rows = rows.to_vec();
        rows.sort_by_key(|row| (row.item, row.child.map_or(0, |c| c + 1)));
        rows.dedup();
        // From the bottom up, so earlier positions stay valid; a group's
        // signals go before its header.
        for row in rows.into_iter().rev() {
            self.remove(row);
        }
    }

    /// Moves the signals among `rows` to `drop`, keeping their order (other
    /// kinds of rows are left alone). Returns their new rows.
    pub fn move_rows(&mut self, rows: &[RowRef], drop: Drop) -> Vec<RowRef> {
        let ids = self.signal_ids_in_order(rows);
        let mut moved = Vec::new();
        match drop {
            Drop::IntoGroup(mut group) => {
                for id in ids {
                    let Some(row) = self.row_of(id) else { continue };
                    if let Some(new_row) = self.move_into_group(row, group) {
                        group = new_row.item; // the group may have shifted up
                        moved.push(new_row);
                    }
                }
            }
            Drop::Before(target) => {
                // A landmark that isn't moving: the target row, or the first
                // row after it that isn't one of the moving signals.
                let rows_now = self.rows();
                let start = target.and_then(|t| rows_now.iter().position(|&r| r == t)).unwrap_or(rows_now.len());
                let landmark = rows_now[start..]
                    .iter()
                    .find(|&&row| self.signal(row).is_none_or(|signal| !ids.contains(&signal.id)))
                    .copied();
                // Remember it by identity: a signal by its id, a divider or
                // group header by its position, corrected for the moved
                // top-level signals above it.
                let landmark_id = landmark.and_then(|row| self.signal(row)).map(|signal| signal.id);
                let landmark_top = landmark.filter(|_| landmark_id.is_none()).map(|row| {
                    let moved_above = ids
                        .iter()
                        .filter_map(|&id| self.row_of(id))
                        .filter(|r| r.child.is_none() && r.item < row.item)
                        .count();
                    row.item - moved_above
                });
                let signals: Vec<DisplayedSignal> = ids.iter().filter_map(|&id| self.take_signal(id)).collect();
                let mut before = match (landmark_id, landmark_top) {
                    (Some(id), _) => self.row_of(id),
                    (None, Some(item)) => Some(RowRef::top(item)),
                    (None, None) => None,
                };
                for signal in signals {
                    let row = self.insert_signal(signal, before);
                    moved.push(row);
                    // The landmark is now one further down.
                    before = before.map(|b| match b.child {
                        Some(child) => RowRef { item: b.item, child: Some(child + 1) },
                        None => RowRef::top(b.item + 1),
                    });
                }
            }
        }
        moved
    }

    /// Takes a row out of the list.
    fn take(&mut self, row: RowRef) -> Option<Taken> {
        match row.child {
            Some(child) => match self.items.get_mut(row.item)? {
                Item::Group(group) if child < group.signals.len() => Some(Taken::Signal(group.signals.remove(child))),
                _ => None,
            },
            None if row.item < self.items.len() => Some(match self.items.remove(row.item) {
                Item::Signal(signal) => Taken::Signal(signal),
                other => Taken::Item(other),
            }),
            None => None,
        }
    }
}

/// A row taken out of the list while moving it.
enum Taken {
    Signal(DisplayedSignal),
    /// A divider or a whole group.
    Item(Item),
}

impl Taken {
    fn into_item(self) -> Item {
        match self {
            Taken::Signal(signal) => Item::Signal(signal),
            Taken::Item(item) => item,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wave_style::WaveColour;
    use studio_core::Radix;

    fn signal(name: &str) -> DisplayedSignal {
        DisplayedSignal {
            // Distinct ids from the names' lengths and first letters are
            // enough for these tests.
            id: wave::SignalId::from_index(name.len() * 100 + name.as_bytes()[0] as usize),
            path: format!("tb.{name}"),
            label: name.to_string(),
            radix: Radix::Hex,
            is_bit: false,
            colour: WaveColour::Default,
        }
    }

    /// The list as text: signals by label, `| label` for dividers,
    /// `[name]` for groups with their signals indented as `  label`.
    fn picture(list: &SignalList) -> Vec<String> {
        list.rows()
            .into_iter()
            .map(|row| match list.kind(row).unwrap() {
                RowKind::Signal { signal, in_group: false } => signal.label.clone(),
                RowKind::Signal { signal, in_group: true } => format!("  {}", signal.label),
                RowKind::Divider(label) => format!("| {label}"),
                RowKind::GroupHeader { name, collapsed, count } => {
                    format!("[{name}{}]", if collapsed { format!(" +{count}") } else { String::new() })
                }
            })
            .collect()
    }

    fn list_of(names: &[&str]) -> SignalList {
        let mut list = SignalList::default();
        for name in names {
            list.push_signal(signal(name));
        }
        list
    }

    #[test]
    fn reordering_top_level_rows() {
        let mut list = list_of(&["a", "b", "c", "d"]);
        // Drag "a" down to just before "d".
        assert_eq!(list.move_row(RowRef::top(0), Some(RowRef::top(3))), Some(RowRef::top(2)));
        assert_eq!(picture(&list), ["b", "c", "a", "d"]);
        // Drag "d" up to the top.
        list.move_row(RowRef::top(3), Some(RowRef::top(0)));
        assert_eq!(picture(&list), ["d", "b", "c", "a"]);
        // Drag "b" to the end.
        list.move_row(RowRef::top(1), None);
        assert_eq!(picture(&list), ["d", "c", "a", "b"]);
    }

    #[test]
    fn groups_collect_signals_and_collapse() {
        let mut list = list_of(&["clk", "rx", "tx"]);
        let header = list.add_group_before(Some(RowRef::top(1)));
        assert_eq!(header, RowRef::top(1));
        assert_eq!(picture(&list), ["clk", "[Group 1]", "rx", "tx"]);

        // Drop rx and tx onto the group header.
        list.move_into_group(RowRef::top(2), 1);
        list.move_into_group(RowRef::top(2), 1);
        assert_eq!(picture(&list), ["clk", "[Group 1]", "  rx", "  tx"]);

        *list.title_mut(RowRef::top(1)).unwrap() = "uart".to_string();
        list.toggle_collapsed(1);
        assert_eq!(picture(&list), ["clk", "[uart +2]"]);
        assert_eq!(list.signals().count(), 3, "collapsed signals are still in the list");
        list.toggle_collapsed(1);

        // Drag tx above rx inside the group.
        list.move_row(RowRef { item: 1, child: Some(1) }, Some(RowRef { item: 1, child: Some(0) }));
        assert_eq!(picture(&list), ["clk", "[uart]", "  tx", "  rx"]);

        // Drag clk into the group, before rx.
        list.move_row(RowRef::top(0), Some(RowRef { item: 1, child: Some(1) }));
        assert_eq!(picture(&list), ["[uart]", "  tx", "  clk", "  rx"]);

        // And rx out of it, to the end.
        list.move_row(RowRef { item: 0, child: Some(2) }, None);
        assert_eq!(picture(&list), ["[uart]", "  tx", "  clk", "rx"]);

        list.ungroup(0);
        assert_eq!(picture(&list), ["tx", "clk", "rx"]);
    }

    #[test]
    fn a_whole_group_moves_together_and_stays_at_the_top_level() {
        let mut list = list_of(&["a", "b"]);
        list.add_group_before(None);
        list.move_into_group(RowRef::top(1), 2);
        assert_eq!(picture(&list), ["a", "[Group 1]", "  b"]);
        // Move the group (header row) above "a".
        list.move_row(RowRef::top(1), Some(RowRef::top(0)));
        assert_eq!(picture(&list), ["[Group 1]", "  b", "a"]);

        // A second group aimed inside the first goes before it instead.
        list.add_group_before(None);
        list.move_row(RowRef::top(2), Some(RowRef { item: 0, child: Some(0) }));
        assert_eq!(picture(&list), ["[Group 2]", "[Group 1]", "  b", "a"]);
    }

    #[test]
    fn dividers() {
        let mut list = list_of(&["a", "b"]);
        let divider = list.add_divider_after(Some(RowRef::top(0)));
        *list.title_mut(divider).unwrap() = "status".to_string();
        assert_eq!(picture(&list), ["a", "| status", "b"]);
        list.move_row(divider, None);
        assert_eq!(picture(&list), ["a", "b", "| status"]);
        list.remove(RowRef::top(2));
        assert_eq!(picture(&list), ["a", "b"]);
    }

    #[test]
    fn removing_rows() {
        let mut list = list_of(&["a", "b", "c"]);
        list.add_group_before(Some(RowRef::top(1)));
        list.move_into_group(RowRef::top(2), 1);
        list.move_into_group(RowRef::top(2), 1);
        assert_eq!(picture(&list), ["a", "[Group 1]", "  b", "  c"]);
        list.remove(RowRef { item: 1, child: Some(0) });
        assert_eq!(picture(&list), ["a", "[Group 1]", "  c"]);
        list.remove(RowRef::top(1));
        assert_eq!(picture(&list), ["a"], "removing a group removes its signals");
        assert!(!list.contains(signal("c").id));
    }

    #[test]
    fn dropping_rows_by_pointer_position() {
        let mut list = list_of(&["a", "b", "c"]);
        list.add_group_before(Some(RowRef::top(2)));
        // Rows: a, b, [Group 1], c.
        let a = RowRef::top(0);
        assert_eq!(list.drop_target(a, 1.4), Drop::Before(Some(RowRef::top(1))), "gap above b");
        assert_eq!(list.drop_target(a, 1.6), Drop::Before(Some(RowRef::top(2))), "gap below b");
        assert_eq!(list.drop_target(a, 2.5), Drop::IntoGroup(2), "middle of the group header");
        assert_eq!(list.drop_target(a, 3.9), Drop::Before(None), "below the last row");
        assert_eq!(list.drop_target(a, -2.0), Drop::Before(Some(RowRef::top(0))));

        // A group header can't go into a group: it lands in the nearest
        // gap instead (here the one below it, before c).
        let header = RowRef::top(2);
        assert_eq!(list.drop_target(header, 2.5), Drop::Before(Some(RowRef::top(3))));

        list.apply_drop(a, Drop::IntoGroup(2));
        assert_eq!(picture(&list), ["b", "[Group 1]", "  a", "c"]);
        list.apply_drop(RowRef::top(2), Drop::Before(Some(RowRef::top(0))));
        assert_eq!(picture(&list), ["c", "b", "[Group 1]", "  a"]);
    }

    #[test]
    fn grouping_several_rows() {
        let mut list = list_of(&["a", "b", "c", "d"]);
        let header = list.group_rows(&[RowRef::top(3), RowRef::top(1)]).unwrap();
        assert_eq!(header, RowRef::top(1), "where the first selected signal was");
        assert_eq!(picture(&list), ["a", "[Group 1]", "  b", "  d", "c"]);

        // Grouping signals that start inside a group puts the new group
        // before that group.
        list.group_rows(&[RowRef { item: 1, child: Some(1) }, RowRef::top(2)]);
        assert_eq!(picture(&list), ["a", "[Group 2]", "  d", "  c", "[Group 1]", "  b"]);
        assert!(list.group_rows(&[]).is_none());
    }

    #[test]
    fn removing_several_rows() {
        let mut list = list_of(&["a", "b", "c", "d"]);
        list.group_rows(&[RowRef::top(1), RowRef::top(2)]);
        // a, [Group 1], b, c, d
        list.remove_rows(&[RowRef::top(0), RowRef { item: 1, child: Some(1) }, RowRef::top(2)]);
        assert_eq!(picture(&list), ["[Group 1]", "  b"]);
    }

    #[test]
    fn moving_several_rows_keeps_their_order() {
        let mut list = list_of(&["a", "b", "c", "d", "e"]);
        list.move_rows(&[RowRef::top(3), RowRef::top(1)], Drop::Before(Some(RowRef::top(0))));
        assert_eq!(picture(&list), ["b", "d", "a", "c", "e"]);
        list.move_rows(&[RowRef::top(0), RowRef::top(2)], Drop::Before(None));
        assert_eq!(picture(&list), ["d", "c", "e", "b", "a"]);
        // Aimed at one of the moving rows: they land before the next row.
        let moved = list.move_rows(&[RowRef::top(1), RowRef::top(2)], Drop::Before(Some(RowRef::top(2))));
        assert_eq!(picture(&list), ["d", "c", "e", "b", "a"]);
        assert_eq!(moved, [RowRef::top(1), RowRef::top(2)]);
    }

    #[test]
    fn moving_several_rows_into_and_around_groups() {
        let mut list = list_of(&["a", "x", "b"]);
        list.group_rows(&[RowRef::top(1)]);
        // a, [Group 1], x, b
        list.move_rows(&[RowRef::top(0), RowRef::top(2)], Drop::IntoGroup(1));
        assert_eq!(picture(&list), ["[Group 1]", "  x", "  a", "  b"]);
        // Two of them out again, before the group.
        let rows = [RowRef { item: 0, child: Some(1) }, RowRef { item: 0, child: Some(2) }];
        list.move_rows(&rows, Drop::Before(Some(RowRef::top(0))));
        assert_eq!(picture(&list), ["a", "b", "[Group 1]", "  x"]);
        // And before x, inside the group.
        list.move_rows(&[RowRef::top(0), RowRef::top(1)], Drop::Before(Some(RowRef { item: 2, child: Some(0) })));
        assert_eq!(picture(&list), ["[Group 1]", "  a", "  b", "  x"]);
    }

    #[test]
    fn signal_rows_can_be_edited() {
        let mut list = list_of(&["count"]);
        let row = RowRef::top(0);
        list.signal_mut(row).unwrap().radix = Radix::Unsigned;
        list.signal_mut(row).unwrap().label = "beats".to_string();
        assert_eq!(list.signal(row).unwrap().radix, Radix::Unsigned);
        assert_eq!(picture(&list), ["beats"]);
        assert!(list.title_mut(row).is_none(), "signals are renamed through signal_mut");
        assert!(list.signal(RowRef { item: 0, child: Some(0) }).is_none());
    }
}
