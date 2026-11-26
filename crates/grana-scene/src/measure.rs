use std::sync::Arc;

use grana_measure::{
    Aggregate, ColumnValues, Filter, Group, Lasso, Mask, MeasureView, Measured, Nearest, Plot,
    PlotAxis, Scope, Table, Window,
};

use grana_volume::Region;

use crate::object_set::{ObjectSetId, Visibility};
use crate::provenance::{FilterRange, LassoPath};
use crate::row::{RowSpec, Rows, RowsRequest};
use crate::scene::Scene;
use crate::state::{HASH_OFFSET, mix};
use crate::table_request::{BasisKey, MeasureKey, TableRequest};

const AXES: [&str; 3] = ["x", "y", "z"];

#[derive(Debug, PartialEq)]
pub struct MeasureState {
    pub set: Option<u64>,
    pub target: Option<u64>,
    pub group: Group,
    pub aggregate: Aggregate,
    pub scope: Scope,
    pub filters: Vec<FilterRange>,
    pub lasso: Option<LassoPath>,
    pub sort: Option<(String, bool)>,
    pub columns: Vec<(String, bool)>,
    pub visibility: bool,
    pub view: MeasureView,
    pub axes: Option<[String; 2]>,
}

pub struct Measure {
    set: Option<ObjectSetId>,
    target: Option<ObjectSetId>,
    target_table: Option<Arc<Table>>,
    target_key: Option<MeasureKey>,
    target_pending: Option<MeasureKey>,
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
    rows: Option<Arc<Rows>>,
    rows_pending: Option<RowSpec>,
    nearest: Option<([usize; 2], Nearest)>,
    generation: u64,
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
    pending_state: Option<MeasureState>,
    materialize_pending: Option<Vec<usize>>,
    deferred_filter: Option<FilterIntent>,
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
            target: None,
            target_table: None,
            target_key: None,
            target_pending: None,
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
            rows: None,
            rows_pending: None,
            nearest: None,
            generation: 0,
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
            pending_state: None,
            materialize_pending: None,
            deferred_filter: None,
            time: 0,
            dirty: false,
        }
    }

    pub fn set(&self) -> Option<ObjectSetId> {
        self.set
    }

    pub fn target(&self) -> Option<ObjectSetId> {
        self.target
    }

    pub fn set_target(&mut self, target: Option<ObjectSetId>, scene: &Scene) {
        let target = target.filter(|id| Some(*id) != self.set && scene.objects.get(*id).is_some());
        if self.target == target {
            return;
        }
        self.clear_target();
        self.target = target;
        self.dirty = true;
        self.refresh_plot();
    }

    fn clear_target(&mut self) {
        self.target = None;
        self.target_table = None;
        self.target_key = None;
        self.target_pending = None;
    }

    pub fn select(&mut self, set: ObjectSetId, scene: &Scene) {
        if self.set == Some(set) {
            return;
        }
        if self.target == Some(set) {
            self.clear_target();
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

    pub fn clear(&mut self) {
        if self.set.take().is_none() {
            return;
        }
        self.clear_target();
        self.basis = None;
        self.filters.clear();
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
        self.relayout();
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
        self.relayout();
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
        self.relayout();
    }

    pub fn total(&self) -> usize {
        self.rows.as_ref().map_or(0, |rows| rows.total())
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
        if self.view.spatial() && window.is_none() {
            return None;
        }
        let table = Arc::clone(self.table.as_ref()?);
        let mask = self.fresh_rows()?.admitted();
        let paired = match (self.view.targeted(), self.target) {
            (true, Some(_)) => Some(Arc::clone(self.target_table.as_ref()?)),
            _ => None,
        };
        self.plot_stamp += 1;
        let key = PlotKey(self.plot_stamp);
        self.plot_wanted = false;
        self.plot_pending = Some(key);
        Some(PlotRequest {
            key,
            table,
            mask,
            paired,
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
        self.refresh_nearest();
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
        self.pin();
        if self
            .lasso
            .as_ref()
            .is_some_and(|lasso| lasso.x() != self.axes[0] || lasso.y() != self.axes[1])
        {
            self.lasso = None;
            self.relayout();
        } else {
            self.refresh_nearest();
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
        self.relayout();
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
            .find(|index| !positional(table.columns(), *index))
            .unwrap_or(table.identity_columns());
        let Some((minimum, maximum)) = table.cached_extent(column) else {
            self.deferred_filter = Some(FilterIntent::Add(column));
            self.dirty = true;
            self.pin();
            return;
        };
        self.filters.push(Filter::new(column, minimum, maximum));
        self.relayout();
    }

    pub fn set_filter(&mut self, index: usize, filter: Filter) {
        let Some(slot) = self.filters.get_mut(index) else {
            return;
        };
        if *slot == filter {
            return;
        }
        *slot = filter;
        self.relayout();
    }

    pub fn remove_filter(&mut self, index: usize) {
        if index >= self.filters.len() {
            return;
        }
        self.filters.remove(index);
        self.relayout();
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
        if column >= table.columns().len() {
            return;
        }
        let Some((minimum, maximum)) = table.cached_extent(column) else {
            self.deferred_filter = Some(FilterIntent::Retarget { index, column });
            self.dirty = true;
            self.pin();
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
        self.relayout();
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

    pub fn rows(&self) -> &[u32] {
        self.rows.as_ref().map_or(&[], |rows| rows.rows())
    }

    pub fn nearest_row(&self, target: [f32; 2], pixels: [f32; 2]) -> Option<u32> {
        match self.nearest.as_ref() {
            Some((axes, nearest)) if *axes == self.axes => nearest.nearest(target, pixels),
            _ => None,
        }
    }

    pub fn current_row(&self, scene: &Scene) -> Option<u32> {
        let selection = scene.selection()?;
        if Some(selection.set) != self.set {
            return None;
        }
        let table = self.table.as_deref()?;
        let object = match selection.item {
            crate::Item::Object(id) => scene
                .objects
                .get(selection.set)
                .and_then(|set| set.frame_shared(scene.time()))
                .and_then(|frame| frame.index_of(id)),
            crate::Item::Track(_) => None,
        };
        table.row(
            Some(scene.time()),
            object,
            match selection.item {
                crate::Item::Track(track) => Some(track),
                crate::Item::Object(_) => None,
            },
            None,
        )
    }

    pub fn selected_row(&self, scene: &Scene) -> Option<u32> {
        let row = self.current_row(scene)?;
        self.fresh_rows()?.admits(row).then_some(row)
    }

    pub fn rows_pending(&self) -> bool {
        self.rows_pending.is_some()
    }

    pub fn visible(&self) -> &[bool] {
        &self.visible
    }

    pub fn set_column_visible(&mut self, column: usize, visible: bool) {
        let Some(slot) = self.visible.get_mut(column) else {
            return;
        };
        *slot = visible;
        self.pin();
    }

    pub fn show_all_columns(&mut self) {
        self.visible.fill(true);
        self.pin();
    }

    pub fn reset(&mut self) {
        self.set = None;
        self.clear_target();
        self.table = None;
        self.basis = None;
        self.retained = None;
        self.pending_state = None;
        self.materialize_pending = None;
        self.deferred_filter = None;
        self.clear_rows();
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
            self.clear_target();
            self.drop_table();
            self.dirty = true;
            return None;
        }
        let time = scene.time();
        let moved = self.time != time;
        self.time = time;
        if !busy && !self.settled(scene) {
            let Some(request) = MeasureKey::of(scene, id, self.group, self.aggregate)
                .zip(TableRequest::new(scene, id, self.group, self.aggregate))
            else {
                self.origin = None;
                self.pending = None;
                if self.table.is_some() {
                    self.drop_table();
                }
                return None;
            };
            let (key, request) = request;
            if self.group == Group::Object
                && let Some((held, table)) = self.basis.as_ref()
                && *held == key.basis()
            {
                self.pending = Some(key);
                self.install(key, Some(Arc::clone(table)));
                return None;
            }
            let request = self.laid(scene, id, request);
            self.pending = Some(request.key());
            return Some(request.with_retained(self.retained_keys()));
        }
        if moved && self.scope == Scope::Current {
            self.refresh_plot();
        }
        if !busy && self.settled(scene) {
            return self.target_request(scene);
        }
        None
    }

    fn target_request(&mut self, scene: &Scene) -> Option<TableRequest> {
        let target = self.target?;
        if !self.view.targeted() {
            return None;
        }
        if scene.objects.get(target).is_none() {
            self.set_target(None, scene);
            return None;
        }
        let key = MeasureKey::of(scene, target, Group::Object, Aggregate::MEAN)?;
        if self.target_key == Some(key) {
            return None;
        }
        let Some(request) = TableRequest::new(scene, target, Group::Object, Aggregate::MEAN) else {
            self.set_target(None, scene);
            return None;
        };
        self.target_pending = Some(key);
        Some(request)
    }

    pub fn install(&mut self, key: MeasureKey, table: Option<Arc<Table>>) -> Installed {
        if self.target_pending == Some(key) {
            self.target_pending = None;
            self.target_key = Some(key);
            self.target_table = table;
            self.refresh_plot();
            return Installed::Target;
        }
        if self.pending != Some(key) {
            return Installed::Ignored;
        }
        self.pending = None;
        self.origin = Some(key);
        if let Some(table) = table.as_ref() {
            self.remember_basis(key, table);
        }
        self.adopt(table);
        if let Some(state) = self.pending_state.take() {
            self.apply_state(state);
        }
        Installed::Table
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

    pub fn export(&self, scene: &Scene) -> Option<(Arc<Table>, Vec<u32>)> {
        if !self.settled(scene) {
            return None;
        }
        let table = Arc::clone(self.table.as_ref()?);
        let rows = self.current_rows()?;
        Some((table, rows.rows().to_vec()))
    }

    pub fn apply(&mut self, scene: &mut Scene) -> bool {
        if !self.dirty {
            return false;
        }
        let Some(id) = self.set else {
            self.dirty = false;
            return false;
        };
        if scene.objects.get(id).is_none() {
            self.dirty = false;
            return false;
        }
        let Some(visibility) = self.restriction(scene, id) else {
            return false;
        };
        self.dirty = false;
        scene.objects.set_visibility(id, visibility)
    }

    fn restriction(&self, scene: &Scene, id: ObjectSetId) -> Option<Visibility> {
        scene.objects.get(id)?;
        let roi = self.roi.filter(|_| self.group != Group::Track);
        if !self.visibility || (self.filters.is_empty() && self.lasso.is_none() && roi.is_none()) {
            return Some(Visibility::All);
        }
        let table = self.current(scene)?;
        if table.rows() == 0 {
            return Some(Visibility::All);
        }
        let mask = self.fresh_rows()?.restricted().clone();
        Some(match self.group {
            Group::Object => Visibility::objects(grana_object::ObjectIds::checked(
                (0..table.rows())
                    .filter(|row| mask.contains(*row))
                    .filter_map(|row| table.observation(row).object())
                    .collect(),
            )?),
            Group::Track => Visibility::Tracks(grana_object::TrackIds::checked(
                (0..table.rows())
                    .filter(|row| mask.contains(*row))
                    .filter_map(|row| table.observation(row).track())
                    .collect(),
            )?),
            Group::Molecule => Visibility::molecules(Arc::new(mask), table.molecule_rows()?),
            Group::Timepoint => Visibility::All,
        })
    }

    pub fn state(&self, scene: &Scene) -> MeasureState {
        let set = self
            .set
            .filter(|id| scene.objects.get(*id).is_some())
            .map(ObjectSetId::raw);
        let measured = self.table.is_some()
            && self
                .set
                .is_some_and(|id| TableRequest::available(scene, id, self.group));
        let names = match measured || self.table.is_none() {
            true => self.names(),
            false => Names::default(),
        };
        MeasureState {
            set,
            target: self.target.map(ObjectSetId::raw),
            group: self.group,
            aggregate: self.aggregate,
            scope: self.scope,
            filters: names.filters,
            lasso: names.lasso,
            sort: names.sort,
            columns: names.columns,
            visibility: self.visibility,
            view: self.view,
            axes: names.axes,
        }
    }

    fn names(&self) -> Names {
        match self.table.as_deref() {
            Some(table) => Names::of(
                table,
                &self.filters,
                self.lasso.as_ref(),
                self.sort,
                &self.visible,
                self.axes,
            ),
            None => self
                .pending_state
                .as_ref()
                .map_or_else(Names::default, |state| Names {
                    filters: state.filters.clone(),
                    lasso: state.lasso.clone(),
                    sort: state.sort.clone(),
                    columns: state.columns.clone(),
                    axes: state.axes.clone(),
                }),
        }
    }

    pub fn restore(&mut self, state: MeasureState, scene: &Scene) {
        self.set = None;
        self.group = state.group;
        self.aggregate = state.aggregate;
        self.scope = state.scope;
        self.roi = scene.region_scope();
        self.time = scene.time();
        self.visibility = state.visibility;
        self.view = state.view;
        self.drop_table();
        self.clear_target();
        self.origin = None;
        self.pending = None;
        self.materialize_pending = None;
        self.deferred_filter = None;
        self.dirty = true;
        let set = state
            .set
            .and_then(ObjectSetId::from_raw)
            .filter(|id| scene.objects.get(*id).is_some());
        self.set = set;
        self.target = state
            .target
            .and_then(ObjectSetId::from_raw)
            .filter(|id| Some(*id) != set && scene.objects.get(*id).is_some());
        self.pending_state = set.map(|_| state);
    }

    pub fn settle(&mut self, scene: &Scene) {
        let Some(id) = self.set else {
            self.drop_table();
            self.pending_state = None;
            return;
        };
        let Some(request) = TableRequest::new(scene, id, self.group, self.aggregate) else {
            self.drop_table();
            self.pending_state = None;
            return;
        };
        let request = self.laid(scene, id, request);
        let key = request.key();
        self.pending = Some(key);
        let table = request
            .with_retained(self.retained_keys())
            .build()
            .map(Arc::new);
        self.install(key, table);
    }

    fn apply_state(&mut self, state: MeasureState) {
        let Some(table) = self.table.as_ref().map(Arc::clone) else {
            return;
        };
        self.visible = default_visibility(&table);
        for (key, visible) in &state.columns {
            if let Some(column) = table.column(key) {
                self.visible[column] = *visible;
            }
        }
        self.filters = state
            .filters
            .iter()
            .filter_map(|filter| {
                Some(Filter::new(
                    table.column(filter.key())?,
                    filter.minimum(),
                    filter.maximum(),
                ))
            })
            .collect();
        self.sort = state
            .sort
            .and_then(|(key, descending)| table.column(&key).map(|column| (column, descending)));
        self.lasso = state.lasso.and_then(|lasso| {
            let x = table.column(lasso.x())?;
            let y = table.column(lasso.y())?;
            Lasso::new(x, y, lasso.points().to_vec())
        });
        self.axes = match state.axes {
            Some([x, y]) => match (table.column(&x), table.column(&y)) {
                (Some(x), Some(y)) => [x, y],
                _ => preferred_axes(&table),
            },
            None => preferred_axes(&table),
        };
        self.pin();
        self.refresh_nearest();
        self.refresh_plot();
        self.dirty = true;
    }

    pub fn document_hash(&self) -> u64 {
        let mut hash = HASH_OFFSET;
        hash = mix(hash, self.set.map_or(0, ObjectSetId::raw));
        hash = mix(hash, self.target.map_or(0, ObjectSetId::raw));
        hash = mix(hash, u64::from(self.group.index()));
        hash = mix(hash, u64::from(self.aggregate.index()));
        hash = mix(hash, u64::from(self.scope.index()));
        let names = self.names();
        for filter in &names.filters {
            hash = mix_key(hash, filter.key());
            hash = mix(hash, u64::from(filter.minimum().to_bits()));
            hash = mix(hash, u64::from(filter.maximum().to_bits()));
        }
        if let Some(lasso) = &names.lasso {
            hash = mix_key(hash, lasso.x());
            hash = mix_key(hash, lasso.y());
            hash = mix(hash, lasso.points().len() as u64);
            for point in lasso.points() {
                hash = mix(hash, u64::from(point[0].to_bits()));
                hash = mix(hash, u64::from(point[1].to_bits()));
            }
        }
        if let Some(roi) = self.roi {
            for value in roi
                .center()
                .to_array()
                .into_iter()
                .chain(roi.half().to_array())
            {
                hash = mix(hash, u64::from(value.to_bits()));
            }
            for axis in 0..3 {
                for value in roi.axis(axis).to_array() {
                    hash = mix(hash, u64::from(value.to_bits()));
                }
            }
        }
        if let Some((column, descending)) = &names.sort {
            hash = mix_key(hash, column);
            hash = mix(hash, u64::from(u8::from(*descending)));
        }
        for (key, visible) in &names.columns {
            hash = mix_key(hash, key);
            hash = mix(hash, u64::from(u8::from(*visible)));
        }
        hash = mix(hash, u64::from(u8::from(self.visibility)));
        hash = mix(hash, u64::from(self.view.index()));
        match &names.axes {
            Some([x, y]) => {
                hash = mix_key(hash, x);
                mix_key(hash, y)
            }
            None => mix(hash, 0),
        }
    }

    fn invalidate(&mut self) {
        self.dirty = true;
        self.pending = None;
        self.pending_state = None;
        self.materialize_pending = None;
        self.deferred_filter = None;
        self.origin = None;
        self.clear_plot();
        if let Some(table) = self.table.take() {
            let keys = column_keys(&table);
            self.retained = Some((keys, std::mem::take(&mut self.visible)));
        }
        self.clear_rows();
        self.lasso = None;
        self.axes = [0; 2];
    }

    fn adopt(&mut self, table: Option<Arc<Table>>) {
        self.dirty = true;
        self.materialize_pending = None;
        self.deferred_filter = None;
        self.clear_plot();
        self.clear_rows();
        let previous = self
            .table
            .take()
            .map(|table| (column_keys(&table), std::mem::take(&mut self.visible)))
            .or_else(|| self.retained.take());
        let Some(table) = table else {
            self.drop_table();
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
        self.generation += 1;
        self.table = Some(table);
        self.pin();
        self.refresh_nearest();
        self.refresh_plot();
    }

    fn spec(&self) -> Option<RowSpec> {
        Some(RowSpec::new(
            self.generation,
            self.scope_frame(),
            self.roi.filter(|_| self.group != Group::Track),
            self.filters.clone(),
            self.lasso.clone(),
            self.sort,
        ))
    }

    fn fresh_rows(&self) -> Option<&Rows> {
        let spec = self.spec()?;
        self.rows.as_deref().filter(|rows| rows.spec() == &spec)
    }

    pub fn current_rows(&self) -> Option<Arc<Rows>> {
        let table = self.table.as_ref()?;
        let spec = self.spec()?;
        match self.rows.as_ref().filter(|rows| rows.spec() == &spec) {
            Some(rows) => Some(Arc::clone(rows)),
            None => Some(Arc::new(Rows::build(table, spec, self.rows.as_deref()))),
        }
    }

    pub fn rows_request(&mut self) -> Option<RowsRequest> {
        let table = Arc::clone(self.table.as_ref()?);
        let spec = self.spec()?;
        if self.rows.as_ref().is_some_and(|rows| rows.spec() == &spec) {
            self.rows_pending = None;
            return None;
        }
        if self.rows_pending.as_ref() == Some(&spec) {
            return None;
        }
        self.rows_pending = Some(spec.clone());
        Some(RowsRequest::new(
            table,
            spec,
            self.rows.as_ref().map(Arc::clone),
        ))
    }

    pub fn install_rows(&mut self, rows: Rows) {
        if self.rows_pending.as_ref() == Some(rows.spec()) {
            self.rows_pending = None;
        }
        if self.spec().as_ref() != Some(rows.spec()) {
            return;
        }
        self.rows = Some(Arc::new(rows));
        self.dirty = true;
        self.refresh_nearest();
        self.refresh_plot();
    }

    pub fn materialize_request(&mut self) -> Option<MaterializeRequest> {
        let table = Arc::clone(self.table.as_ref()?);
        let missing = self
            .required_columns()
            .into_iter()
            .filter(|column| !table.cached(*column))
            .collect::<Vec<_>>();
        if missing.is_empty() {
            self.materialize_pending = None;
            self.commit_filter();
            return None;
        }
        if self.materialize_pending.as_ref() == Some(&missing) {
            return None;
        }
        self.materialize_pending = Some(missing.clone());
        Some(MaterializeRequest::new(table, missing))
    }

    pub fn install_materialized(&mut self, table: &Arc<Table>, values: ColumnValues) {
        if self
            .table
            .as_ref()
            .is_none_or(|held| !Arc::ptr_eq(held, table))
        {
            return;
        }
        table.install(values);
        if self
            .materialize_pending
            .as_ref()
            .is_some_and(|held| held.iter().all(|column| table.cached(*column)))
        {
            self.materialize_pending = None;
        }
        self.commit_filter();
        self.refresh_nearest();
        self.dirty = true;
    }

    fn commit_filter(&mut self) {
        let Some(intent) = self.deferred_filter else {
            return;
        };
        let Some(table) = self.table.as_deref() else {
            self.deferred_filter = None;
            return;
        };
        let Some((minimum, maximum)) = table.cached_extent(intent.column()) else {
            return;
        };
        self.deferred_filter = None;
        let filter = Filter::new(intent.column(), minimum, maximum);
        match intent {
            FilterIntent::Add(_) => self.filters.push(filter),
            FilterIntent::Retarget { index, .. } => {
                if let Some(slot) = self.filters.get_mut(index) {
                    *slot = filter;
                }
            }
        }
        self.relayout();
    }

    pub fn value(&self, row: usize, column: usize) -> Option<Measured> {
        self.table.as_deref()?.cached_value(row, column)
    }

    fn relayout(&mut self) {
        self.dirty = true;
        self.pin();
        self.refresh_plot();
    }

    fn pin(&self) {
        let Some(table) = self.table.as_deref() else {
            return;
        };
        table.retain(self.required_columns());
    }

    fn required_columns(&self) -> Vec<usize> {
        let Some(table) = self.table.as_deref() else {
            return Vec::new();
        };
        let count = table.columns().len();
        let mut columns = self
            .visible
            .iter()
            .enumerate()
            .filter_map(|(index, visible)| (*visible && index < count).then_some(index))
            .collect::<Vec<_>>();
        columns.extend(self.filters.iter().map(|filter| filter.column()));
        if let Some((column, _)) = self.sort {
            columns.push(column);
        }
        columns.extend(self.axes);
        if let Some(lasso) = &self.lasso {
            columns.push(lasso.x());
            columns.push(lasso.y());
        }
        if let Some(deferred) = self.deferred_filter {
            columns.push(deferred.column());
        }
        if self.roi.is_some_and(Region::enabled) {
            for key in position_keys(table) {
                if let Some(column) = table.column(&key) {
                    columns.push(column);
                }
            }
        }
        columns.retain(|column| *column < count);
        columns
    }

    fn retained_keys(&self) -> Vec<String> {
        let Some(table) = self.table.as_deref() else {
            return Vec::new();
        };
        let mut keys = Vec::new();
        for (index, visible) in self.visible.iter().enumerate() {
            if let Some(column) = table.columns().get(index)
                && *visible
            {
                keys.push(column.key().to_string());
            }
        }
        for filter in &self.filters {
            if let Some(column) = table.columns().get(filter.column()) {
                keys.push(column.key().to_string());
            }
        }
        let mut wanted = Vec::with_capacity(self.axes.len() + 1);
        if let Some((column, _)) = self.sort {
            wanted.push(column);
        }
        wanted.extend(self.axes);
        for column in wanted {
            if let Some(column) = table.columns().get(column) {
                keys.push(column.key().to_string());
            }
        }
        if let Some(lasso) = &self.lasso {
            for column in [lasso.x(), lasso.y()] {
                if let Some(column) = table.columns().get(column) {
                    keys.push(column.key().to_string());
                }
            }
        }
        if self.roi.is_some_and(Region::enabled) {
            keys.extend(position_keys(table));
        }
        keys
    }

    fn refresh_nearest(&mut self) {
        self.nearest = None;
        if self.view != MeasureView::Scatter {
            return;
        }
        let Some(table) = self.table.as_deref() else {
            return;
        };
        let Some(rows) = self.rows.as_deref() else {
            return;
        };
        if self.spec().as_ref() != Some(rows.spec()) {
            return;
        }
        if self.axes.iter().any(|column| !table.cached(*column)) {
            return;
        }
        if let Some(nearest) = Nearest::of(table, rows.rows(), self.axes) {
            self.nearest = Some((self.axes, nearest));
        }
    }

    fn clear_rows(&mut self) {
        self.rows = None;
        self.rows_pending = None;
        self.nearest = None;
    }

    fn drop_table(&mut self) {
        self.dirty = true;
        self.table = None;
        self.retained = None;
        self.pending_state = None;
        self.visible.clear();
        self.filters.clear();
        self.lasso = None;
        self.sort = None;
        self.axes = [0; 2];
        self.clear_rows();
        self.clear_plot();
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
enum FilterIntent {
    Add(usize),
    Retarget { index: usize, column: usize },
}

impl FilterIntent {
    fn column(self) -> usize {
        match self {
            Self::Add(column) => column,
            Self::Retarget { column, .. } => column,
        }
    }
}

pub struct MaterializeRequest {
    table: Arc<Table>,
    columns: Vec<usize>,
}

impl MaterializeRequest {
    fn new(table: Arc<Table>, columns: Vec<usize>) -> Self {
        Self { table, columns }
    }

    pub fn build(self) -> (Arc<Table>, ColumnValues) {
        let values = self.table.scan(&self.columns);
        (self.table, values)
    }
}

#[derive(Default)]
struct Names {
    filters: Vec<FilterRange>,
    lasso: Option<LassoPath>,
    sort: Option<(String, bool)>,
    columns: Vec<(String, bool)>,
    axes: Option<[String; 2]>,
}

impl Names {
    fn of(
        table: &Table,
        filters: &[Filter],
        lasso: Option<&Lasso>,
        sort: Option<(usize, bool)>,
        visible: &[bool],
        axes: [usize; 2],
    ) -> Self {
        let key = |column: usize| {
            table
                .columns()
                .get(column)
                .map(|column| column.key().to_string())
        };
        let axes = match (key(axes[0]), key(axes[1])) {
            (Some(x), Some(y)) => Some([x, y]),
            _ => None,
        };
        Self {
            filters: filters
                .iter()
                .filter_map(|filter| {
                    Some(FilterRange::new(
                        &key(filter.column())?,
                        filter.minimum(),
                        filter.maximum(),
                    ))
                })
                .collect(),
            lasso: lasso.and_then(|lasso| {
                LassoPath::new(&key(lasso.x())?, &key(lasso.y())?, lasso.points().to_vec())
            }),
            sort: sort.and_then(|(column, descending)| Some((key(column)?, descending))),
            columns: table
                .columns()
                .iter()
                .zip(visible)
                .map(|(column, visible)| (column.key().to_string(), *visible))
                .collect(),
            axes,
        }
    }
}

fn mix_key(hash: u64, key: &str) -> u64 {
    let mut hash = mix(hash, key.len() as u64);
    for byte in key.bytes() {
        hash = mix(hash, u64::from(byte));
    }
    hash
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlotKey(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Installed {
    Ignored,
    Table,
    Target,
}

pub struct PlotRequest {
    key: PlotKey,
    table: Arc<Table>,
    mask: Arc<Mask>,
    paired: Option<Arc<Table>>,
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
        self.table.plot(
            self.view,
            self.axes,
            &self.mask,
            window,
            self.paired.as_deref(),
        )
    }
}

pub fn molecule_grouping(scene: &Scene, set: ObjectSetId) -> bool {
    scene
        .objects
        .get(set)
        .and_then(|object| object.localization())
        .is_some_and(crate::LocalizationSource::clustered)
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
    let present = |key: &str| columns.iter().any(|column| column.key() == key);
    let length = grana_measure::length_key;
    let volume = grana_measure::volume_key;
    let family: Vec<String> = if present("contrast") {
        vec![length("diameter"), "contrast".to_string()]
    } else if present("junctions") {
        vec![length("length"), length("diameter")]
    } else {
        vec![
            volume("volume"),
            volume("hull_volume"),
            "sphericity".to_string(),
        ]
    };
    let fallback: Vec<String> = [
        length("diameter"),
        "contrast".to_string(),
        volume("volume"),
        grana_measure::area_key("area"),
        "sphericity".to_string(),
        volume("hull_volume"),
        length("max_diameter"),
        length("length"),
        grana_measure::speed_key("speed"),
        length("displacement"),
        "straightness".to_string(),
        "branches".to_string(),
        "terminals".to_string(),
        "junctions".to_string(),
        "nodes".to_string(),
    ]
    .to_vec();
    let mut chosen = Vec::with_capacity(2);
    for key in family.iter().chain(&fallback) {
        if chosen.len() == 2 {
            break;
        }
        let Some(index) = columns.iter().position(|column| column.key() == *key) else {
            continue;
        };
        if chosen.contains(&index) {
            continue;
        }
        chosen.push(index);
    }
    for index in identity..columns.len() {
        if chosen.len() == 2 {
            break;
        }
        if chosen.contains(&index) || positional(columns, index) {
            continue;
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
    position_key(&columns[index])
}

fn position_key(column: &grana_measure::Column) -> bool {
    AXES.contains(&column.label())
}

fn position_keys(table: &Table) -> Vec<String> {
    table
        .columns()
        .iter()
        .filter(|column| position_key(column))
        .map(|column| column.key().to_string())
        .collect()
}

fn default_visibility(table: &Table) -> Vec<bool> {
    let identity = table.identity_columns();
    table
        .columns()
        .iter()
        .enumerate()
        .map(|(index, column)| {
            if index < identity {
                return !matches!(
                    column.key(),
                    "first_frame" | "last_frame" | "nodes" | "straightness"
                );
            }
            highlighted(column)
        })
        .collect()
}

fn highlighted(column: &grana_measure::Column) -> bool {
    if column.unit().spatial() || column.key().contains("_intensity_mean") {
        return true;
    }
    let key = column.key();
    key.starts_with("contrast")
        || key == "truncated"
        || key.starts_with("branches")
        || key.starts_with("terminals")
        || key.starts_with("sphericity")
        || key.starts_with("pearson_")
        || key.starts_with("overlap_")
        || key.starts_with(grana_measure::SCORE_KEY)
        || key.ends_with("_inside")
        || key.ends_with("_count")
}

pub fn default_retained(table: &Table) -> Vec<usize> {
    default_visibility(table)
        .iter()
        .enumerate()
        .filter_map(|(index, visible)| visible.then_some(index))
        .collect()
}
