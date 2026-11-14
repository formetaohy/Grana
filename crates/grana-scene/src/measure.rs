use std::collections::BTreeSet;
use std::sync::Arc;

use grana_measure::{
    Aggregate, Filter, Group, Lasso, Mask, MeasureView, Plot, PlotAxis, Scope, Table, Window,
};

use glam::Vec3;
use grana_volume::Region;

use crate::object_set::{ObjectSetId, Visibility};
use crate::scene::Scene;
use crate::state::{HASH_OFFSET, mix};
use crate::table_request::{BasisKey, MeasureKey, TableRequest};

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
    table: Option<Arc<Table>>,
    basis: Option<(BasisKey, Arc<Table>)>,
    retained: Option<(Vec<String>, Vec<bool>)>,
    rows: Vec<usize>,
    mask: Arc<Mask>,
    frame_mask: Option<(Option<u32>, Mask)>,
    region_mask: Option<(Option<Region>, Mask)>,
    scope_mask: Mask,
    filter_masks: Vec<(Filter, Mask)>,
    lasso_mask: Option<(Lasso, Mask)>,
    gate_mask: Mask,
    total: usize,
    visible: Vec<bool>,
    visibility: bool,
    view: MeasureView,
    axes: [usize; 2],
    plot: Option<Plot>,
    plot_wanted: bool,
    plot_pending: Option<PlotKey>,
    plot_stamp: u64,
    origin: Option<MeasureKey>,
    pending: Option<MeasureKey>,
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
            basis: None,
            retained: None,
            rows: Vec::new(),
            mask: Arc::new(Mask::none(0)),
            frame_mask: None,
            region_mask: None,
            scope_mask: Mask::none(0),
            filter_masks: Vec::new(),
            lasso_mask: None,
            gate_mask: Mask::none(0),
            total: 0,
            visible: Vec::new(),
            visibility: true,
            view: MeasureView::Table,
            axes: [0; 2],
            plot: None,
            plot_wanted: false,
            plot_pending: None,
            plot_stamp: 0,
            origin: None,
            pending: None,
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
        self.basis = None;
        self.time = scene.time();
        if self.group == Group::Molecule && !molecule_grouping(scene, set) {
            self.group = Group::Object;
        }
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
        self.invalidate();
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
        if group == Group::Molecule && !self.set.is_some_and(|set| molecule_grouping(scene, set)) {
            return;
        }
        self.group = group;
        self.time = scene.time();
        self.filters.clear();
        self.lasso = None;
        self.sort = None;
        self.invalidate();
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

    pub fn plot_request(&mut self, window: Option<Window>) -> Option<PlotRequest> {
        if !self.plot_wanted || self.plot_pending.is_some() || self.view.plot().is_none() {
            return None;
        }
        if self.view == MeasureView::Ripley && window.is_none() {
            return None;
        }
        let table = Arc::clone(self.table.as_ref()?);
        self.plot_stamp += 1;
        let key = PlotKey(self.plot_stamp);
        self.plot_wanted = false;
        self.plot_pending = Some(key);
        Some(PlotRequest {
            key,
            table,
            mask: Arc::clone(&self.mask),
            view: self.view,
            axes: self.axes,
            window,
        })
    }

    pub fn install_plot(&mut self, key: PlotKey, plot: Option<Plot>) {
        if self.plot_pending != Some(key) {
            return;
        }
        self.plot_pending = None;
        self.plot = plot.filter(|_| self.view.plot().is_some());
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
        self.invalidate();
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
        self.table.as_deref()
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
        self.basis = None;
        self.retained = None;
        self.rows.clear();
        self.total = 0;
        self.clear_masks();
        self.visible.clear();
        self.scope = Scope::All;
        self.filters.clear();
        self.lasso = None;
        self.roi = None;
        self.sort = None;
        self.view = MeasureView::Table;
        self.axes = [0; 2];
        self.clear_plot();
        self.origin = None;
        self.pending = None;
        self.time = 0;
        self.dirty = true;
    }

    pub fn request(&mut self, scene: &Scene, busy: bool) -> Option<TableRequest> {
        let id = self.set?;
        if scene.objects.get(id).is_none() {
            self.set = None;
            self.origin = None;
            self.pending = None;
            self.table = None;
            self.retained = None;
            self.clear_plot();
            self.rows.clear();
            self.total = 0;
            self.clear_masks();
            self.visible.clear();
            self.lasso = None;
            self.axes = [0; 2];
            self.dirty = true;
            return None;
        }
        let time = scene.time();
        let moved = self.time != time;
        self.time = time;
        if !busy && !self.settled(scene) {
            let key = MeasureKey::of(scene, id, self.group, self.aggregate)?;
            if self.group == Group::Object
                && let Some((held, table)) = self.basis.as_ref()
                && *held == key.basis()
            {
                self.pending = Some(key);
                self.install(key, Some(Arc::clone(table)));
                return None;
            }
            let request = self.laid(
                scene,
                id,
                TableRequest::new(scene, id, self.group, self.aggregate)?,
            );
            self.pending = Some(request.key());
            return Some(request);
        }
        if moved && self.scope == Scope::Current {
            self.refresh();
        }
        None
    }

    pub fn install(&mut self, key: MeasureKey, table: Option<Arc<Table>>) {
        if self.pending != Some(key) {
            return;
        }
        self.pending = None;
        self.origin = Some(key);
        if let Some(table) = table.as_ref() {
            self.remember_basis(key, table);
        }
        self.adopt(table);
    }

    fn laid(&self, scene: &Scene, id: ObjectSetId, request: TableRequest) -> TableRequest {
        let Some(key) = MeasureKey::of(scene, id, self.group, self.aggregate) else {
            return request;
        };
        match self.basis.as_ref() {
            Some((held, table)) if *held == key.basis() => request.with_basis(Arc::clone(table)),
            _ => request,
        }
    }

    fn remember_basis(&mut self, key: MeasureKey, table: &Arc<Table>) {
        let basis = match key.group() {
            Group::Object => Some(Arc::clone(table)),
            Group::Track | Group::Timepoint => table.basis_shared(),
            Group::Molecule => None,
        };
        self.basis = basis.map(|basis| (key.basis(), basis));
    }

    pub fn settled(&self, scene: &Scene) -> bool {
        let Some(id) = self.set else {
            return false;
        };
        self.origin.is_some_and(|origin| {
            Some(origin) == MeasureKey::of(scene, id, self.group, self.aggregate)
        })
    }

    pub fn current(&self, scene: &Scene) -> Option<&Table> {
        if !self.settled(scene) {
            return None;
        }
        self.table.as_deref()
    }

    pub fn export(&self, scene: &Scene) -> Option<(Arc<Table>, Vec<usize>)> {
        if !self.settled(scene) {
            return None;
        }
        self.table
            .as_ref()
            .map(|table| (Arc::clone(table), self.rows.clone()))
    }

    pub fn apply(&mut self, scene: &mut Scene) {
        self.dirty = false;
        let Some(id) = self.set else {
            return;
        };
        let Some(visibility) = self.restriction(scene, id) else {
            return;
        };
        if let Some(object) = scene.objects.get_mut(id) {
            object.set_visibility(visibility);
        }
    }

    fn restriction(&self, scene: &Scene, id: ObjectSetId) -> Option<Visibility> {
        let object = scene.objects.get(id)?;
        let roi = self.roi.filter(|_| self.group != Group::Track);
        if !self.visibility || (self.filters.is_empty() && self.lasso.is_none() && roi.is_none()) {
            return Some(Visibility::All);
        }
        let table = self.current(scene)?;
        if table.rows() == 0 {
            return Some(Visibility::All);
        }
        let mut mask = self.gate_mask.clone();
        if let Some((_, region)) = &self.region_mask {
            mask.intersect(region);
        }
        Some(match self.group {
            Group::Object => {
                let mut frames = (0..object.frame_count())
                    .map(|time| vec![false; object.object_count(time)])
                    .collect::<Vec<_>>();
                for row in mask.iter() {
                    let observation = table.observation(row);
                    if let (Some(time), Some(index)) = (observation.time(), observation.object())
                        && let Some(frame) = frames.get_mut(time as usize)
                        && let Some(value) = frame.get_mut(index)
                    {
                        *value = true;
                    }
                }
                Visibility::Object(Arc::new(frames))
            }
            Group::Track => {
                let count = object.tracks().map_or(0, |layer| layer.set().len());
                let mut visible = vec![false; count];
                for row in mask.iter() {
                    if let Some(track) = table.observation(row).track()
                        && let Some(value) = visible.get_mut(track)
                    {
                        *value = true;
                    }
                }
                Visibility::Track(Arc::new(visible))
            }
            Group::Molecule => {
                let admitted = mask
                    .iter()
                    .filter_map(|row| table.observation(row).molecule())
                    .collect::<BTreeSet<_>>();
                let mut frames = (0..object.frame_count())
                    .map(|time| vec![false; object.object_count(time)])
                    .collect::<Vec<_>>();
                for (time, frame) in object.frames() {
                    let Some(spots) = frame.spots() else {
                        continue;
                    };
                    for (index, spot) in spots.spots().iter().enumerate() {
                        if spot.group().is_some_and(|group| admitted.contains(&group)) {
                            frames[time as usize][index] = true;
                        }
                    }
                }
                Visibility::Object(Arc::new(frames))
            }
            Group::Timepoint => Visibility::All,
        })
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
        self.clear_masks();
        self.visible.clear();
        self.filters.clear();
        self.lasso = None;
        self.sort = None;
        self.visibility = visibility;
        self.view = view;
        self.axes = axes;
        self.clear_plot();
        self.origin = None;
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
        self.retained = None;
        self.rebuild(scene);
        let count = self.table.as_ref().map_or(0, |table| table.columns().len());
        assert!(
            columns.is_empty() || columns.len() == count,
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
        self.visible = match self.table.as_ref() {
            Some(table) if columns.is_empty() => default_visibility(table),
            _ => columns,
        };
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

    pub fn rebuild(&mut self, scene: &Scene) {
        self.dirty = true;
        self.pending = None;
        self.clear_plot();
        self.rows.clear();
        self.total = 0;
        self.clear_masks();
        self.origin = None;
        let Some(id) = self.set else {
            self.table = None;
            self.visible.clear();
            self.lasso = None;
            self.axes = [0; 2];
            return;
        };
        if scene.objects.get(id).is_none() {
            self.set = None;
            self.table = None;
            self.visible.clear();
            self.lasso = None;
            self.axes = [0; 2];
            return;
        }
        let Some(request) = TableRequest::new(scene, id, self.group, self.aggregate) else {
            self.table = None;
            return;
        };
        let request = self.laid(scene, id, request);
        let key = request.key();
        self.origin = Some(key);
        let table = request.build().map(Arc::new);
        if let Some(table) = table.as_ref() {
            self.remember_basis(key, table);
        }
        self.adopt(table);
    }

    fn invalidate(&mut self) {
        self.dirty = true;
        self.pending = None;
        self.origin = None;
        self.clear_plot();
        if let Some(table) = self.table.take() {
            let keys = column_keys(&table);
            self.retained = Some((keys, std::mem::take(&mut self.visible)));
        }
        self.rows.clear();
        self.total = 0;
        self.clear_masks();
        self.lasso = None;
        self.axes = [0; 2];
    }

    fn adopt(&mut self, table: Option<Arc<Table>>) {
        self.dirty = true;
        self.clear_plot();
        self.rows.clear();
        self.total = 0;
        self.clear_masks();
        let previous = self
            .table
            .take()
            .map(|table| (column_keys(&table), std::mem::take(&mut self.visible)))
            .or_else(|| self.retained.take());
        let Some(table) = table else {
            self.table = None;
            self.visible.clear();
            self.lasso = None;
            self.axes = [0; 2];
            return;
        };
        let keys = column_keys(&table);
        let columns = keys.len();
        self.visible = default_visibility(&table);
        if let Some((previous, flags)) = previous {
            for (index, key) in previous.iter().enumerate() {
                if let Some(current) = keys.iter().position(|candidate| candidate == key)
                    && let Some(flag) = flags.get(index)
                {
                    self.visible[current] = *flag;
                }
            }
            self.filters
                .retain_mut(|filter| match remap(&previous, &keys, filter.column()) {
                    Some(column) => {
                        *filter = Filter::new(column, filter.minimum(), filter.maximum());
                        true
                    }
                    None => false,
                });
            self.sort = self.sort.and_then(|(column, descending)| {
                remap(&previous, &keys, column).map(|at| (at, descending))
            });
            self.lasso = self.lasso.take().and_then(|lasso| {
                let x = remap(&previous, &keys, lasso.x())?;
                let y = remap(&previous, &keys, lasso.y())?;
                Lasso::new(x, y, lasso.points().to_vec())
            });
            self.axes = [
                remap(&previous, &keys, self.axes[0]).unwrap_or(self.axes[0]),
                remap(&previous, &keys, self.axes[1]).unwrap_or(self.axes[1]),
            ];
        }
        self.filters.retain(|filter| filter.column() < columns);
        self.axes = readable_axes(&table, self.axes);
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
        let Some(table) = self.table.clone() else {
            self.rows.clear();
            self.total = 0;
            self.clear_masks();
            self.clear_plot();
            return;
        };
        let frame = self.scope_frame();
        let roi = self.roi.filter(|_| self.group != Group::Track);
        let table = table.as_ref();
        self.sync_scope(table, frame, roi);
        self.sync_gate(table);
        let mut mask = self.scope_mask.clone();
        mask.intersect(&self.gate_mask);
        self.total = self.scope_mask.count() as usize;
        let mut rows = mask.to_rows();
        if let Some((column, descending)) = self.sort
            && column < table.columns().len()
        {
            table.sort(&mut rows, column, descending);
        }
        self.rows = rows;
        self.mask = Arc::new(mask);
        self.refresh_plot();
    }

    fn sync_scope(&mut self, table: &Table, frame: Option<u32>, roi: Option<Region>) {
        let stale = match &self.frame_mask {
            Some((cached, _)) => *cached != frame,
            None => true,
        };
        if stale {
            self.frame_mask = Some((frame, table.frame_mask(frame)));
        }
        let stale = match &self.region_mask {
            Some((cached, _)) => *cached != roi,
            None => true,
        };
        if stale {
            let mask = roi.map_or_else(
                || Mask::all(table.rows()),
                |region| Mask::of(table.rows(), |row| holds_row(region, table, row)),
            );
            self.region_mask = Some((roi, mask));
        }
        let (_, frame_mask) = self
            .frame_mask
            .as_ref()
            .expect("a refreshed measure frames its table");
        let (_, region_mask) = self
            .region_mask
            .as_ref()
            .expect("a refreshed measure bounds its table");
        self.scope_mask.clone_from(frame_mask);
        self.scope_mask.intersect(region_mask);
    }

    fn sync_gate(&mut self, table: &Table) {
        if self.filters.is_empty() && self.lasso.is_none() {
            if self.filter_masks.is_empty()
                && self.lasso_mask.is_none()
                && self.gate_mask.rows() == table.rows()
            {
                return;
            }
            self.filter_masks.clear();
            self.lasso_mask = None;
            self.gate_mask = Mask::all(table.rows());
            return;
        }
        let mut masks = Vec::with_capacity(self.filters.len());
        for filter in &self.filters {
            match self
                .filter_masks
                .iter()
                .position(|(cached, _)| cached == filter)
            {
                Some(position) => masks.push(self.filter_masks.swap_remove(position)),
                None => masks.push((*filter, table.filter_mask(*filter))),
            }
        }
        self.filter_masks = masks;
        let stale = match (&self.lasso, &self.lasso_mask) {
            (None, None) => false,
            (Some(lasso), Some((cached, _))) => cached != lasso,
            _ => true,
        };
        if stale {
            self.lasso_mask = self
                .lasso
                .as_ref()
                .map(|lasso| (lasso.clone(), table.lasso_mask(lasso)));
        }
        let mut gate = Mask::all(table.rows());
        for (_, mask) in &self.filter_masks {
            gate.intersect(mask);
        }
        if let Some((_, mask)) = &self.lasso_mask {
            gate.intersect(mask);
        }
        self.gate_mask = gate;
    }

    fn clear_masks(&mut self) {
        self.mask = Arc::new(Mask::none(0));
        self.frame_mask = None;
        self.region_mask = None;
        self.scope_mask = Mask::none(0);
        self.filter_masks.clear();
        self.lasso_mask = None;
        self.gate_mask = Mask::none(0);
    }

    fn scope_frame(&self) -> Option<u32> {
        (self.scope == Scope::Current && !matches!(self.group, Group::Track | Group::Molecule))
            .then_some(self.time)
    }

    fn refresh_plot(&mut self) {
        if self.table.is_none() || self.view.plot().is_none() {
            self.clear_plot();
            return;
        }
        self.plot_wanted = true;
    }

    fn clear_plot(&mut self) {
        self.plot = None;
        self.plot_wanted = false;
        self.plot_pending = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlotKey(u64);

pub struct PlotRequest {
    key: PlotKey,
    table: Arc<Table>,
    mask: Arc<Mask>,
    view: MeasureView,
    axes: [usize; 2],
    window: Option<Window>,
}

impl PlotRequest {
    pub fn key(&self) -> PlotKey {
        self.key
    }

    pub fn build(&self) -> Option<Plot> {
        let window = self.window?;
        self.table.plot(self.view, self.axes, &self.mask, window)
    }
}

pub fn molecule_grouping(scene: &Scene, set: ObjectSetId) -> bool {
    scene
        .objects
        .get(set)
        .and_then(|object| object.localization())
        .is_some_and(crate::LocalizationSource::clustered)
}

fn holds_row(region: Region, table: &Table, row: usize) -> bool {
    table
        .position(row)
        .is_none_or(|position| region.contains(Vec3::from(position)))
}

fn column_keys(table: &Table) -> Vec<String> {
    table
        .columns()
        .iter()
        .map(|column| column.key().to_string())
        .collect()
}

fn remap(previous: &[String], current: &[String], column: usize) -> Option<usize> {
    let key = previous.get(column)?;
    current.iter().position(|candidate| candidate == key)
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
                    || key.starts_with(grana_measure::NEIGHBOUR_KEY)
                    || key.ends_with("_inside")
                    || key.ends_with("_count")
                    || key == grana_measure::SCORE_KEY
            }
        })
        .collect()
}
