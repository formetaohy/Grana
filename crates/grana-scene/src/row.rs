use std::sync::Arc;

use glam::Vec3;
use grana_measure::{Filter, Lasso, Mask, Table};
use grana_volume::Region;

#[derive(Clone, Debug, PartialEq)]
pub struct RowSpec {
    generation: u64,
    frame: Option<u32>,
    region: Option<Region>,
    filters: Vec<Filter>,
    lasso: Option<Lasso>,
    sort: Option<(usize, bool)>,
}

impl RowSpec {
    pub(crate) fn new(
        generation: u64,
        frame: Option<u32>,
        region: Option<Region>,
        filters: Vec<Filter>,
        lasso: Option<Lasso>,
        sort: Option<(usize, bool)>,
    ) -> Self {
        Self {
            generation,
            frame,
            region,
            filters,
            lasso,
            sort,
        }
    }
}

#[derive(Clone)]
struct Order {
    sort: (usize, bool),
    rows: Arc<Vec<u32>>,
}

#[derive(Clone)]
struct Masks {
    generation: u64,
    frame: Option<(Option<u32>, Arc<Mask>)>,
    region: Option<(Option<Region>, Arc<Mask>)>,
    filters: Vec<(Filter, Arc<Mask>)>,
    lasso: Option<(Lasso, Arc<Mask>)>,
}

impl Masks {
    fn empty(generation: u64) -> Self {
        Self {
            generation,
            frame: None,
            region: None,
            filters: Vec::new(),
            lasso: None,
        }
    }

    fn reused(previous: &Rows, spec: &RowSpec) -> Option<Self> {
        (previous.masks.generation == spec.generation).then(|| Self {
            generation: spec.generation,
            frame: previous
                .masks
                .frame
                .clone()
                .filter(|(time, _)| *time == spec.frame),
            region: previous
                .masks
                .region
                .clone()
                .filter(|(region, _)| *region == spec.region),
            filters: spec
                .filters
                .iter()
                .filter_map(|filter| {
                    previous
                        .masks
                        .filters
                        .iter()
                        .find(|(held, _)| held == filter)
                        .map(|(held, mask)| (*held, Arc::clone(mask)))
                })
                .collect(),
            lasso: previous
                .masks
                .lasso
                .clone()
                .filter(|(lasso, _)| spec.lasso.as_ref() == Some(lasso)),
        })
    }
}

pub struct Rows {
    spec: RowSpec,
    rows: Vec<u32>,
    admitted: Arc<Mask>,
    restricted: Arc<Mask>,
    total: usize,
    order: Option<Order>,
    masks: Masks,
}

impl Rows {
    pub fn build(table: &Table, spec: RowSpec, previous: Option<&Rows>) -> Self {
        let count = table.rows();
        let mut masks = previous
            .filter(|rows| rows.spec.generation == spec.generation)
            .and_then(|rows| Masks::reused(rows, &spec))
            .unwrap_or_else(|| Masks::empty(spec.generation));
        let mut order = previous
            .filter(|rows| rows.spec.generation == spec.generation)
            .and_then(|rows| rows.order.clone())
            .filter(|order| Some(order.sort) == spec.sort);
        if let Some(frame) = spec.frame
            && masks.frame.is_none()
        {
            masks.frame = Some((Some(frame), Arc::new(table.frame_mask(Some(frame)))));
        }
        if let Some(region) = spec.region
            && masks.region.is_none()
        {
            masks.region = Some((Some(region), Arc::new(mask_of(region, table))));
        }
        let filters = spec
            .filters
            .iter()
            .map(
                |filter| match masks.filters.iter().find(|(held, _)| held == filter) {
                    Some((held, mask)) => (*held, Arc::clone(mask)),
                    None => (*filter, Arc::new(table.filter_mask(*filter))),
                },
            )
            .collect::<Vec<_>>();
        masks.filters = filters;
        if let Some(lasso) = &spec.lasso
            && masks.lasso.is_none()
        {
            masks.lasso = Some((lasso.clone(), Arc::new(table.lasso_mask(lasso))));
        }
        let mut admitted = masks
            .frame
            .as_ref()
            .map_or_else(|| Mask::all(count), |(_, mask)| mask.as_ref().clone());
        let mut restricted = masks
            .region
            .as_ref()
            .map_or_else(|| Mask::all(count), |(_, mask)| mask.as_ref().clone());
        admitted.intersect(&restricted);
        let total = admitted.count() as usize;
        for (_, mask) in &masks.filters {
            restricted.intersect(mask);
            admitted.intersect(mask);
        }
        if let Some((_, mask)) = &masks.lasso {
            restricted.intersect(mask);
            admitted.intersect(mask);
        }
        let rows = match spec.sort {
            Some((column, descending)) if column < table.columns().len() => {
                let sorted = match order.take() {
                    Some(order) => order.rows,
                    None => {
                        let mut rows = (0..count as u32).collect::<Vec<_>>();
                        table.sort(&mut rows, column, descending);
                        Arc::new(rows)
                    }
                };
                let rows = sorted
                    .iter()
                    .copied()
                    .filter(|row| admitted.contains(*row as usize))
                    .collect::<Vec<_>>();
                order = Some(Order {
                    sort: (column, descending),
                    rows: sorted,
                });
                rows
            }
            _ => admitted.to_rows(),
        };
        Self {
            spec,
            rows,
            admitted: Arc::new(admitted),
            restricted: Arc::new(restricted),
            total,
            order,
            masks,
        }
    }

    pub fn spec(&self) -> &RowSpec {
        &self.spec
    }

    pub fn rows(&self) -> &[u32] {
        &self.rows
    }

    pub fn admits(&self, row: u32) -> bool {
        self.admitted.contains(row as usize)
    }

    pub fn admitted(&self) -> Arc<Mask> {
        Arc::clone(&self.admitted)
    }

    pub fn restricted(&self) -> &Mask {
        &self.restricted
    }

    pub fn total(&self) -> usize {
        self.total
    }
}

pub struct RowsRequest {
    table: Arc<Table>,
    spec: RowSpec,
    previous: Option<Arc<Rows>>,
}

impl RowsRequest {
    pub(crate) fn new(table: Arc<Table>, spec: RowSpec, previous: Option<Arc<Rows>>) -> Self {
        Self {
            table,
            spec,
            previous,
        }
    }

    pub fn build(self) -> Rows {
        Rows::build(&self.table, self.spec, self.previous.as_deref())
    }
}

fn mask_of(region: Region, table: &Table) -> Mask {
    let count = table.rows();
    let (Some(x), Some(y), Some(z)) = (
        table.column("x_um"),
        table.column("y_um"),
        table.column("z_um"),
    ) else {
        return Mask::all(count);
    };
    let xs = table.materialize(x);
    let ys = table.materialize(y);
    let zs = table.materialize(z);
    Mask::of(count, |row| {
        let position = Vec3::new(xs[row], ys[row], zs[row]);
        region.contains(position)
    })
}
