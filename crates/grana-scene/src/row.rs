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

pub struct Rows {
    spec: RowSpec,
    rows: Vec<usize>,
    admitted: Arc<Mask>,
    restricted: Arc<Mask>,
    total: usize,
}

impl Rows {
    pub fn build(table: &Table, spec: RowSpec) -> Self {
        let count = table.rows();
        let mut admitted = match spec.frame {
            Some(frame) => table.frame_mask(Some(frame)),
            None => Mask::all(count),
        };
        let mut restricted = match spec.region {
            Some(region) => mask_of(region, table),
            None => Mask::all(count),
        };
        admitted.intersect(&restricted);
        let total = admitted.count() as usize;
        for filter in &spec.filters {
            let mask = table.filter_mask(*filter);
            restricted.intersect(&mask);
            admitted.intersect(&mask);
        }
        if let Some(lasso) = &spec.lasso {
            let mask = table.lasso_mask(lasso);
            restricted.intersect(&mask);
            admitted.intersect(&mask);
        }
        let mut rows = admitted.to_rows();
        if let Some((column, descending)) = spec.sort
            && column < table.columns().len()
        {
            table.sort(&mut rows, column, descending);
        }
        Self {
            spec,
            rows,
            admitted: Arc::new(admitted),
            restricted: Arc::new(restricted),
            total,
        }
    }

    pub fn spec(&self) -> &RowSpec {
        &self.spec
    }

    pub fn rows(&self) -> &[usize] {
        &self.rows
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
}

impl RowsRequest {
    pub(crate) fn new(table: Arc<Table>, spec: RowSpec) -> Self {
        Self { table, spec }
    }

    pub fn build(self) -> Rows {
        Rows::build(&self.table, self.spec)
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
    Mask::of(count, |row| {
        let position = Vec3::new(
            table.value(row, x),
            table.value(row, y),
            table.value(row, z),
        );
        region.contains(position)
    })
}
