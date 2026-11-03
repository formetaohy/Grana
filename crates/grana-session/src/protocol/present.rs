use grana_measure::{Lasso, PlotAxis};
use grana_scene::{Measure, ObjectSetId, Scene};

use super::Protocol;

impl Protocol {
    pub fn present_measure(&self, sets: &[ObjectSetId], measure: &mut Measure, scene: &Scene) {
        let Some(index) = self.sets.iter().position(|set| set.measure.is_some()) else {
            return;
        };
        let Some(spec) = self.sets[index].measure.as_ref() else {
            return;
        };
        let Some(id) = sets.get(index).copied() else {
            return;
        };
        measure.select(id, scene);
        measure.set_group(spec.group(), scene);
        measure.set_aggregate(spec.aggregate(), scene);
        measure.sync(scene, false);
        let Some(table) = measure.table() else {
            return;
        };
        let identity = table.identity_columns();
        let columns = table
            .columns()
            .iter()
            .map(|column| column.key().to_string())
            .collect::<Vec<_>>();
        let visible = columns
            .iter()
            .enumerate()
            .map(|(index, key)| index < identity || spec.columns.contains(key))
            .collect::<Vec<_>>();
        let filters = spec
            .filters
            .iter()
            .filter_map(|filter| Some((table.column(&filter.column)?, filter)))
            .map(|(column, filter)| (column, filter.minimum, filter.maximum))
            .collect::<Vec<_>>();
        let sort = spec.order(table);
        let axes = spec
            .axes()
            .and_then(|[x, y]| Some([table.column(x)?, table.column(y)?]));
        let lasso = spec.lasso().and_then(|lasso| {
            let x = table.column(&lasso.x)?;
            let y = table.column(&lasso.y)?;
            Lasso::new(x, y, lasso.points.clone())
        });
        measure.show_all_columns();
        for (index, shown) in visible.iter().enumerate() {
            if !shown {
                measure.set_column_visible(index, false);
            }
        }
        for (column, minimum, maximum) in filters {
            measure.set_gate(column, minimum, maximum);
        }
        measure.set_sort(sort);
        measure.set_scope(spec.scope());
        measure.set_visibility(spec.visible());
        if let Some(view) = spec.view() {
            measure.set_view(view);
        }
        if let Some([x, y]) = axes {
            measure.set_axis(PlotAxis::X, x);
            measure.set_axis(PlotAxis::Y, y);
        }
        if let Some(lasso) = lasso {
            measure.set_lasso(Some(lasso));
        }
    }
}
