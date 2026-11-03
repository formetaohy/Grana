use super::Protocol;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Detect { set: usize, mask: Option<usize> },
    Relation { source: usize, target: usize },
    Coloc { pair: usize, mask: Option<usize> },
    Present,
}

impl Protocol {
    pub(crate) fn steps(&self) -> Vec<Step> {
        let position = |name: &str| {
            self.sets
                .iter()
                .position(|set| set.name == name)
                .expect("a checked protocol names its own object sets")
        };
        let mut steps = self
            .sets
            .iter()
            .enumerate()
            .map(|(index, set)| Step::Detect {
                set: index,
                mask: set.mask.as_deref().map(position),
            })
            .collect::<Vec<_>>();
        for relation in &self.relations {
            steps.push(Step::Relation {
                source: position(&relation.source),
                target: position(&relation.target),
            });
        }
        if let Some(coloc) = &self.coloc {
            let mask = coloc.mask.as_deref().map(position);
            for pair in 0..coloc.pairs.len() {
                steps.push(Step::Coloc { pair, mask });
            }
        }
        steps.push(Step::Present);
        steps
    }
}
