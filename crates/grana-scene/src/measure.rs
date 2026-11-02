use std::sync::Arc;

use grana_measure::{Aggregate, Filter, Frame, Group, Table};

use crate::object::ObjectFrame;
use crate::object_set::{ObjectSetId, Visibility};
use crate::scene::Scene;
use crate::state::{HASH_OFFSET, mix};

#[derive(Debug, PartialEq)]
pub struct MeasureState {
    pub set: Option<u64>,
    pub group: Group,
    pub aggregate: Aggregate,
    pub filters: Vec<Filter>,
    pub sort: Option<(usize, bool)>,
    pub columns: Vec<bool>,
    pub visibility: bool,
}

pub struct Measure {
    set: Option<ObjectSetId>,
    group: Group,
    aggregate: Aggregate,
    filters: Vec<Filter>,
    sort: Option<(usize, bool)>,
    table: Option<Table>,
    rows: Vec<usize>,
    visible: Vec<bool>,
    visibility: bool,
    revision: Option<(ObjectSetId, u64)>,
    dirty: bool,
}

impl Default for Measure {
    fn default() -> Self {
        Self::new()
    }
}

impl Measure {
    pub fn new() -> Self {
        Self {
            set: None,
            group: Group::Object,
            aggregate: Aggregate::Mean,
            filters: Vec::new(),
            sort: None,
            table: None,
            rows: Vec::new(),
            visible: Vec::new(),
            visibility: true,
            revision: None,
            dirty: false,
        }
    }

    pub fn set(&self) -> Option<ObjectSetId> {
        self.set
    }

    pub fn select(&mut self, set: ObjectSetId, scene: &Scene) {
        if self.set == Some(set) {
            return;
        }
        self.set = Some(set);
        if !scene
            .objects
            .get(set)
            .is_some_and(|object| object.kind().trackable())
        {
            self.group = Group::Object;
        }
        self.filters.clear();
        self.sort = None;
        self.rebuild(scene);
    }

    pub fn group(&self) -> Group {
        self.group
    }

    pub fn set_group(&mut self, group: Group, scene: &Scene) {
        if self.group == group {
            return;
        }
        let trackable = self
            .set
            .and_then(|set| scene.objects.get(set))
            .is_some_and(|object| object.kind().trackable());
        if group == Group::Track && !trackable {
            return;
        }
        self.group = group;
        self.filters.clear();
        self.sort = None;
        self.rebuild(scene);
    }

    pub fn aggregate(&self) -> Aggregate {
        self.aggregate
    }

    pub fn set_aggregate(&mut self, aggregate: Aggregate, scene: &Scene) {
        if self.aggregate == aggregate {
            return;
        }
        self.aggregate = aggregate;
        self.filters.clear();
        self.rebuild(scene);
    }

    pub fn filters(&self) -> &[Filter] {
        &self.filters
    }

    pub fn add_filter(&mut self) {
        let Some(table) = self.table.as_ref() else {
            return;
        };
        let column = (table.identity_columns()..table.columns().len())
            .find(|index| {
                let key = table.columns()[*index].key();
                !(key.starts_with("x_um") || key.starts_with("y_um") || key.starts_with("z_um"))
            })
            .unwrap_or(table.identity_columns());
        let Some((minimum, maximum)) = table.extent(column) else {
            return;
        };
        self.filters.push(Filter::new(column, minimum, maximum));
        self.refresh();
    }

    pub fn set_filter(&mut self, index: usize, filter: Filter) {
        let Some(slot) = self.filters.get_mut(index) else {
            return;
        };
        if *slot == filter {
            return;
        }
        *slot = filter;
        self.refresh();
    }

    pub fn remove_filter(&mut self, index: usize) {
        if index >= self.filters.len() {
            return;
        }
        self.filters.remove(index);
        self.refresh();
    }

    pub fn retarget_filter(&mut self, index: usize, column: usize) {
        let Some(filter) = self.filters.get(index).copied() else {
            return;
        };
        if filter.column() == column {
            return;
        }
        let Some(table) = self.table.as_ref() else {
            return;
        };
        let Some((minimum, maximum)) = table.extent(column) else {
            return;
        };
        self.set_filter(index, Filter::new(column, minimum, maximum));
    }

    pub fn visibility(&self) -> bool {
        self.visibility
    }

    pub fn set_visibility(&mut self, visibility: bool) {
        if self.visibility == visibility {
            return;
        }
        self.visibility = visibility;
        self.dirty = true;
    }

    pub fn sort(&self) -> Option<(usize, bool)> {
        self.sort
    }

    pub fn toggle_sort(&mut self, column: usize) {
        self.sort = match self.sort {
            Some((current, false)) if current == column => Some((column, true)),
            Some((current, true)) if current == column => None,
            _ => Some((column, false)),
        };
        self.refresh();
    }

    pub fn table(&self) -> Option<&Table> {
        self.table.as_ref()
    }

    pub fn rows(&self) -> &[usize] {
        &self.rows
    }

    pub fn visible(&self) -> &[bool] {
        &self.visible
    }

    pub fn set_column_visible(&mut self, column: usize, visible: bool) {
        let Some(slot) = self.visible.get_mut(column) else {
            return;
        };
        *slot = visible;
    }

    pub fn show_all_columns(&mut self) {
        self.visible.fill(true);
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn reset(&mut self) {
        self.set = None;
        self.table = None;
        self.rows.clear();
        self.visible.clear();
        self.filters.clear();
        self.sort = None;
        self.revision = None;
        self.dirty = true;
    }

    pub fn sync(&mut self, scene: &Scene, busy: bool) {
        let Some(id) = self.set else {
            return;
        };
        let Some(object) = scene.objects.get(id) else {
            self.set = None;
            self.revision = None;
            self.table = None;
            self.rows.clear();
            self.visible.clear();
            self.dirty = true;
            return;
        };
        if !busy && self.revision != Some((id, object.revision())) {
            self.rebuild(scene);
        }
    }

    pub fn apply(&mut self, scene: &mut Scene) {
        self.dirty = false;
        let Some(id) = self.set else {
            return;
        };
        let Some(object) = scene.objects.get_mut(id) else {
            return;
        };
        if !self.visibility || self.filters.is_empty() {
            object.set_visibility(Visibility::All);
            return;
        }
        let Some(table) = self.table.as_ref() else {
            return;
        };
        if table.rows() == 0 {
            object.set_visibility(Visibility::All);
            return;
        }
        let visibility = match self.group {
            Group::Object => {
                let mut frames = (0..object.frame_count())
                    .map(|time| {
                        vec![false; object.frame(time).map_or(0, ObjectFrame::object_count)]
                    })
                    .collect::<Vec<_>>();
                for row in &self.rows {
                    let observation = table.observation(*row);
                    if let (Some(time), Some(index)) = (observation.time(), observation.object())
                        && let Some(mask) = frames.get_mut(time as usize)
                        && let Some(value) = mask.get_mut(index)
                    {
                        *value = true;
                    }
                }
                Visibility::Object(Arc::new(frames))
            }
            Group::Track => {
                let count = object.tracks().map_or(0, |layer| layer.set().len());
                let mut mask = vec![false; count];
                for row in &self.rows {
                    if let Some(track) = table.observation(*row).track()
                        && let Some(value) = mask.get_mut(track)
                    {
                        *value = true;
                    }
                }
                Visibility::Track(Arc::new(mask))
            }
            Group::Timepoint => Visibility::All,
        };
        object.set_visibility(visibility);
    }

    pub fn csv(&self) -> Option<String> {
        Some(self.table.as_ref()?.csv(&self.rows))
    }

    pub fn state(&self) -> MeasureState {
        MeasureState {
            set: self.set.map(ObjectSetId::raw),
            group: self.group,
            aggregate: self.aggregate,
            filters: self.filters.clone(),
            sort: self.sort,
            columns: self.visible.clone(),
            visibility: self.visibility,
        }
    }

    pub fn restore(&mut self, state: MeasureState, scene: &Scene) {
        let MeasureState {
            set,
            group,
            aggregate,
            filters,
            sort,
            columns,
            visibility,
        } = state;
        self.set = None;
        self.group = group;
        self.aggregate = aggregate;
        self.table = None;
        self.rows.clear();
        self.visible.clear();
        self.filters.clear();
        self.sort = None;
        self.visibility = visibility;
        self.revision = None;
        self.dirty = true;
        let Some(set) = set else {
            assert!(
                columns.is_empty() && filters.is_empty() && sort.is_none(),
                "a measure state without an object set carries no table state"
            );
            return;
        };
        self.set = Some(ObjectSetId::new(set));
        self.rebuild(scene);
        let count = self.table.as_ref().map_or(0, |table| table.columns().len());
        assert_eq!(
            columns.len(),
            count,
            "a measure state covers every column of its table"
        );
        assert!(
            filters.iter().all(|filter| filter.column() < count)
                && sort.is_none_or(|(column, _)| column < count),
            "a measure state names columns inside its table"
        );
        self.visible = columns;
        self.filters = filters;
        self.sort = sort;
        self.refresh();
        self.dirty = true;
    }

    pub fn document_hash(&self) -> u64 {
        let mut hash = HASH_OFFSET;
        hash = mix(hash, self.set.map_or(0, ObjectSetId::raw));
        hash = mix(hash, u64::from(self.group.index()));
        hash = mix(hash, u64::from(self.aggregate.index()));
        for filter in &self.filters {
            hash = mix(hash, filter.column() as u64);
            hash = mix(hash, u64::from(filter.minimum().to_bits()));
            hash = mix(hash, u64::from(filter.maximum().to_bits()));
        }
        if let Some((column, descending)) = self.sort {
            hash = mix(hash, column as u64);
            hash = mix(hash, u64::from(u8::from(descending)));
        }
        for visible in &self.visible {
            hash = mix(hash, u64::from(u8::from(*visible)));
        }
        mix(hash, u64::from(u8::from(self.visibility)))
    }

    fn rebuild(&mut self, scene: &Scene) {
        self.dirty = true;
        self.table = None;
        self.rows.clear();
        let Some(id) = self.set else {
            self.revision = None;
            self.visible.clear();
            return;
        };
        let Some(object) = scene.objects.get(id) else {
            self.set = None;
            self.revision = None;
            self.visible.clear();
            return;
        };
        self.revision = Some((id, object.revision()));
        let frames = object
            .frames()
            .map(|(time, frame)| Frame::new(time, frame.objects()))
            .collect::<Vec<_>>();
        if frames.is_empty() {
            self.visible.clear();
            return;
        }
        let channels = scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>();
        let tracks = object.tracks().map(|layer| layer.set());
        self.table = Some(Table::build(
            &frames,
            tracks,
            &channels,
            scene.dataset.frame_interval(),
            self.group,
            self.aggregate,
        ));
        self.visible = default_visibility(self.table.as_ref().expect("the table exists"));
        self.refresh();
    }

    fn refresh(&mut self) {
        self.dirty = true;
        let Some(table) = self.table.as_ref() else {
            self.rows.clear();
            return;
        };
        let mut rows = table.matching(&self.filters);
        if let Some((column, descending)) = self.sort
            && column < table.columns().len()
        {
            table.sort(&mut rows, column, descending);
        }
        self.rows = rows;
    }
}

fn default_visibility(table: &Table) -> Vec<bool> {
    let identity = table.identity_columns();
    table
        .columns()
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let key = column.key();
            if index < identity {
                !matches!(key, "first_frame" | "last_frame" | "nodes" | "straightness")
            } else {
                key.starts_with("diameter_um")
                    || key.starts_with("length_um")
                    || key.starts_with("contrast")
                    || key.starts_with("branches")
                    || key.starts_with("terminals")
                    || key.starts_with("volume_um3")
                    || key.starts_with("area_um2")
                    || key.starts_with("sphericity")
                    || key.contains("_intensity_mean")
            }
        })
        .collect()
}
