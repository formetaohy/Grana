use std::sync::Arc;

use grana_measure::{Aggregate, Filter, Group, Lasso, MeasureView, Plot, PlotAxis, Scope, Table};

use glam::Vec3;
use grana_volume::Region;

use crate::object::ObjectFrame;
use crate::object_set::{ObjectSetId, Visibility};
use crate::scene::Scene;
use crate::state::{HASH_OFFSET, mix};

#[derive(Debug, PartialEq)]
pub struct MeasureState {
    pub set: Option<u64>,
    pub group: Group,
    pub aggregate: Aggregate,
    pub scope: Scope,
    pub filters: Vec<Filter>,
    pub lasso: Option<Lasso>,
    pub sort: Option<(usize, bool)>,
    pub columns: Vec<bool>,
    pub visibility: bool,
    pub view: MeasureView,
    pub axes: [usize; 2],
}

pub struct Measure {
    set: Option<ObjectSetId>,
    group: Group,
    aggregate: Aggregate,
    scope: Scope,
    filters: Vec<Filter>,
    lasso: Option<Lasso>,
    roi: Option<Region>,
    sort: Option<(usize, bool)>,
    table: Option<Table>,
    rows: Vec<usize>,
    total: usize,
    visible: Vec<bool>,
    visibility: bool,
    view: MeasureView,
    axes: [usize; 2],
    plot: Option<Plot>,
    revision: Option<(ObjectSetId, u64)>,
    time: u32,
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
            aggregate: Aggregate::MEAN,
            scope: Scope::All,
            filters: Vec::new(),
            lasso: None,
            roi: None,
            sort: None,
            table: None,
            rows: Vec::new(),
            total: 0,
            visible: Vec::new(),
            visibility: true,
            view: MeasureView::Table,
            axes: [0; 2],
            plot: None,
            revision: None,
            time: 0,
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
        self.time = scene.time();
        if !scene
            .objects
            .get(set)
            .is_some_and(|object| object.kind().trackable())
        {
            self.group = Group::Object;
        }
        self.filters.clear();
        self.lasso = None;
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
        self.time = scene.time();
        self.filters.clear();
        self.lasso = None;
        self.sort = None;
        self.rebuild(scene);
    }

    pub fn aggregate(&self) -> Aggregate {
        self.aggregate
    }

    pub fn scope(&self) -> Scope {
        self.scope
    }

    pub fn set_scope(&mut self, scope: Scope) {
        if self.scope == scope {
            return;
        }
        self.scope = scope;
        self.dirty = true;
        self.refresh();
    }

    pub fn lasso(&self) -> Option<&Lasso> {
        self.lasso.as_ref()
    }

    pub fn roi(&self) -> Option<Region> {
        self.roi
    }

    pub fn set_roi(&mut self, roi: Option<Region>) {
        assert!(
            roi.is_none_or(Region::enabled),
            "a statistics scope carries an enabled region"
        );
        if self.roi == roi {
            return;
        }
        self.roi = roi;
        self.dirty = true;
        self.refresh();
    }

    pub fn set_lasso(&mut self, lasso: Option<Lasso>) {
        if let Some(lasso) = &lasso {
            let Some(table) = self.table.as_ref() else {
                return;
            };
            if lasso.x() >= table.columns().len() || lasso.y() >= table.columns().len() {
                return;
            }
        }
        if self.lasso == lasso {
            return;
        }
        self.lasso = lasso;
        self.refresh();
    }

    pub fn total(&self) -> usize {
        self.total
    }

    pub fn view(&self) -> MeasureView {
        self.view
    }

    pub fn axes(&self) -> [usize; 2] {
        self.axes
    }

    pub fn plot(&self) -> Option<&Plot> {
        self.plot.as_ref()
    }

    pub fn set_view(&mut self, view: MeasureView) {
        if self.view == view {
            return;
        }
        self.view = view;
        self.dirty = true;
        self.refresh_plot();
    }

    pub fn set_axis(&mut self, axis: PlotAxis, column: usize) {
        let Some(table) = self.table.as_ref() else {
            return;
        };
        if column >= table.columns().len() || self.axes[axis.index()] == column {
            return;
        }
        self.axes[axis.index()] = column;
        self.dirty = true;
        if self
            .lasso
            .as_ref()
            .is_some_and(|lasso| lasso.x() != self.axes[0] || lasso.y() != self.axes[1])
        {
            self.lasso = None;
            self.refresh();
        } else {
            self.refresh_plot();
        }
    }

    pub fn set_gate(&mut self, column: usize, minimum: f32, maximum: f32) {
        let Some(table) = self.table.as_ref() else {
            return;
        };
        if column >= table.columns().len() {
            return;
        }
        let filter = Filter::new(column, minimum, maximum);
        match self
            .filters
            .iter()
            .position(|current| current.column() == column)
        {
            Some(index) => {
                if self.filters[index] == filter {
                    return;
                }
                self.filters[index] = filter;
            }
            None => self.filters.push(filter),
        }
        self.refresh();
    }

    pub fn set_aggregate(&mut self, aggregate: Aggregate, scene: &Scene) {
        if self.aggregate == aggregate {
            return;
        }
        self.aggregate = aggregate;
        self.time = scene.time();
        self.filters.clear();
        self.lasso = None;
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

    pub fn set_sort(&mut self, sort: Option<(usize, bool)>) {
        if let Some((column, _)) = sort
            && !self
                .table
                .as_ref()
                .is_some_and(|table| column < table.columns().len())
        {
            return;
        }
        if self.sort == sort {
            return;
        }
        self.sort = sort;
        self.refresh();
    }

    pub fn toggle_sort(&mut self, column: usize) {
        let sort = match self.sort {
            Some((current, false)) if current == column => Some((column, true)),
            Some((current, true)) if current == column => None,
            _ => Some((column, false)),
        };
        self.set_sort(sort);
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
        self.total = 0;
        self.visible.clear();
        self.scope = Scope::All;
        self.filters.clear();
        self.lasso = None;
        self.roi = None;
        self.sort = None;
        self.view = MeasureView::Table;
        self.axes = [0; 2];
        self.plot = None;
        self.revision = None;
        self.time = 0;
        self.dirty = true;
    }

    pub fn sync(&mut self, scene: &Scene, busy: bool) {
        let Some(id) = self.set else {
            return;
        };
        if scene.objects.get(id).is_none() {
            self.set = None;
            self.revision = None;
            self.table = None;
            self.plot = None;
            self.rows.clear();
            self.total = 0;
            self.visible.clear();
            self.lasso = None;
            self.axes = [0; 2];
            self.dirty = true;
            return;
        }
        let time = scene.time();
        let moved = self.time != time;
        self.time = time;
        if !busy && self.revision != scene.table_revision(id).map(|revision| (id, revision)) {
            self.rebuild(scene);
        } else if moved && self.scope == Scope::Current {
            self.refresh();
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
        let roi = self.roi.filter(|_| self.group == Group::Object);
        if !self.visibility || (self.filters.is_empty() && self.lasso.is_none() && roi.is_none()) {
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
        let rows = table.matching(&self.filters, self.lasso.as_ref(), None);
        let mut rows = rows;
        if let Some(roi) = roi {
            rows.retain(|row| holds_row(roi, table, *row));
        }
        let visibility = match self.group {
            Group::Object => {
                let mut frames = (0..object.frame_count())
                    .map(|time| {
                        vec![false; object.frame(time).map_or(0, ObjectFrame::object_count)]
                    })
                    .collect::<Vec<_>>();
                for row in rows {
                    let observation = table.observation(row);
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
                for row in rows {
                    if let Some(track) = table.observation(row).track()
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
            scope: self.scope,
            filters: self.filters.clone(),
            lasso: self.table.as_ref().and(self.lasso.clone()),
            sort: self.sort,
            columns: self.visible.clone(),
            visibility: self.visibility,
            view: self.view,
            axes: self.axes,
        }
    }

    pub fn restore(&mut self, state: MeasureState, scene: &Scene) {
        let MeasureState {
            set,
            group,
            aggregate,
            scope,
            filters,
            lasso,
            sort,
            columns,
            visibility,
            view,
            axes,
        } = state;
        self.set = None;
        self.group = group;
        self.aggregate = aggregate;
        self.scope = scope;
        self.time = scene.time();
        self.table = None;
        self.rows.clear();
        self.total = 0;
        self.visible.clear();
        self.filters.clear();
        self.lasso = None;
        self.sort = None;
        self.visibility = visibility;
        self.view = view;
        self.axes = axes;
        self.plot = None;
        self.revision = None;
        self.dirty = true;
        let Some(set) = set else {
            assert!(
                columns.is_empty()
                    && filters.is_empty()
                    && lasso.is_none()
                    && sort.is_none()
                    && axes == [0; 2],
                "a measure state without an object set carries no table state"
            );
            self.refresh();
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
        assert!(
            axes.iter().all(|axis| *axis < count) || (count == 0 && axes == [0; 2]),
            "a measure state names plot axes inside its table"
        );
        assert!(
            lasso
                .as_ref()
                .is_none_or(|lasso| lasso.x() < count && lasso.y() < count),
            "a measure state names lasso axes inside its table"
        );
        self.visible = columns;
        self.filters = filters;
        self.lasso = lasso;
        self.sort = sort;
        self.axes = match self.table.as_ref() {
            Some(table) => readable_axes(table, axes),
            None => axes,
        };
        self.refresh();
        self.dirty = true;
    }

    pub fn document_hash(&self) -> u64 {
        let mut hash = HASH_OFFSET;
        hash = mix(hash, self.set.map_or(0, ObjectSetId::raw));
        hash = mix(hash, u64::from(self.group.index()));
        hash = mix(hash, u64::from(self.aggregate.index()));
        hash = mix(hash, u64::from(self.scope.index()));
        for filter in &self.filters {
            hash = mix(hash, filter.column() as u64);
            hash = mix(hash, u64::from(filter.minimum().to_bits()));
            hash = mix(hash, u64::from(filter.maximum().to_bits()));
        }
        if let Some(lasso) = &self.lasso {
            hash = mix(hash, lasso.x() as u64);
            hash = mix(hash, lasso.y() as u64);
            hash = mix(hash, lasso.points().len() as u64);
            for point in lasso.points() {
                hash = mix(hash, u64::from(point[0].to_bits()));
                hash = mix(hash, u64::from(point[1].to_bits()));
            }
        }
        if let Some((column, descending)) = self.sort {
            hash = mix(hash, column as u64);
            hash = mix(hash, u64::from(u8::from(descending)));
        }
        for visible in &self.visible {
            hash = mix(hash, u64::from(u8::from(*visible)));
        }
        hash = mix(hash, u64::from(u8::from(self.visibility)));
        hash = mix(hash, u64::from(self.view.index()));
        hash = mix(hash, self.axes[0] as u64);
        mix(hash, self.axes[1] as u64)
    }

    fn rebuild(&mut self, scene: &Scene) {
        self.dirty = true;
        self.table = None;
        self.plot = None;
        self.rows.clear();
        self.total = 0;
        self.visible.clear();
        let Some(id) = self.set else {
            self.revision = None;
            self.visible.clear();
            self.lasso = None;
            self.axes = [0; 2];
            return;
        };
        if scene.objects.get(id).is_none() {
            self.set = None;
            self.revision = None;
            self.visible.clear();
            self.lasso = None;
            self.axes = [0; 2];
            return;
        }
        self.revision = scene.table_revision(id).map(|revision| (id, revision));
        let Some(table) = scene.table(id, self.group, self.aggregate) else {
            self.visible.clear();
            self.lasso = None;
            self.axes = [0; 2];
            return;
        };
        self.visible = default_visibility(&table);
        self.axes = readable_axes(&table, self.axes);
        let columns = table.columns().len();
        self.filters.retain(|filter| filter.column() < columns);
        if self
            .lasso
            .as_ref()
            .is_some_and(|lasso| lasso.x() >= columns || lasso.y() >= columns)
        {
            self.lasso = None;
        }
        if self.sort.is_some_and(|(column, _)| column >= columns) {
            self.sort = None;
        }
        self.table = Some(table);
        self.refresh();
    }

    fn refresh(&mut self) {
        self.dirty = true;
        let Some(table) = self.table.as_ref() else {
            self.rows.clear();
            self.total = 0;
            self.plot = None;
            return;
        };
        let frame = self.scope_frame();
        let roi = self.roi.filter(|_| self.group == Group::Object);
        self.total = match roi {
            None => table.count(frame),
            Some(roi) => (0..table.rows())
                .filter(|row| {
                    frame.is_none_or(|time| table.observation(*row).time() == Some(time))
                        && holds_row(roi, table, *row)
                })
                .count(),
        };
        let mut rows = table.matching(&self.filters, self.lasso.as_ref(), frame);
        if let Some(roi) = roi {
            rows.retain(|row| holds_row(roi, table, *row));
        }
        if let Some((column, descending)) = self.sort
            && column < table.columns().len()
        {
            table.sort(&mut rows, column, descending);
        }
        self.rows = rows;
        self.refresh_plot();
    }

    fn scope_frame(&self) -> Option<u32> {
        (self.scope == Scope::Current && self.group != Group::Track).then_some(self.time)
    }

    fn refresh_plot(&mut self) {
        self.plot = self
            .table
            .as_ref()
            .and_then(|table| table.plot(self.view, self.axes, &self.rows));
    }
}

fn holds_row(region: Region, table: &Table, row: usize) -> bool {
    table
        .position(row)
        .is_none_or(|position| region.contains(Vec3::from(position)))
}

fn readable_axes(table: &Table, axes: [usize; 2]) -> [usize; 2] {
    let columns = table.columns().len();
    if axes == [0; 2] || axes[0] >= columns || axes[1] >= columns {
        return preferred_axes(table);
    }
    axes
}

fn preferred_axes(table: &Table) -> [usize; 2] {
    let columns = table.columns();
    let identity = table.identity_columns();
    let varies = |index: usize| {
        table
            .extent(index)
            .is_some_and(|(low, high)| low.is_finite() && high.is_finite() && high > low)
    };
    let key_family = |key: &str| columns.iter().any(|column| column.key() == key);
    let preferred: &[&str] = if key_family("contrast") {
        &["diameter_um", "contrast"]
    } else if key_family("junctions") {
        &["length_um", "diameter_um"]
    } else {
        &["volume_um3", "hull_volume_um3", "area_um2", "sphericity"]
    };
    let mut chosen = Vec::with_capacity(2);
    for key in preferred {
        if chosen.len() == 2 {
            break;
        }
        if let Some(index) = columns.iter().position(|column| column.key() == *key)
            && varies(index)
            && !chosen.contains(&index)
        {
            chosen.push(index);
        }
    }
    let mut scored = Vec::new();
    for index in identity..columns.len() {
        if chosen.contains(&index) || positional(columns, index) {
            continue;
        }
        let Some((low, high)) = table.extent(index) else {
            continue;
        };
        if !(low.is_finite() && high.is_finite()) {
            continue;
        }
        let span = high - low;
        if span <= 0.0 {
            continue;
        }
        let scale = low.abs().max(high.abs()).max(1.0e-6);
        scored.push((span / scale, index));
    }
    scored.sort_by(|left, right| right.0.total_cmp(&left.0));
    for (_, index) in scored {
        if chosen.len() == 2 {
            break;
        }
        chosen.push(index);
    }
    let x = chosen
        .first()
        .copied()
        .unwrap_or(identity.min(columns.len() - 1));
    let y = chosen.get(1).copied().unwrap_or(x);
    [x, y]
}

fn positional(columns: &[grana_measure::Column], index: usize) -> bool {
    let key = columns[index].key();
    key.starts_with("x_um") || key.starts_with("y_um") || key.starts_with("z_um")
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
                    || key.starts_with("max_diameter_um")
                    || key.starts_with("hull_volume_um3")
                    || key.contains("_intensity_mean")
                    || key.starts_with("pearson_")
                    || key.starts_with("overlap_")
                    || key.ends_with("_distance_um")
                    || key.ends_with("_inside")
                    || key.ends_with("_count")
                    || key == grana_measure::SCORE_KEY
            }
        })
        .collect()
}
