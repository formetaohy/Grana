use grana_object::Link;

use super::detection::Detections;
use super::link::Matching;

pub(super) struct Chain {
    points: Vec<usize>,
    start: u32,
    end: u32,
    incoming: u8,
    outgoing: u8,
}

impl Chain {
    pub fn points(&self) -> &[usize] {
        &self.points
    }

    pub fn start(&self) -> u32 {
        self.start
    }

    pub fn end(&self) -> u32 {
        self.end
    }

    pub fn incoming(&self) -> u8 {
        self.incoming
    }

    pub fn outgoing(&self) -> u8 {
        self.outgoing
    }
}

pub(super) struct Chains {
    chains: Vec<Chain>,
    links: Vec<(usize, usize, Link)>,
    stamps: Vec<u64>,
    epoch: u64,
}

impl Chains {
    pub fn build(detections: &Detections, matching: &Matching) -> Self {
        let mut chains: Vec<Chain> = Vec::new();
        let mut links = Vec::new();
        for first in 0..detections.len() {
            if matching.predecessor(first).is_some() {
                continue;
            }
            let mut points = Vec::new();
            let mut at = first;
            let mut incoming = 0u8;
            loop {
                points.push(at);
                match matching.successor(at) {
                    Some(next) if detections.frame(next) == detections.frame(at) + 1 => {
                        at = next;
                    }
                    next => {
                        let outgoing = u8::from(next.is_some());
                        let start = detections.frame(points[0]);
                        let end = detections.frame(at);
                        chains.push(Chain {
                            points: std::mem::take(&mut points),
                            start,
                            end,
                            incoming,
                            outgoing,
                        });
                        if incoming == 1 {
                            links.push((chains.len() - 2, chains.len() - 1, Link::Gap));
                        }
                        let Some(next) = next else {
                            break;
                        };
                        at = next;
                        incoming = 1;
                    }
                }
            }
        }
        Self {
            stamps: vec![1; chains.len()],
            chains,
            links,
            epoch: 1,
        }
    }

    pub fn len(&self) -> usize {
        self.chains.len()
    }

    pub fn chain(&self, index: usize) -> &Chain {
        &self.chains[index]
    }

    pub fn points(&self, index: usize) -> &[usize] {
        &self.chains[index].points
    }

    pub fn links(&self) -> &[(usize, usize, Link)] {
        &self.links
    }

    pub fn stamp(&self, index: usize) -> u64 {
        self.stamps[index]
    }

    fn touch(&mut self, index: usize) {
        self.epoch += 1;
        self.stamps[index] = self.epoch;
    }

    pub fn cut(&mut self, index: usize, frame: u32) -> usize {
        let chain = &mut self.chains[index];
        let position = (frame - chain.start) as usize;
        assert!(
            position > 0 && position < chain.points.len(),
            "a cut divides a chain between two of its frames"
        );
        let points = chain.points.split_off(position);
        let end = chain.end;
        chain.end = frame - 1;
        let outgoing = chain.outgoing;
        chain.outgoing = 0;
        self.chains.push(Chain {
            points,
            start: frame,
            end,
            incoming: 0,
            outgoing,
        });
        let tail = self.chains.len() - 1;
        self.touch(index);
        self.epoch += 1;
        self.stamps.push(self.epoch);
        for link in &mut self.links {
            if link.0 == index {
                link.0 = tail;
            }
        }
        tail
    }

    pub fn attach(&mut self, parent: usize, child: usize, kind: Link) {
        self.chains[parent].outgoing += 1;
        self.chains[child].incoming += 1;
        self.links.push((parent, child, kind));
        self.touch(parent);
        self.touch(child);
    }
}
